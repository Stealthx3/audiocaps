<!-- SPDX-License-Identifier: LGPL-2.1-or-later -->
# Packaging AudioCaps in Flatpak and Snap

AudioCaps runs entirely inside your app's process. It needs no host service, no daemon and no
helper outside the sandbox. What it can see depends on the permissions your package grants. A
missing permission never makes it fail. The layers it hides are reported as `unknown` with the
reason `not_granted`, and the `SANDBOX_PERMISSION_MISSING` finding names the permission and the
exact fix (`flatpak override …` or `snap connect …`).

- Flatpak: [`flatpak/finish-args.yml`](flatpak/finish-args.yml) (copy into your manifest) and a
  test app manifest, [`flatpak/io.github.stealthx3.AudioCapsProbe.yml`](flatpak/io.github.stealthx3.AudioCapsProbe.yml).
- Snap: [`snap/snapcraft.yaml`](snap/snapcraft.yaml) (plugs, plus the runtime data a core24
  snap must stage).
- Build the library for both with `scripts/build-portable.sh`. It builds in an Ubuntu 24.04
  container, so it needs glibc 2.39 and runs on the freedesktop 24.08 runtime and on core24.

## What each permission unlocks

### Flatpak

| Permission | Unlocks | Without it |
| --- | --- | --- |
| `--socket=pulseaudio` (required) | PulseAudio API: sinks, ports, formats, the `pulse_audio` path. On PipeWire systems, pipewire-pulse. | `sound_server` is `not_granted` with the fix. ALSA and sysfs reads still work. |
| `--filesystem=xdg-run/pipewire-0:ro` | Native PipeWire: nodes, devices, profiles, routes, negotiated formats, the `pipe_wire` path. Also JACK through pipewire-jack. | `pipe_wire` is `not_granted`. |
| `--system-talk-name=org.bluez` | Bluetooth codecs and transports. | `wireless` is `not_granted`. |
| `--device=all` | Direct ALSA (opening `hw:` for ranges) and the CEC opt-in (`/dev/cec*`). | CEC reports no adapter. ALSA ranges come from the sound server. |
| *(none needed)* | `/proc/asound` (cards, PCMs, codec pages, ELD) and `/sys/class/drm/*/edid`. Realtime through the portal. | n/a |

The kernel driver layer (`/sys/module`: driver parameters such as HDA power saving) isn't
mounted in Flatpak. No permission exposes it, so it is reported `not_exposed`.

### Snap (strict confinement)

| Plug | Auto-connects | Unlocks | Without it |
| --- | --- | --- | --- |
| `audio-playback` (required) | yes | PulseAudio API, the `pulse_audio` path. | `sound_server` is `not_granted`. |
| `pipewire` | no | Native PipeWire, the `pipe_wire` path. AppArmor refuses the `pipewire-0` socket without it. | `pipe_wire` is `not_granted`, with the plug named. |
| `alsa` | no | `/proc/asound` and `/dev/snd`: the `alsa_direct` path, codec pages, ELD. | `alsa_userspace` is `not_granted`. |
| `hardware-observe` | no | sysfs device attributes, notably the DRM connectors' EDID (sink identity, formats). `/sys/class/sound` lists without it. | EDID unreadable; the plug is listed missing. |
| `kernel-module-observe` | no | `/sys/module` driver parameters (e.g. HDA power saving). | `kernel_driver` is `not_granted`. |
| `bluez` | no | Bluetooth codecs and transports. | `wireless` is `not_granted`. |
| `audio-record` | no | Capture endpoints through the server. | n/a |
| `desktop` | yes | Session bus: the Realtime portal. | Realtime only through rtkit, if a plug grants it. |

No snap interface grants `/dev/cec*`. In a strict snap the CEC opt-in reports the adapter as
`not_granted`, after one quick failed open and no wait.

A core24 snap also has to stage what the base lacks, as the template does:

- PipeWire's client modules, SPA support plugins and `client.conf`;
- alsa-lib's configuration;
- environment variables pointing the staged libraries at them: `PIPEWIRE_MODULE_DIR`,
  `SPA_PLUGIN_DIR`, `PIPEWIRE_CONFIG_DIR`, `ALSA_CONFIG_DIR`, `ALSA_CONFIG_PATH`.

