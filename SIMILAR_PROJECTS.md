# Similar Projects

Research on existing tools/libraries that overlap with `reminder` (a native
Windows Notification Center reminder CLI). Compiled via web search.

## Direct CLI competition (most similar)

| Project               | Language | GitHub URL                                              | Notes                                                                                                 |
|---                    |---       |---                                                      |---                                                                                                    |
| shanselman/toasty     | C#       | https://github.com/shanselman/toasty                    | Tiny Windows toast CLI (~229 KB). Closest in spirit to `reminder`.                                 |
| App-vNext/Notifier    | Go       | https://github.com/App-vNext/Notifier                   | CLI that sends Windows toast notifications.                                                           |
| Yajusta/send-toast    | Python   | https://github.com/Yajusta/send-toast                   | Small CLI — native toast with icon/hero image/sound/urgency. "No daemon, no open port, no lock file." |
| hoodie/toastify       | Rust     | https://github.com/hoodie/toastify                      | CLI built on notify-rust; exposes most of that lib's functionality.                                   |
| stuartleeks/toast.exe | —        | https://github.com/stuartleeks/github-cli-notifications | WSL-focused toast sender (part of github-cli-notifications tooling).                                  |

## Rust libraries (build blocks / prior art)

| Project                      | GitHub URL                                      | Notes                                                                                                       |
|---                           |---                                              |---                                                                                                          |
| saez-juan/wpush.rs           | https://github.com/saez-juan/wpush.rs           | Rust wrapper over go-toast/toast; easiest console-visible toasts; supports WSL. Often cited as inspiration. |
| iKineticate/win-toast-notify | https://github.com/iKineticate/win-toast-notify | Rust lib for Windows toast; inspired by wpush.rs; still "unstable".                                         |
| elkablo/winrt-toast          | https://docs.rs/winrt-toast                     | Low-level `winrt_toast` crate.                                                                              |
| hoodie/notify-rust           | https://github.com/hoodie/notify-rust           | Cross-platform (Linux/macOS/Win via winrt-notification); most widely-used Rust notif lib.                   |
| kojiishi/toast-logger-win    | https://github.com/kojiishi/toast-logger-win    | Rust `log` crate logger → toast output.                                                                     |
| Microsoft windows-docs-rs    | https://github.com/microsoft/windows-rs         | The `windows::UI::Notifications` bindings used via `windows 0.58`.                                          |

## PowerShell / script approach

| Project                              | GitHub URL                                              | Notes                                                                                        |
|---                                   |---                                                      |---                                                                                           |
| Windos/BurntToast                    | https://github.com/Windos/BurntToast                    | The popular PowerShell module that `reminder.py`'s subprocess baseline effectively emulates. |
| github30/toast-notification-examples | https://github.com/GitHub30/toast-notification-examples | PowerShell WinRT examples.                                                                   |

## Notable differentiators of `reminder`

The biggest contrast: most of these are either libraries or rely on a
neighboring runtime/crate. `reminder` distinguishes itself by:

1. Zero third-party deps beyond `windows` (no image, no arg-parser, no notify lib).
2. Runtime-drawn + cached severity icons (other CLIs ship binary assets or accept an image path).
3. Procedural severity levels (color/sound/glyph) since WinRT has no native severity concept.
4. Extremely small, fast binary — ~300 KB, 28 ms latency, buildable with plain rustup/GNU target.

The closest "competitor" for the niche is probably Yajusta/send-toast
(native, no daemon/port, icon+sound+urgency) and Shanselman/toasty (tiny
standalone CLI). None found combine native-GNU-rustup-only builds,
runtime-drawn icons, and severity levels the way `reminder` does.
