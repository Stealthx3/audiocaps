# VM Lab Standard (v1.2)

> **Instruction to Claude:** If this file is in a project, **all building, running and testing of project
> code happens inside a lab guest (KVM VM or LXC container) managed by the host's libvirt**, never
> directly on the host. Every guest must be **visible and watchable in Virtual Machine Manager**
> (virt-manager) so the user can inspect the work live (§7). Choose VM or LXC for this project per §2,
set up the lab per §3 on first use, record it
> in `docs/LAB.md`, and route every build/test/run command through `scripts/lab.sh` for the rest of the
> project. Nobody has to ask again. If `docs/LAB.md` already exists, read it at the start of each session
> and reuse the guests it lists.

---

## 1. What it is

A fixed way to keep development and test execution off the workstation. The host is a personal machine
(desktop apps, games, personal data), so anything that installs packages, runs services, runs untrusted or
half-finished code, needs root, or might break the system runs in a disposable guest. The user can watch
it happen.

| Piece | Path | Role |
|---|---|---|
| Lab script | `scripts/lab.sh` | Copied **verbatim** from §10. The only way Claude touches guests. |
| Lab config | `lab.env` (repo root) | Per-project settings (§5). Sourced by the script. |
| Lab record | `docs/LAB.md` | What exists: guests, base image, snapshots, quirks found. |

**On the host, Claude may:** read/edit files, use git, run linters/formatters that don't execute project
code, run `scripts/lab.sh`, and run the tooling scripts of other ai-assets standards that only read
project records and execute no project code (e.g. `scripts/progress.mjs`, `scripts/postmortem.mjs`).
If such a script ever needs to build, install or run project code, that part goes in a guest. **Everything else** (package installs, `npm install`, `pip install`,
`make`, `cargo build`, test suites, dev servers, containers, systemd units, anything with `sudo`) runs in
a guest via `lab.sh run` / `lab.sh exec`.

Exceptions need the user's explicit OK and a line in `docs/LAB.md` → *Host exceptions* (e.g. "GUI app
must be tested on the real desktop", "GPU passthrough not set up, CUDA build runs on host").

## 2. Guest selection: VM or LXC (decide before setup)

Before writing `lab.env`, Claude decides which guest type fits **the project this file is in**. The decision
is made per **role** (a project with a build guest and a service-test guest can use one of each), written
to `docs/LAB.md` → *Guest decision*, and set in `lab.env` as `LAB_KIND`.

### 2.1 Gather the facts (one cheap pass)

Read only what's needed: the project brief / `CLAUDE.md` / `docs/PLAN.md`, the README, and the build and
dependency manifests (`package.json`, `pyproject.toml`, `Cargo.toml`, `go.mod`, `Makefile`, `Dockerfile`,
`*.service`, CI workflow files). From them, answer:

| # | Question | Typical evidence |
|---|---|---|
| Q1 | Does the work touch the **kernel or boot**? (modules, drivers, sysctl, kernel version, bootloader, initramfs, filesystems/mounts, disk partitioning) | `.ko`/DKMS, `modprobe`, `mount`, `mkfs`, `grub`, `dracut` |
| Q2 | Does it need **systemd as PID 1 or real init behavior**? (installing/enabling services, timers, reboot tests) | `*.service`, `systemctl enable`, installer scripts |
| Q3 | Does it run **containers or nested virtualization**? | `Dockerfile`, `docker compose`, Podman, k3s/kind, `virsh`/QEMU in tests |
| Q4 | Does it change **networking or firewall** state? (nftables/iptables, interfaces, routing, VPN, DNS servers) | `nft`, `iptables`, `ip link`, WireGuard, `netplan` |
| Q5 | Does it need a **different OS, distro family or kernel** than the host, or a specific deploy target? | Deploy docs, "targets Debian 12", Windows/BSD |
| Q6 | Is the code **untrusted, security-sensitive or potentially destructive**? (third-party code, malware analysis, exploit testing, `rm -rf`-style installers) | Source of the code, the task itself |
| Q7 | Will tests need **snapshot/revert** to a known state between runs? | Installers, migrations, system config tools |
| Q8 | Does the user want the **live console feed** (§7) for this work? | Default **yes** unless they've said otherwise |
| Q9 | Is the work **plain userspace** (compile, unit tests, linters, scripts, app servers on high ports) with **many short runs or parallel jobs** where boot time matters? | Language toolchains only, no system changes |
| Q10 | Is the **libvirt LXC driver available** and is a template rootfs set up? | `lab.sh doctor` (§3) |

