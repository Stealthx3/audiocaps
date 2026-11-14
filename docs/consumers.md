<!-- SPDX-License-Identifier: LGPL-2.1-or-later -->
# Using AudioCaps from your application

AudioCaps ships as one shared library, `libaudiocaps.so.1`, and one C header,
[`include/audiocaps.h`](../include/audiocaps.h). Every language uses the same C API; there are no
per-language binding packages. Bundle the library with your app (in a Flatpak, Snap or AppImage, or
next to your binary) and follow this page.

Working examples, checked by `scripts/gate-examples.sh` to print identical output on the same
fixture:

| Language | Example | How it loads the library |
| --- | --- | --- |
| C | [`examples/c/probe.c`](../examples/c/probe.c) | links `-laudiocaps` |
| C++ | [`examples/cpp/probe.cpp`](../examples/cpp/probe.cpp) | links `-laudiocaps`, RAII via `std::unique_ptr` |
| Python | [`examples/python/probe.py`](../examples/python/probe.py) | `ctypes.CDLL("libaudiocaps.so.1")` |
| .NET / C# | [`examples/dotnet/Probe.cs`](../examples/dotnet/Probe.cs) | `[DllImport("audiocaps")]` |

## Building the library

```bash
cargo build --release -p audiocaps-ffi
```

This produces `target/release/libaudiocaps.so` with the soname `libaudiocaps.so.1`. Install or bundle
it as `libaudiocaps.so.1`. The soname changes only when the ABI breaks, and `ac_abi_version()`
reports the same number as `AC_ABI_VERSION` in the header. The library is LGPL-2.1-or-later:
linking it dynamically and shipping it unmodified (or with your changes published) is fine for
closed-source apps.

## The model in five calls

```c
ac_context  *ctx  = ac_context_new();                        /* the running system */
ac_snapshot *snap = ac_probe(ctx, scope_json, options_json); /* one read, immutable result */
int ok = ac_can_play(snap, "alsa:card1/pcm9p", "pipe_wire",
                     "{\"codec\":\"eac3\"}", &reason);       /* AC_YES / AC_NO / AC_UNKNOWN */
char *json = ac_snapshot_json(snap);                          /* everything, as schema JSON */
ac_snapshot_free(snap); ac_context_free(ctx);
```

- **Snapshots are immutable.** Queries on a snapshot take no locks and do no I/O, from any thread.
  `ac_snapshot_clone` gives a second handle to the same data.
- **Contexts** hold the probe thread and the generation counter. Each probe returns a snapshot with
  a higher `generation`. Free a context when you're done. Snapshots outlive it.
- **Ownership.** `char *` results are yours: free them with `ac_string_free`. `const char *` results
  are borrowed from the handle they came from (endpoint ids live as long as the snapshot).
- **Errors.** Failing calls return `NULL` or `AC_ERROR`. `ac_last_error()` holds the message for
  the calling thread.
- **Fixtures.** `ac_context_new_fixture(dir)` replays a capture (`audiocaps capture` then
  `audiocaps import`), so you can test your app's logic against a user's system without their
  hardware.

## Scope: ask only for what you need

