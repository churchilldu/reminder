# reminder

A native Windows Notification Center reminder tool.

## Motivation

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

```text
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
    --button "Open logs=https://github.com/" \
    --button "Runbook=https://example.com/runbook"
```

## Layout

| File           | What's in it                                              |
| -------------- | --------------------------------------------------------- |
| `src/main.rs`  | CLI parsing and the send loop                             |
| `src/appid.rs` | Own AppUserModelId registration via a Start Menu shortcut |
| `src/toast.rs` | Toast XML builder and the WinRT call that shows it        |
| `src/level.rs` | The four severity levels: colour, glyph, sound            |
| `src/icon.rs`  | Draws and caches the level icons                          |
| `src/png.rs`   | Minimal PNG encoder (CRC-32, Adler-32, stored DEFLATE)    |

No image crate. No arg-parser crate. `windows` is the only dependency, which
is why the toolchain requirement stays minimal.

## Design notes

**Severity levels.** Windows toasts have no native severity concept —
`NIIF_INFO`/`NIIF_ERROR` from old balloon tips are gone. The only
severity-adjacent attribute, `scenario="urgent"`, is Windows 11 only
and an unrecognised value risks the payload being silently dropped. So each
level is expressed via a procedurally-drawn icon plus a system sound:

| Level   | Colour | Sound                         |
| ------- | ------ | ----------------------------- |
| info    | blue   | `Notification.Default`        |
| success | green  | `Notification.Default`        |
| warning | amber  | `Notification.Reminder`       |
| error   | red    | `Notification.Looping.Alarm2` |

**Icons.** A toast needs an image file — it cannot reference an icon inside a
DLL. Rather than ship binary assets, the four icons are drawn at runtime (96×96
RGBA, anti-aliased disc + glyph) and cached at
`%LOCALAPPDATA%\reminder\icons`. First draw is a few dozen milliseconds;
subsequent runs hit the cache in under a millisecond.

**AppID.** Toasts are shown under our own AppUserModelId (`Reminder.App`), not
PowerShell's. On first use the app registers that identity by creating a Start
Menu shortcut at `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Reminder.lnk`
stamped with the `System.AppUserModel.ID` property -- the documented requirement
for an unpackaged app to own a notification identity. The result: Action Center
shows **"Reminder"** as the sender instead of "Windows PowerShell". See
`appid.rs`. `--app-id` overrides the identity. Manage the registration explicitly
with `--register` / `--unregister`.

**XML element order.** The toast schema requires `visual`, then `audio`, then
`actions`. An out-of-order payload is silently dropped by Windows.

**Escaping.** Both `&` and `'` must be XML-escaped: attributes are
single-quoted in the payload, so a button label like `It's fine` produces
malformed XML if `'` is left bare.

## Limitations

- **Click callbacks.** `--url` and `--button` open a URL via protocol
  activation, but cannot run a local command.
- **AppID registration.** The identity shortcut is created once, on first use.
  If the exe is subsequently moved, the shortcut still points at the old
  location; delete it (or run `reminder --unregister` then `--register`) to
  re-register at the new path. Registration writes one `Reminder.lnk` file into
  the current user's Start Menu.

## License

MIT. See [LICENSE](LICENSE).