### 2.2 Decide

Apply in order; the first rule that matches wins:

1. **Q10 is no → VM.** (LXC isn't possible on this host yet; don't set it up unasked.)
2. **Any of Q1–Q7 is yes → VM.** Each needs a separate kernel, full init, nested virt, isolation or
   snapshots, which libvirt LXC doesn't give safely or at all.
3. **Q8 is yes → VM**, unless Q9 is strongly yes and the user has accepted a text-only console for
   that role.
4. **Q9 is yes → LXC.** Fast startup, low overhead, host kernel acceptable.
5. **Otherwise → VM** (the safe default).

When the answer is **unclear** (e.g. the project might later add services or containers), choose a **VM**.
Moving from LXC to a VM mid-project costs more than starting in a VM.

### 2.3 Record and tell the user

- Write the result to `docs/LAB.md` → *Guest decision*: one row per role with the kind, the rule
  that decided it, and the Q-numbers that were "yes" (e.g. `main → vm · rule 2 · Q2, Q3`).
- Set `LAB_KIND` in `lab.env`. For mixed roles, keep a second config for the other kind (e.g.
  `lab.lxc.env` with `LAB_KIND=lxc`) and select it per call with `LAB_ENV=lab.lxc.env scripts/lab.sh …`;
  note in `docs/LAB.md` which file each role uses.
- Tell the user in one line, e.g. *"Lab: VM (project installs systemd services and runs Docker); watch
  it in virt-manager as lab-myproj-main."* If the choice was close or LXC was picked, say so, so they
  can overrule it.

### 2.4 Re-evaluate when the work changes

Re-run §2.1–2.3 when a new phase or task brings in something from Q1–Q7 (a Dockerfile appears, a
service gets added, kernel work starts), when the user asks, or when an LXC guest hits a limitation
(permission denied on mounts/sysctl, systemd failing to start, nested containers not working). If an
LXC role now needs a VM, create the VM role, move work to it, record the change in `docs/LAB.md` → *Log*,
and destroy the old container after confirming with the user. The user's explicit choice of guest type
always overrides the rules.

## 3. Discovery (once per machine, re-checked once per project)

Run `scripts/lab.sh doctor` (copy the script first). It checks and prints, without changing anything:

- `virsh`, `virt-install` (≥ 3.0, needed for `--cloud-init`), `qemu-img`, `rsync`, `ssh` present.
- `virt-manager` present (warns if missing; the lab still works, but live viewing doesn't).
- `/dev/kvm` exists and is usable (hardware acceleration on).
- `virsh -c qemu:///system` connects **without sudo** (user is in the `libvirt` group).
- The configured network is defined and active; the configured storage pool is active.
- The configured base volume exists in that pool.
- Whether the LXC driver works: `virsh -c lxc:/// version`.
- Host free RAM and CPU count, for sizing.

Then write `docs/LAB.md` (§9) with the results. If `doctor` reports a hard failure, **stop and tell the
user** what's missing and the one-line fix (e.g. `sudo usermod -aG libvirt $USER` then log out/in;
`sudo virsh net-start default`). Claude never runs `sudo` on the host itself, never installs host packages,
and never edits libvirt's global config, networks or pools without the user's explicit OK.

Use `qemu:///system` (not `qemu:///session`). It's the connection virt-manager opens by default as
**"QEMU/KVM"**, so guests show up there without extra setup. It also has the NAT network with DHCP
leases that `lab.sh` uses to find guest IPs.

## 4. Base images

Guests are thin **qcow2 overlays** on a shared, read-only base volume, so creating one takes seconds and
a few MB.

- Base volumes live in the configured pool and are named `lab-base-<distro><ver>.qcow2`
  (e.g. `lab-base-debian12.qcow2`). They are **never booted directly and never modified.**
- Use the distro's official **generic cloud image** (Debian `genericcloud`, Ubuntu `cloudimg`,
  Fedora `Cloud Base Generic`); they ship cloud-init, which `lab.sh` uses to inject the user, SSH key
  and console setup.
