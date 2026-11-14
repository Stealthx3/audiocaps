#!/usr/bin/env bash
# lab.sh — VM Lab Standard v1.2. Manage libvirt lab guests for this project.
# Usage: scripts/lab.sh <doctor|up|view|ip|ssh|exec|push|run|pull|put|snap|revert|snaps|down|destroy|list> [args]
#   up [role]            create/start lab-<project>-<role> (default role: main)
#   view [role]          open the guest's console in Virtual Machine Manager
#   exec [-r role] CMD   run CMD in guest (in $LAB_DEST), mirrored to the live console
#   run  [-r role] CMD   push working tree, then exec CMD
#   push [role] / pull GUEST_PATH HOST_DIR [role] / put HOST_PATH GUEST_DIR [role]
#   snap NAME [role] / revert NAME [role] / snaps [role]
#   down [role] / destroy role / list
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ENVF="${LAB_ENV:-lab.env}"; [[ "$ENVF" == /* ]] || ENVF="$ROOT/$ENVF"   # LAB_ENV=lab.lxc.env selects another config
[ -f "$ENVF" ] || { echo "lab: missing $ENVF (see VM-LAB-STANDARD §5)" >&2; exit 2; }
# shellcheck disable=SC1090
source "$ENVF"

: "${LAB_PROJECT:?}" "${LAB_URI:=qemu:///system}" "${LAB_KIND:=vm}" "${LAB_NET:=default}"
: "${LAB_POOL:=default}" "${LAB_BASE_VOL:=}" "${LAB_OS_VARIANT:=debian12}" "${LAB_VCPUS:=2}"
: "${LAB_RAM_MB:=4096}" "${LAB_DISK_GB:=20}" "${LAB_USER:=lab}" "${LAB_SSH_KEY:=$HOME/.ssh/id_ed25519.pub}"
: "${LAB_DEST:=/home/$LAB_USER/src}" "${LAB_HOST_RESERVE_MB:=4096}" "${LAB_LXC_ROOTFS:=}"
: "${LAB_GRAPHICS:=spice,listen=none}" "${LAB_AUTO_VIEW:=1}"

[ "$LAB_KIND" = lxc ] && URI="lxc:///" || URI="$LAB_URI"
V() { virsh -q -c "$URI" "$@"; }
KEY="${LAB_SSH_KEY%.pub}"
SSH_OPTS=(-i "$KEY" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -o ConnectTimeout=5)

die() { echo "lab: $*" >&2; exit 1; }
name_of() { echo "lab-${LAB_PROJECT}-${1:-main}"; }
guard() { [[ "$1" == lab-* ]] || die "refusing to touch non-lab domain '$1'"; }
exists() { V dominfo "$1" >/dev/null 2>&1; }
state() { V domstate "$1" 2>/dev/null | head -n1; }

ip_of() {
  local n="$1" ip=""
  for _ in $(seq 1 60); do
    ip="$(V domifaddr "$n" --source lease 2>/dev/null | awk '/ipv4/ {split($4,a,"/"); print a[1]; exit}')"
    [ -n "$ip" ] && { echo "$ip"; return 0; }
    sleep 2
  done
  return 1
}

gexec() { # gexec NAME CMD — runs via guest's lab-run (mirrors to console) when present
  local n="$1"; shift
  local q; q="$(printf %q "$*")"
  local remote="if command -v lab-run >/dev/null 2>&1; then lab-run $q; else bash -c $q; fi"
  if [ "$LAB_KIND" = lxc ]; then
    sudo virsh -c lxc:/// lxc-enter-namespace "$n" --noseclabel /bin/bash -c "$remote"
  else
    local ip; ip="$(ip_of "$n")" || die "no IP for $n (is it running?)"
    ssh "${SSH_OPTS[@]}" "$LAB_USER@$ip" "$remote"
  fi
}

wait_ssh() {
  local n="$1" ip; ip="$(ip_of "$n")" || die "no DHCP lease for $n after 120s"
  for _ in $(seq 1 60); do
    ssh "${SSH_OPTS[@]}" "$LAB_USER@$ip" true 2>/dev/null && return 0
    sleep 2
  done
  die "SSH to $n ($ip) not ready after 120s"
}

check_ram() {
  local avail; avail="$(awk '/MemAvailable/ {print int($2/1024)}' /proc/meminfo)"
  (( avail - LAB_RAM_MB >= LAB_HOST_RESERVE_MB )) || \
    die "not enough host RAM: ${avail}MB free, guest wants ${LAB_RAM_MB}MB, reserve ${LAB_HOST_RESERVE_MB}MB"
}

cmd_view() {
  local n; n="$(name_of "${1:-main}")"; guard "$n"
  command -v virt-manager >/dev/null || die "virt-manager not installed on host"
  [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ] || die "no graphical session here; open virt-manager and double-click $n"
  nohup virt-manager --connect "$URI" --show-domain-console "$n" >/dev/null 2>&1 &
  echo "view: opened $n in Virtual Machine Manager ($URI)"
}

auto_view() { [ "$LAB_AUTO_VIEW" = 1 ] && { ( cmd_view "$1" ) || true; }; return 0; }

user_data() { # cloud-config: user, key, live-console feed on tty1/ttyS0, lab-run helper
  cat <<'EOF' | sed -e "s|@USER@|$LAB_USER|g" -e "s|@NAME@|$1|g" -e "s|@KEY@|$(cat "$LAB_SSH_KEY")|g"
#cloud-config
hostname: @NAME@
users:
  - name: @USER@
    sudo: ALL=(ALL) NOPASSWD:ALL
    shell: /bin/bash
    ssh_authorized_keys:
      - @KEY@
package_update: true
packages: [rsync, git, ca-certificates]
write_files:
  - path: /usr/local/bin/lab-run
    permissions: '0755'
    content: |
      #!/bin/bash
      # lab-run CMD: run CMD, mirror it to the live console log, keep its exit code.
      log=/var/log/lab/run.log; [ -w "$log" ] || log=/dev/null
      printf '\n\033[1;36m[%s] $ %s\033[0m\n' "$(date +%T)" "$*" >>"$log"
      bash -c "$*" 2>&1 | tee -a "$log"
      rc=${PIPESTATUS[0]}
      printf '\033[2m[exit %s]\033[0m\n' "$rc" >>"$log"
      exit "$rc"
  - path: /etc/systemd/system/getty@tty1.service.d/lab-autologin.conf
    content: |
      [Service]
      ExecStart=
      ExecStart=-/sbin/agetty --autologin @USER@ --noclear %I $TERM
  - path: /etc/systemd/system/serial-getty@ttyS0.service.d/lab-autologin.conf
    content: |
      [Service]
      ExecStart=
      ExecStart=-/sbin/agetty --autologin @USER@ --keep-baud 115200,57600,38400,9600 %I $TERM
  - path: /etc/profile.d/zz-lab-console.sh
    content: |
      # Live feed of lab commands on the virt-manager console. Ctrl-C for a shell.
      case "$(tty 2>/dev/null)" in
        /dev/tty1|/dev/ttyS0)
          if [ "$(id -un)" = "@USER@" ] && [ -z "${LAB_CONSOLE_SHELL:-}" ]; then
            export LAB_CONSOLE_SHELL=1
            clear
            echo "== @NAME@ : live log of lab commands (Ctrl-C for a shell) =="
            tail -n 200 -F /var/log/lab/run.log
          fi ;;
      esac
runcmd:
  - install -d -o @USER@ -m 0755 /var/log/lab
  - install -o @USER@ -m 0644 /dev/null /var/log/lab/run.log
  - systemctl daemon-reload
  - systemctl restart getty@tty1.service serial-getty@ttyS0.service || true
EOF
}

cmd_doctor() {
  local bad=0
  ok() { printf '  ok   %s\n' "$*"; }
  no() { printf '  FAIL %s\n' "$*"; bad=1; }
  wa() { printf '  warn %s\n' "$*"; }
  echo "lab doctor ($LAB_PROJECT, kind=$LAB_KIND)"
  for b in virsh virt-install qemu-img rsync ssh; do command -v "$b" >/dev/null && ok "$b" || no "$b not installed"; done
  command -v virt-manager >/dev/null && ok "virt-manager (live viewing)" || wa "virt-manager not installed: guests run but can't be watched"
  if command -v virt-install >/dev/null; then
    local vi; vi="$(virt-install --version 2>/dev/null || echo 0)"
    [ "${vi%%.*}" -ge 3 ] 2>/dev/null && ok "virt-install $vi (cloud-init ok)" || no "virt-install $vi < 3.0 (no --cloud-init)"
  fi
  [ -r /dev/kvm ] && [ -w /dev/kvm ] && ok "/dev/kvm usable" || no "/dev/kvm missing or no access (kvm group?)"
  if virsh -q -c "$LAB_URI" version >/dev/null 2>&1; then ok "connect $LAB_URI (shown in virt-manager as QEMU/KVM)"
  else no "cannot connect $LAB_URI without sudo (libvirt group? libvirtd running?)"; fi
  virsh -q -c "$LAB_URI" net-info "$LAB_NET" 2>/dev/null | grep 'Active:.*yes' >/dev/null && ok "network $LAB_NET active" || no "network $LAB_NET not active"
  virsh -q -c "$LAB_URI" pool-info "$LAB_POOL" 2>/dev/null | grep 'State:.*running' >/dev/null && ok "pool $LAB_POOL running" || no "pool $LAB_POOL not running"
  if [ "$LAB_KIND" = vm ]; then
    [ -n "$LAB_BASE_VOL" ] && virsh -q -c "$LAB_URI" vol-info --pool "$LAB_POOL" "$LAB_BASE_VOL" >/dev/null 2>&1 \
      && ok "base volume $LAB_BASE_VOL" || no "base volume '$LAB_BASE_VOL' not in pool $LAB_POOL (VM-LAB-STANDARD §4)"
  fi
  [ -f "$LAB_SSH_KEY" ] && [ -f "$KEY" ] && ok "ssh key $LAB_SSH_KEY" || no "ssh key pair $KEY(.pub) missing"
  virsh -q -c lxc:/// version >/dev/null 2>&1 && ok "LXC driver available (virt-manager: File > Add Connection > LXC)" || wa "LXC driver not available (VMs only)"
  if [ "$LAB_KIND" = lxc ]; then [ -d "$LAB_LXC_ROOTFS" ] && ok "LXC rootfs $LAB_LXC_ROOTFS" || no "LAB_LXC_ROOTFS not a directory"; fi
  [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ] && ok "graphical session (lab.sh view works)" || wa "no DISPLAY here: lab.sh view can't open windows; open virt-manager manually"
  echo "  host: $(nproc) CPUs, $(awk '/MemTotal/ {print int($2/1048576)}' /proc/meminfo) GB RAM, $(awk '/MemAvailable/ {print int($2/1024)}' /proc/meminfo) MB free"
  echo "  lab guests: $(virsh -q -c "$LAB_URI" list --all --name 2>/dev/null | grep -c '^lab-' || true)"
  [ $bad -eq 0 ] && echo "doctor: OK" || { echo "doctor: FAILED"; exit 1; }
}

cmd_up() {
  local role="${1:-main}" n; n="$(name_of "$role")"; guard "$n"
  local meta="title=lab $LAB_PROJECT ($role),description=lab:$LAB_PROJECT"
  if exists "$n"; then
    [ "$(state "$n")" = running ] || { check_ram; V start "$n" >/dev/null; }
    auto_view "$role"
    if [ "$LAB_KIND" = vm ]; then wait_ssh "$n"; echo "up: $n running"; echo "ip: $(ip_of "$n")"
    else echo "up: $n running (lxc)"; fi
    return 0
  fi
  check_ram
  if [ "$LAB_KIND" = lxc ]; then
    local fs="/var/lib/libvirt/lxc/$n"
    echo "lab: creating LXC rootfs copy at $fs (needs sudo)"
    sudo mkdir -p "$fs" && sudo cp -a "$LAB_LXC_ROOTFS/." "$fs/"
    virt-install --connect lxc:/// --name "$n" --memory "$LAB_RAM_MB" --vcpus "$LAB_VCPUS" \
      --filesystem "$fs,/" --network "network=$LAB_NET" --metadata "$meta" \
      --console pty --noautoconsole >/dev/null
    auto_view "$role"
    echo "up: $n (lxc) created; in virt-manager use the LXC connection"; return 0
  fi
  local ud md; ud="$(mktemp)"; md="$(mktemp)"; trap 'rm -f "$ud" "$md"' RETURN
  user_data "$n" >"$ud"; printf 'instance-id: %s\nlocal-hostname: %s\n' "$n" "$n" >"$md"
  V vol-create-as "$LAB_POOL" "$n.qcow2" "${LAB_DISK_GB}G" --format qcow2 \
    --backing-vol "$LAB_BASE_VOL" --backing-vol-format qcow2 >/dev/null
  virt-install --connect "$LAB_URI" --name "$n" --memory "$LAB_RAM_MB" --vcpus "$LAB_VCPUS" \
    --cpu host-passthrough --os-variant "$LAB_OS_VARIANT" --import \
    --disk "vol=$LAB_POOL/$n.qcow2,bus=virtio" --network "network=$LAB_NET,model=virtio" \
    --cloud-init "user-data=$ud,meta-data=$md" --graphics "$LAB_GRAPHICS" --serial pty --console pty,target_type=serial \
    --metadata "$meta" --noautoconsole >/dev/null
  auto_view "$role"
  wait_ssh "$n"
  gexec "$n" "cloud-init status --wait >/dev/null 2>&1 || true; mkdir -p '$LAB_DEST'"
  V snapshot-create-as "$n" clean --atomic >/dev/null && echo "snapshot: clean"
  echo "up: $n created (virt-manager: QEMU/KVM > $n)"; echo "ip: $(ip_of "$n")"
}

parse_role() { ROLE=main; if [ "${1:-}" = -r ]; then ROLE="$2"; shift 2; fi; ARGS=("$@"); }

cmd_push() {
  local n; n="$(name_of "${1:-main}")"; guard "$n"
  [ "$LAB_KIND" = vm ] || die "push: not supported for lxc; copy into /var/lib/libvirt/lxc/$n with sudo"
  local ip; ip="$(ip_of "$n")" || die "no IP for $n"
  ssh "${SSH_OPTS[@]}" "$LAB_USER@$ip" "mkdir -p '$LAB_DEST'"
  rsync -az --delete --exclude .git --filter=':- .gitignore' \
    -e "ssh ${SSH_OPTS[*]}" "$ROOT/" "$LAB_USER@$ip:$LAB_DEST/"
}

cmd_exec() { parse_role "$@"; local n; n="$(name_of "$ROLE")"; guard "$n"; gexec "$n" "cd '$LAB_DEST' && ${ARGS[*]}"; }
cmd_run()  { parse_role "$@"; cmd_push "$ROLE"; local n; n="$(name_of "$ROLE")"; gexec "$n" "cd '$LAB_DEST' && ${ARGS[*]}"; }

cmd_pull() {
  local src="${1:?guest path}" dst="${2:?host dir}" n; n="$(name_of "${3:-main}")"; guard "$n"
  local ip; ip="$(ip_of "$n")" || die "no IP for $n"
  mkdir -p "$dst"
  rsync -az -e "ssh ${SSH_OPTS[*]}" "$LAB_USER@$ip:$src" "$dst/"
}

cmd_put() {
  local src="${1:?host path}" dst="${2:?guest dir}" n; n="$(name_of "${3:-main}")"; guard "$n"
  local ip; ip="$(ip_of "$n")" || die "no IP for $n"
  ssh "${SSH_OPTS[@]}" "$LAB_USER@$ip" "mkdir -p '$dst'"
  rsync -az -e "ssh ${SSH_OPTS[*]}" "$src" "$LAB_USER@$ip:$dst/"
}

cmd_snap()   { local s="${1:?name}" n; n="$(name_of "${2:-main}")"; guard "$n"; [[ "$s" =~ ^[a-z0-9-]+$ ]] || die "bad snapshot name"; V snapshot-create-as "$n" "$s" --atomic >/dev/null; echo "snapshot: $s"; }
cmd_revert() { local s="${1:?name}" n; n="$(name_of "${2:-main}")"; guard "$n"; V snapshot-revert "$n" "$s" --running; wait_ssh "$n"; echo "reverted: $n -> $s"; }
cmd_snaps()  { local n; n="$(name_of "${1:-main}")"; guard "$n"; V snapshot-list "$n" --name; }
cmd_ip()     { local n; n="$(name_of "${1:-main}")"; guard "$n"; ip_of "$n"; }
cmd_ssh()    { local n; n="$(name_of "${1:-main}")"; guard "$n"; exec ssh "${SSH_OPTS[@]}" "$LAB_USER@$(ip_of "$n")"; }

cmd_down() {
  local n; n="$(name_of "${1:-main}")"; guard "$n"
  [ "$(state "$n")" = running ] || { echo "down: $n not running"; return 0; }
  V shutdown "$n" >/dev/null
  for _ in $(seq 1 30); do [ "$(state "$n")" = running ] || { echo "down: $n"; return 0; }; sleep 2; done
  V destroy "$n" >/dev/null; echo "down: $n (forced)"
}

cmd_destroy() {
  local n; n="$(name_of "${1:?role (explicit, e.g. main)}")"; guard "$n"
  exists "$n" || die "$n does not exist"
  [ "$(state "$n")" = running ] && V destroy "$n" >/dev/null
  if [ "$LAB_KIND" = lxc ]; then
    V undefine "$n" >/dev/null; echo "destroyed: $n (rootfs left at /var/lib/libvirt/lxc/$n; remove with sudo)"
  else
    for s in $(V snapshot-list "$n" --name 2>/dev/null); do V snapshot-delete "$n" "$s" >/dev/null; done
    V undefine "$n" --nvram >/dev/null 2>&1 || V undefine "$n" >/dev/null
    V vol-delete --pool "$LAB_POOL" "$n.qcow2" >/dev/null 2>&1 || true
    echo "destroyed: $n (overlay removed; base untouched)"
  fi
}

cmd_list() { V list --all | awk '$2 ~ /^lab-/'; }

c="${1:-}"; shift || true
case "$c" in
  doctor|up|view|ip|ssh|exec|push|run|pull|put|snap|revert|snaps|down|destroy|list) "cmd_$c" "$@" ;;
  *) sed -n '2,12p' "$0"; exit 2 ;;
esac