The library reads only what the scope asks for (see the
[principles](../CLAUDE.md#guiding-principles-apply-to-every-api-and-implementation-choice)). A
`NULL` scope means everything. Otherwise pass a JSON object in which each field is `"all"` or a
list. A missing field means `"all"`.

```json
{
  "endpoint_classes": ["hardware", "bluetooth"],
  "directions": ["playback"],
  "endpoints": "all",
  "layers": "all",
  "fields": ["effective", "findings"],
  "access_paths": ["pipe_wire"]
}
```

The values come from the [snapshot schema](../schema/audiocaps-snapshot-v1.schema.json):

- **endpoint classes:** `hardware`, `bluetooth`, `network`, `virtual`.
- **directions:** `playback`, `capture`, `midi`.
- **access paths:** `alsa_direct`, `pulse_audio`, `pipe_wire`, `jack`.
- **layers** and **fields:** the `LayerKind` and `FieldGroup` enums in the schema.

`ac_can_play` needs `effective` in `fields`.

## Options

`NULL` options means the defaults: passive, 150 ms per-backend deadlines, redaction on, no opt-ins.
Every field below is optional, and unknown fields are rejected.

| Field | Meaning |
| --- | --- |
| `deadline_ms` | Per-backend deadline. `0` waits for every backend. A backend that misses its deadline reports `unknown` with the reason `timeout`. |
| `app` | What you will play: `{"endpoint": "...", "codec": "eac3", "channels": 8, "rate_hz": 48000, "bits": 24, "latency_target_us": 20000}`. App-specific findings use it. |
| `cec` | Opt-in. Query the TV and receiver over HDMI-CEC (`/dev/cec*`, if your sandbox exposes it). |
| `software_codecs` | Opt-in. List the codec libraries your app could load (FFmpeg, GStreamer, BlueZ codec plugins). Libraries are listed, never loaded. |
| `active_probe` | Opt-in. `{"endpoint": "alsa:card1/pcm9p", "codecs": ["lpcm", "ac3"], "channels": 2, "rate_hz": 48000}` plays 100 ms of silence on one idle ALSA device. It is refused while a sound server manages the device or the device is in use. |
| `quirk_dirs`, `rule_dirs` | Your app's own quirk and rule packs (see [rules.md](rules.md)). |
| `redact` | `false` turns redaction off: serial numbers, Bluetooth addresses and personal device names are returned in the clear. Leave it on for anything you log or upload. |
| `redaction` | A custom redaction policy object (schema `RedactionPolicy`). |

## Watching for changes

```c
ac_watch *w = ac_watch_new(ctx, scope_json, options_json, mode, on_update, user);
```

The first delivery is the initial snapshot. After that, every kernel hotplug (`/dev/snd`) and
every PipeWire or PulseAudio change in the watched scope delivers a new snapshot and a diff. A
Bluetooth device shows up when the sound server creates its node; BlueZ itself isn't watched.
Each update carries `added`, `removed` and
`changed` endpoint ids, and the old and new generation. Only the watched scope is re-probed.

- `AC_WATCH_THREAD`: the callback runs on the library's event thread. Post the result to your UI
  thread yourself. `ac_watch_free` waits for a running callback to return, so don't call it from
  inside the callback.
- `AC_WATCH_DISPATCH`: the library writes to a pipe. Add `ac_watch_fd(w)` to your event loop
  (`poll`, `GSource`/`g_unix_fd_add`, `QSocketNotifier`, `asyncio.loop.add_reader`) and call
  `ac_watch_dispatch(w)` when it's readable. Callbacks then run on your thread.

The snapshot passed to the callback is borrowed. Keep it with `ac_snapshot_clone`. In dispatch
mode at most 32 undelivered updates are kept; if you fall further behind, the oldest are dropped
(compare the diff's generations). `ac_watch_free` waits for an in-flight re-probe, which is
bounded by the deadlines, so free a watch with `"deadline_ms": 0` off your UI thread. A `mode`
other than the two constants is an error.

## Language notes

### C and C++

Include `audiocaps.h` (it has `extern "C"` guards) and link `-laudiocaps`. In C++, wrap handles in
`std::unique_ptr` with a deleter that calls the matching `_free`, as the example does. Parse the
JSON with whatever you already use (nlohmann/json, simdjson, Qt's `QJsonDocument`, GLib's
json-glib).

### Python (ctypes)

Declare `restype` and `argtypes` for every function you call. Declare owned strings (`char *`
results and the `reason` out-parameter) as `c_void_p` rather than `c_char_p`. That way you keep the
pointer, copy it with `ctypes.string_at`, and free it with `ac_string_free`. A `c_char_p` restype
copies the string and loses the pointer, which leaks. For callbacks, keep the `CFUNCTYPE` object
referenced for as long as the watch runs. In dispatch mode, `asyncio` integration is
`loop.add_reader(ac_watch_fd(w), lambda: lib.ac_watch_dispatch(w))`.

### .NET (P/Invoke)

`[DllImport("audiocaps")]` resolves `libaudiocaps.so` on Linux. Bundle the library in your app's
`runtimes/linux-x64/native/` directory, or set `NativeLibrary.SetDllImportResolver` to load
`libaudiocaps.so.1`. Pass strings as UTF-8 NUL-terminated `byte[]` (as the example does), or use
`[LibraryImport]` with `StringMarshalling.Utf8` on .NET 7 and later. Read owned strings and pass
them back to `ac_string_free`; don't use `Marshal.FreeHGlobal`. Keep the callback delegate
referenced for as long as the watch runs (`GC.KeepAlive`). In thread mode the callback runs on a
non-.NET thread, so marshal to your UI context. The example builds with `dotnet build`
(`Probe.csproj`, .NET 8) and with Mono's `mcs`. The lab gate runs it on Mono.

## mpv mapping

mpv's audio options map directly onto a snapshot. Read the endpoint you're going to use, pick the
access path that matches mpv's `--ao`, and translate:

| mpv option | From the snapshot |
| --- | --- |
| `--ao=pipewire` / `pulse` / `alsa` / `jack` | An access path with `availability.state` of `read` or `partial` in `effective[]`: `pipe_wire`, `pulse_audio`, `alsa_direct`, `jack`. Prefer the sound server the system runs (`servers[]`). |
| `--audio-device=` | The endpoint's `correlation[]`. `pipewire_node.name` gives `pipewire/<name>`, `pulse_device.name` gives `pulse/<name>`, and `alsa_pcm` card/device gives `alsa/hw:<card>,<device>` for PCM. For passthrough over ALSA, `hdmi:CARD=<id>,DEV=<n>` takes the HDMI *ordinal* (the n-th HDMI PCM of the card; on HDA, `hw:1,3` is `DEV=0`), not the PCM device number. |
| `--audio-spdif=` | `effective[path].caps.passthrough` (codec names map as `ac3`, `eac3`, `dts`, `dts_hd` to `dts-hd`, `true_hd` to `truehd`). Include `dts-hd` and `truehd` only when the codec is listed, and check that `flags` doesn't contain `needs_hbr` unless the path also lists 8 channels at 192 kHz. Leave the option unset when passthrough is `unknown`, and show the `reason`. |
| `--audio-channels=` | `effective[path].caps.channels.max` together with `channel_maps`, for example `5.1` when the path carries 6 channels with an FL FR FC LFE RL RR (or SL SR) map. Use `auto-safe` when it's `unknown`. |
| `--audio-samplerate=` | One of `effective[path].caps.rates`. Leave it unset unless the source rate isn't carried. |
| `--audio-format=` | `effective[path].caps.sample_formats` (`s16_le` → `s16`, `s32_le` → `s32`, `f32_le` → `float`). |
| `--audio-exclusive=yes` | Only on `alsa_direct` when `direct_access` is known `true`. Under a sound server it does nothing useful. |

Before enabling passthrough, ask the library rather than re-deriving:

```c
if (ac_can_play(snap, id, "pipe_wire", "{\"codec\":\"true_hd\"}", &why) == AC_YES)
    /* add truehd to --audio-spdif */;
else
    /* show `why`: it names the layer that drops it, e.g. "limited by sink" */;
```

Findings (`ac_snapshot_findings_json`) explain the gaps in user-facing terms, with a fix hint for
the user's distribution. For example: the receiver supports TrueHD but the TV doesn't forward it
over ARC; the Flatpak lacks `--socket=pulseaudio`; the Bluetooth codec plugin isn't installed.

## Sandboxes

The library runs entirely in your process. It has no daemon and no host service. What it can see
depends on the permissions your package grants. See [packaging/](../packaging/) for Flatpak and
Snap templates and what each permission unlocks. A layer the sandbox hides is reported as `unknown`
with the reason `not_granted`, never as "unsupported", and a finding names the missing permission.