- Import a base (one-time, the user runs or approves this; download URL from the distro's site):
  ```bash
  qemu-img info ~/Downloads/debian-12-genericcloud-amd64.qcow2          # confirm format + virtual size
  virsh -c qemu:///system vol-create-as default lab-base-debian12.qcow2 2G --format qcow2
  virsh -c qemu:///system vol-upload --pool default lab-base-debian12.qcow2 ~/Downloads/debian-12-genericcloud-amd64.qcow2
  ```
- Match the base to the project's deploy target when there is one (deploys to Debian 12 → test on
  Debian 12). Otherwise default to the newest Debian stable.
- One base can back many projects. Don't create a new base per project.

## 5. `lab.env` (per project, repo root)

```bash
LAB_PROJECT=myproject          # [a-z0-9-]; guest names become lab-<project>-<role>
LAB_URI=qemu:///system
LAB_KIND=vm                    # vm | lxc
LAB_NET=default                # libvirt network (NAT). Never a host bridge unless the user asks.
LAB_POOL=default
LAB_BASE_VOL=lab-base-debian12.qcow2
LAB_OS_VARIANT=debian12        # osinfo-query os  → short id
LAB_VCPUS=2
LAB_RAM_MB=4096
LAB_DISK_GB=20
LAB_USER=lab
LAB_SSH_KEY="$HOME/.ssh/id_ed25519.pub"   # public key; private key = same path without .pub
LAB_DEST=/home/lab/src         # where the repo is synced inside the guest
LAB_HOST_RESERVE_MB=4096       # RAM always left free on the host
LAB_GRAPHICS=spice,listen=none # display virt-manager shows; local socket only. Use vnc,listen=none if SPICE is unavailable
LAB_AUTO_VIEW=1                # 1 = open the guest's console in virt-manager whenever lab.sh up runs
# LXC only (libvirt LXC driver): template rootfs dir copied per container
LAB_LXC_ROOTFS=
```

Commit `lab.env`. It contains no secrets.

## 6. Workflow

**Setup (once per project)**
1. Copy §10 to `scripts/lab.sh` verbatim and `chmod +x` it. Don't rewrite or "improve" it per project.
2. Write a minimal `lab.env` (§5) with `LAB_KIND=vm` and run `scripts/lab.sh doctor` to learn what the
   host supports (§3). Fix or report failures.
3. **Choose the guest type per §2** (VM or LXC, per role), record it in `docs/LAB.md`, tell the user,
   and finish `lab.env` to match. Size it to the job: 2 vCPU / 4 GB is the default; go heavier only when
   a build needs it. If LXC was chosen, re-run `doctor` to confirm the LXC checks pass.
4. `scripts/lab.sh up`: creates `lab-<project>-main`, opens its console in virt-manager, waits for
   SSH and cloud-init, takes snapshot `clean`.
5. Provision project dependencies **inside the guest**. Put the commands in `scripts/provision.sh` in
   the repo (idempotent) and run it with `scripts/lab.sh run 'bash scripts/provision.sh'`, so the guest
   can be rebuilt from scratch.
6. `scripts/lab.sh snap provisioned`.
7. Write `docs/LAB.md` (§9) and add one line to `CLAUDE.md`:
   `Build/test only via scripts/lab.sh (see docs/LAB.md). Guests are watchable in virt-manager.`
8. Tell the user which guest(s) to look for in virt-manager (§7).

**Every build/test cycle**
1. `scripts/lab.sh run '<build or test command>'` syncs the working tree to the guest (respecting
   `.gitignore`, excluding `.git`), runs the command in `LAB_DEST`, mirrors it to the live console, and
   returns its exit code.
2. Keep output small: pipe through `tail -n 50` or a summarizing script; don't stream full logs into
   context. (The user still sees full output on the guest console.)
3. Need artifacts back (binaries, reports, coverage)? `scripts/lab.sh pull <guest-path> <host-dir>`.
   Pull only into the repo's ignored output dirs (e.g. `dist/`, `out/`), never over source files.