Inside a snap, `XDG_RUNTIME_DIR` is the snap's private directory. AudioCaps finds the sound
sockets one level up, where the audio plugs grant them, so the app needs no `PULSE_SERVER` or
`PIPEWIRE_REMOTE` setup.

## Sandbox release gate (phase 9.4)

The gate is `scripts/gate-sandbox.sh`. It ran on 2026-11-13 in two lab guests:

- `lab-audiocaps-capture`: Debian 13, PipeWire 1.4.2 with WirePlumber, Flatpak 1.16.6 and the
  freedesktop 24.08 runtime.
- `lab-audiocaps-snap`: Ubuntu 24.04, kernel 6.8 with full AppArmor mediation, snapd 2.77.1
  reporting `confinement: strict`, PipeWire 1.0.5.

Both guests have emulated HDA cards and a vivid CEC pair: a TV, and an audio system answering
queries. Every run completed (exit 0) in 75–650 ms, apart from the opt-in CEC runs, which wait on
the bus.

| Run | Missing (as reported) | Servers read | Endpoints / paths | Realtime |
| --- | --- | --- | --- | --- |
| Flatpak, no audio permission | `--socket=pulseaudio`, `--filesystem=xdg-run/pipewire-0`, `--system-talk-name=org.bluez` | none (`not_granted`) | 10 / `alsa_direct` | portal |
| Flatpak, `--socket=pulseaudio` | pipewire-0, bluez | PulseAudio | 10 / `alsa_direct`, `pulse_audio` | portal |
| Flatpak, + `pipewire-0` | bluez | PipeWire, PulseAudio, JACK (pipewire-jack) | 10 / all three | portal |
| Flatpak, + bluez + `--device=all` | none | all | 10 / all three; CEC read | portal |
| Snap, auto-connected plugs | `alsa`, `hardware-observe`, `kernel-module-observe`, `pipewire` | PulseAudio | 2 / `alsa_direct`, `pulse_audio` | portal (rtkit refused) |
| Snap, every plug | none | PipeWire, PulseAudio | 7 / all three, same as the host | portal and rtkit |

What the plan's open items turned out to be:

- **`/proc/asound` and DRM EDID with only `--socket=pulseaudio`: readable.** Cards, PCMs, HDA
  codec pages and `/sys/class/drm/*/edid` all read inside the sandbox. Only `/sys/module` is
  absent.
- **What WirePlumber lets a sandboxed native PipeWire client see: everything.** WirePlumber
  tags the client `pipewire.access = flatpak` with its app id, and grants it `rwxm` on every
  object. The sandboxed probe saw the same nodes, devices and formats as the host. Flatpak leaves
  an empty placeholder file at `pipewire-0` when the socket isn't granted. AudioCaps checks for a
  real socket, so that case reads `not_granted`, not a failed connection.
- **Realtime in a strict snap:** with auto-connected plugs, AppArmor refuses rtkit on the
  system bus (`not_granted`), but the Realtime portal (through `desktop`) grants priority 20, so
  realtime is available. In Flatpak the bus proxy hides rtkit and the portal is the way in. The
  library never starts rtkit or the portal; an activatable but stopped portal reads
  `service_not_running`.
- **What a strict snap reads without manual plugs:** PulseAudio (endpoints and formats), and
  `/sys/class/sound`. `/proc/asound`, `/sys/class/drm/*/edid` and `/sys/module` are denied by
  AppArmor until `alsa`, `hardware-observe` and `kernel-module-observe` are connected.
- **CEC in a strict snap:** the `/dev/cec*` nodes are visible but the open fails with EPERM. No
  interface grants them. `cec-dump` shows each adapter skipped (75 ms in total), and the probe
  reports that as `not_granted` on the HDMI endpoint. In Flatpak with `--device=all`, CEC reads the bus fully, the same as the host.

The shared library also ran from Python (ctypes) inside the Flatpak, against the live system.
