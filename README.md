# reminder-rs

A native Windows Notification Center reminder tool, and a port of
[`reminder.py`](../reminder.py) in the parent directory.

## Motivation

`reminder.py` drives the Windows toast API by shelling out to PowerShell.
That works, but every notification pays a fixed process-startup cost:

| Implementation | Mechanism | Median latency (7 runs, warm cache) |
|---|---|---|
| `reminder.py` | Python → PowerShell subprocess → WinRT | **335 ms** |
| `reminder.exe` | Direct WinRT call | **28 ms** |

**11.8× faster, 307 ms saved per notification.**

The overhead matters less for one-off reminders than for tools like
`pipeline.py`, which fires a toast at the end of a CI run and then exits.
The PowerShell cold-start (≈150 ms) and Python cold-start (≈34 ms) are both
real costs there.

Beyond latency, calling WinRT directly removes two layers of indirection that
neither add features nor improve reliability. `reminder.py` also had to carry
a fairly elaborate workaround: because it borrows PowerShell's AppID, foreground
and background COM activation both dispatch to PowerShell rather than back to the
script. That's why `reminder.py` has a `reminder://` URI scheme, a token-keyed
action store, TTL pruning, and traversal guards — all infrastructure that exists
only to route a notification click back into the right process. A native binary
with its own AppID would remove all of that. `reminder.exe` doesn't implement the
COM activator yet (see [Limitations](#limitations)), but it's the right foundation.

## Building

Needs nothing but [rustup](https://rustup.rs) — no Visual Studio, no MSYS2,
no MinGW:

```bash
rustup toolchain install stable-x86_64-pc-windows-gnu
cargo build --release
```

The binary lands at `target/release/reminder.exe`, ~300 KB, with the `windows`
crate as its only dependency.

> **Note — do not bump `windows` past 0.58 without reading `Cargo.toml`.**
> Newer versions link via `raw-dylib`, which on the GNU target makes rustc call
> `dlltool.exe` to build import libraries. `dlltool` in turn needs an assembler
> (`as.exe`) that rustup's self-contained bundle does not ship, so the build
> dies with `CreateProcess` from inside dlltool. The `0.58` pin resolves to
> `windows-targets 0.52.6`, which ships prebuilt `.a` import libraries and
> bypasses dlltool entirely.

## Usage

```
reminder <message> [options]

Options:
    --title <text>        Notification title (default: Reminder)
    -l, --level <name>    Severity: info, success, warning, error (default: info)
    --icon <path>         Custom image instead of the level icon
    --url <url>           Clicking the notification body opens this URL
    --button <LABEL=URL>  Add a clickable button (repeatable, max 5)
    --silent              Suppress the notification sound
    --in <seconds>        Wait before the first reminder
    --repeat <n>          How many reminders to send (default: 1)
    --every <seconds>     Interval between repeats (default: 60)
    --app-id <aumid>      Override the AppUserModelID
    --print-xml           Print the toast XML instead of sending it
```

```bash
reminder "Time for a break"
reminder "Build broke" --level error
reminder "Meeting starting" --in 300
reminder "Stand up" --repeat 4 --every 1800
reminder "Review PR" --url https://github.com/notifications
reminder "Pipeline failed" \
    --level error \
    --button "Open logs=https://devops.kingdee.com" \
    --button "Runbook=https://example.com/runbook"
```

## Layout

| File | What's in it |
|---|---|
| `src/main.rs` | CLI parsing and the send loop |
| `src/toast.rs` | Toast XML builder and the WinRT call that shows it |
| `src/level.rs` | The four severity levels: colour, glyph, sound |
| `src/icon.rs` | Draws and caches the level icons |
| `src/png.rs` | Minimal PNG encoder (CRC-32, Adler-32, stored DEFLATE) |

No image crate. No arg-parser crate. `windows` is the only dependency, which
is why the toolchain requirement stays minimal.

## Design notes

**Severity levels.** Windows toasts have no native severity concept —
`NIIF_INFO`/`NIIF_ERROR` from old balloon tips are gone. The only
severity-adjacent attribute, `scenario="urgent"`, is Windows 11 only
and an unrecognised value risks the payload being silently dropped. So each
level is expressed via a procedurally-drawn icon plus a system sound:

| Level | Colour | Sound |
|---|---|---|
| info | blue | `Notification.Default` |
| success | green | `Notification.Default` |
| warning | amber | `Notification.Reminder` |
| error | red | `Notification.Looping.Alarm2` |

**Icons.** A toast needs an image file — it cannot reference an icon inside a
DLL. Rather than ship binary assets, the four icons are drawn at runtime (96×96
RGBA, anti-aliased disc + glyph) and cached at
`%LOCALAPPDATA%\reminder-rs\icons`. First draw is a few dozen milliseconds;
subsequent runs hit the cache in under a millisecond.

**AppID.** Toasts are shown under PowerShell's AppID, which Windows already
trusts. That avoids needing a Start Menu shortcut or an `AppUserModelId`
registry entry. The cost is that Action Center attributes toasts to PowerShell.
`--app-id` overrides this.

**XML element order.** The toast schema requires `visual`, then `audio`, then
`actions`. An out-of-order payload is silently dropped by Windows.

**Escaping.** Both `&` and `'` must be XML-escaped: attributes are
single-quoted in the payload, so a button label like `It's fine` produces
malformed XML if `'` is left bare.

## Limitations

- **Click callbacks.** `--url` and `--button` open a URL via protocol
  activation, but cannot run a local command. `reminder.py` handles this with a
  registered `reminder://` URI scheme and a token store. The native equivalent
  is an `INotificationActivationCallback` COM activator. `windows-implement
  0.58` is already a transitive dependency, so this is a natural next step.
- **AppID.** Using PowerShell's AppID means Action Center shows "Windows
  PowerShell" as the sender. Registering a proper AppID requires either a Start
  Menu shortcut or an `AppUserModelId` registry entry pointing at the exe.