**When a guest gets dirty or broken**
- `scripts/lab.sh revert provisioned` (or `clean`). Prefer revert over debugging a polluted guest.
- Tests that mutate system state (installers, migrations, service setup) **start from a revert** every
  time so results are reproducible.

**Multiple guests** (e.g. client/server, multi-distro matrix): `scripts/lab.sh up <role>` for each
role (`lab-<project>-server`, `lab-<project>-db`). Guests on the same libvirt network reach each other by
IP (`scripts/lab.sh ip <role>`). Record every role in `docs/LAB.md`.

**End of session:** `scripts/lab.sh down` (shut down, keep). **End of project or on request:**
`scripts/lab.sh destroy <role>` for each guest; confirm with the user first.

## 7. Live visibility in Virtual Machine Manager

The user must be able to open virt-manager at any time and see what Claude is doing.

**Where guests appear**
- **VMs:** under the default **QEMU/KVM** connection (`qemu:///system`), named `lab-<project>-<role>`,
  with title `lab <project> (<role>)` and description `lab:<project>`.
- **LXC containers:** under an **LXC** connection. virt-manager doesn't add one by default; the user
  adds it once via *File → Add Connection → Hypervisor: LXC (Linux Containers)*. Claude says this when
  it creates the first container.

**What the console shows (VMs)**
- Every lab VM gets a real display (`LAB_GRAPHICS`, SPICE by default) plus a serial console, so
  *Open* in virt-manager shows a screen, not "Guest has no graphical display".
- cloud-init configures `tty1` and `ttyS0` to auto-log in as `LAB_USER` and show a **live feed**
  (`tail -F /var/log/lab/run.log`). Every command Claude runs through `lab.sh exec`/`run` goes through
  the guest helper `lab-run`, which writes a timestamped `$ command` header, the full output and
  `[exit N]` to that log. The user sees each command and its output as it runs.
- **Ctrl-C** on the console drops to a normal shell in the guest for poking around; log out
  (`exit` / Ctrl-D) and the feed comes back.
- `scripts/lab.sh view [role]` opens that guest's console window. With `LAB_AUTO_VIEW=1` (default) `up`
  does this automatically, so the user can watch first boot and provisioning. It needs a graphical
  session on the host; if none is available it says so and carries on.
- **LXC:** the console in virt-manager is the container's text console. The live feed works only if the
  template rootfs includes `lab-run` and the same console setup; otherwise the user sees a login prompt.
  Prefer VMs when live viewing matters.

**Rules**
- Never create a lab guest with `--graphics none` or headless, and never remove its display or serial
  device.
- Never hide work from the feed: don't use raw `ssh` or `virsh` to run project commands in a guest. Use
  `lab.sh exec`/`run`, which log to the console.
- The display listens on a **local socket only** (`listen=none`), never on a network port, so the
  auto-logged-in console is reachable only by local users in the `libvirt` group. Don't change that
  without the user asking.
- If the user is watching and asks Claude to pause, stop issuing guest commands until they say go;
  they may be typing in the console.

## 8. Safety rules (always)

- **Only touch guests named `lab-*`.** `lab.sh` refuses anything else. Never run raw `virsh destroy`,
  `undefine`, `snapshot-revert` or `vol-delete` against other domains or volumes; the user's other VMs
  are off-limits.
- **Never delete or write to a `lab-base-*` volume.** Destroy removes only the guest's own overlay.
- **No host bridges, passthrough or shared host directories** unless the user asks. The default NAT
  network is the boundary. Code moves in and out by `rsync` over SSH only.
- **Respect host resources.** `lab.sh up` refuses to start if it would leave less than
  `LAB_HOST_RESERVE_MB` free. Never run more than 3 guests at once without asking. Shut guests down at
  the end of a session.
- **No secrets in guests** beyond what the test strictly needs. Never copy `~/.ssh` private keys,
  cloud credentials or browser profiles in, and remember the console log is visible: use throwaway test
  credentials and never echo secrets into commands.
- **Destructive actions ask first:** `destroy`, importing or replacing a base volume, changing
  networks/pools, anything needing `sudo` on the host.
- Snapshot names are `[a-z0-9-]`. Keep ≤ 5 per guest; delete stale ones
  (`virsh snapshot-delete <guest> <name>`) rather than accumulating.

## 9. `docs/LAB.md` template

```markdown
# Lab — <project>
_Standard: VM-LAB-STANDARD v1.2 · Created <YYYY-MM-DD>_

## Host (from lab.sh doctor)
- libvirt <ver>, virt-install <ver>, virt-manager: yes/no, KVM: yes, LXC driver: yes/no
- Network: default (NAT, 192.168.122.0/24) · Pool: default (/var/lib/libvirt/images)
- Host: <N> CPUs, <N> GB RAM

## Guest decision (§2)
| Role | Kind | Rule | "Yes" answers | Note |
|---|---|---|---|---|
| main | vm | 2 | Q2, Q3 | installs systemd units, runs Docker |

## Guests (watch in virt-manager → QEMU/KVM)
| Guest | Kind | Base | vCPU/RAM | Purpose | Snapshots |
|---|---|---|---|---|---|
| lab-<project>-main | vm | lab-base-debian12 | 2 / 4 GB | build + unit tests | clean, provisioned |

## Provisioning
- scripts/provision.sh: <one line on what it installs>

## Commands
- Build: `scripts/lab.sh run '<cmd>'`
- Test:  `scripts/lab.sh run '<cmd> 2>&1 | tail -n 50'`
- Watch: `scripts/lab.sh view`

## Host exceptions
- <none | what runs on host, why, approved by user on YYYY-MM-DD>

## Log
- <YYYY-MM-DD> — <guest created / rebuilt / quirk found and workaround>
```

## 10. `scripts/lab.sh` (copy verbatim)

```bash
#!/usr/bin/env bash
# lab.sh — VM Lab Standard v1.2. Manage libvirt lab guests for this project.
# Usage: scripts/lab.sh <doctor|up|view|ip|ssh|exec|push|run|pull|snap|revert|snaps|down|destroy|list> [args]
#   up [role]            create/start lab-<project>-<role> (default role: main)
#   view [role]          open the guest's console in Virtual Machine Manager
#   exec [-r role] CMD   run CMD in guest (in $LAB_DEST), mirrored to the live console
#   run  [-r role] CMD   push working tree, then exec CMD
#   push [role] / pull GUEST_PATH HOST_DIR [role]
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
  doctor|up|view|ip|ssh|exec|push|run|pull|snap|revert|snaps|down|destroy|list) "cmd_$c" "$@" ;;
  *) sed -n '2,12p' "$0"; exit 2 ;;
esac
```

Notes on the script:
- `destroy` deletes only `<guest>.qcow2` (the overlay `up` created) and that guest's snapshots. It
  never deletes the base volume or any volume it didn't create.
- `revert` uses libvirt internal qcow2 snapshots; they work because guests use BIOS boot and a single
  qcow2 disk. Don't add UEFI firmware or extra raw disks to lab guests, or snapshots will fail. After a
  revert the console feed shows the log as it was at snapshot time.
- `up` passes its own `meta-data`: virt-install 5.x omits an empty meta-data file from the seed ISO, and cloud-init then rejects the disk (`not a valid seed`).
- `lab-run` and the console feed are installed by cloud-init on first boot, so they live in the
  `clean` snapshot and survive reverts.
- LXC mode uses the libvirt LXC driver; creating the rootfs copy and `lxc-enter-namespace` need sudo,
  so Claude asks the user to run those steps, or uses VMs.

## 11. Anti-patterns (never)

- Running `npm install`, `pip install`, `make`, test suites or dev servers on the host "just this once".
- Picking VM or LXC by habit instead of running the §2 decision, or keeping an LXC guest after the
  work has grown into Q1–Q7 territory.
- Creating a headless lab guest, or running project commands in a guest by any route that bypasses the
  live console feed.
- Booting, modifying or deleting a `lab-base-*` volume.
- Touching any libvirt domain, network, pool or volume not created by `lab.sh`.
- Bridging a lab guest onto the LAN, exposing its display on a network port, passing through devices,
  or mounting host home directories into it without the user asking.
- Debugging a polluted guest for long instead of `revert`-ing it.
- Leaving guests running after the session, or starting more than the host can spare.
- Copying personal credentials or SSH private keys into a guest, or printing secrets to the console.
- Rewriting `lab.sh` per project; fixes go back into this standard with a version bump.
