//! Send reminders to the Windows Notification Center.
//!
//! A native port of reminder.py. That version drives the toast API through a
//! PowerShell subprocess; this one calls WinRT directly, so there is no
//! interpreter and no child process in the path.
//!
//! Only dependency is the `windows` crate. The PNG encoder for the level icons
//! is in png.rs precisely so that stays true.

mod appid;
mod callback;
mod ico;
mod icon;
mod level;
mod png;
mod toast;

use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use windows::Win32::System::Com::CoIncrementMTAUsage;

use toast::Toast;

/// Default AppUserModelId: our own registered identity, so Action Center
/// shows "Reminder" instead of "Windows PowerShell". Registered by the
/// Start Menu shortcut in appid.rs. Override with --app-id for a custom one.
const DEFAULT_APP_ID: &str = appid::AUMID;

const USAGE: &str = "\
Send a reminder to the Windows Notification Center.

Usage:
    reminder <message> [options]
    reminder --register
    reminder --unregister
    reminder --register-protocol
    reminder --unregister-protocol
    reminder --protocol-status
    reminder --list-actions
    reminder --open-uri <URI>

Options:
    --title <text>        Notification title (default: Reminder)
    -l, --level <name>    Severity: info, success, warning, error (default: info)
                          Sets the icon and the sound.
    --icon <path|app>     Custom image instead of the level icon; 'app' uses
                          the built-in app icon
    --url <url>           Clicking the notification body opens this URL
    --on-click <cmd...>   Run a command when clicked (all args after this flag)
                          Requires --register-protocol to have been run once.
    --button <LABEL=URL>  Add a clickable button (repeatable, max 5)
    --silent              Suppress the notification sound
    --tag <text>          Notification tag; same tag replaces a previous toast
                          from this app (default: unique per toast)
    --in <seconds>        Wait this long before the first reminder
    --repeat <n>          How many reminders to send (default: 1)
    --every <seconds>     Interval between repeats (default: 60)
    --app-id <aumid>      Override the AppUserModelID used to show the toast
    --print-xml           Print the payload instead of sending it
    -h, --help            Show this help
    -V, --version         Show the version

App identity:
    Reminders show in Action Center under your own identity (\"Reminder\"),
    registered via a Start Menu shortcut on first use -- no PowerShell sender.
    --register / --unregister manage that registration explicitly.

Click callbacks:
    --on-click stores the command locally and puts an unguessable token in the
    toast's launch URI (reminder://run/<token>). Clicking invokes the registered
    handler, which looks up the token and runs the stored command directly.

    The command is NEVER placed in the URI. The worst a hostile web page can do
    is re-fire an action one of your own scripts already registered.

    One-time setup:
        reminder --register-protocol

Examples:
    reminder \"Time for a break\"
    reminder \"Meeting starting\" --in 300
    reminder \"Stand up\" --repeat 4 --every 1800
    reminder \"Build broke\" --level error
    reminder \"Review PR\" --url https://github.com/notifications
    reminder \"Pipeline failed\" --button \"Logs=https://github.com/\"
    reminder \"Tests failed\" --on-click py rerun_tests.py
";

struct Args {
    message: String,
    title: String,
    level: &'static level::Level,
    icon: Option<PathBuf>,
    app_icon: bool,
    url: Option<String>,
    on_click: Option<Vec<String>>,
    buttons: Vec<(String, String)>,
    silent: bool,
    delay: u64,
    repeat: u32,
    every: u64,
    app_id: String,
    print_xml: bool,
    tag: Option<String>,
}

/// What to do after parsing: run, or handle a subcommand.
enum Parsed {
    Run(Args),
    Help,
    Version,
    Register,
    Unregister,
    RegisterProtocol,
    UnregisterProtocol,
    ProtocolStatus,
    ListActions,
    OpenUri(String),
}

/// A usage error, reported like a CLI should and exiting 2.
struct UsageError(String);

fn parse_args(argv: Vec<String>) -> Result<Parsed, UsageError> {
    let mut message: Option<String> = None;
    let mut title = String::from("Reminder");
    let mut level_name = String::from("info");
    let mut icon: Option<String> = None;
    let mut app_icon = false;
    let mut url: Option<String> = None;
    let mut on_click: Option<Vec<String>> = None;
    let mut button_specs: Vec<String> = Vec::new();
    let mut silent = false;
    let mut delay: u64 = 0;
    let mut repeat: u32 = 1;
    let mut every: u64 = 60;
    let mut app_id = String::from(DEFAULT_APP_ID);
    let mut print_xml = false;
    let mut tag: Option<String> = None;

    let mut it = argv.into_iter();

    // Pull the value that follows a flag, or complain that it is missing.
    fn value(it: &mut std::vec::IntoIter<String>, flag: &str) -> Result<String, UsageError> {
        it.next()
            .ok_or_else(|| UsageError(format!("{flag} needs a value")))
    }

    fn number<T: std::str::FromStr>(raw: &str, flag: &str) -> Result<T, UsageError> {
        raw.parse::<T>()
            .map_err(|_| UsageError(format!("{flag} expects a number, got {raw:?}")))
    }

    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Parsed::Help),
            "-V" | "--version" => return Ok(Parsed::Version),
            "--register" => return Ok(Parsed::Register),
            "--unregister" => return Ok(Parsed::Unregister),
            "--register-protocol" => return Ok(Parsed::RegisterProtocol),
            "--unregister-protocol" => return Ok(Parsed::UnregisterProtocol),
            "--protocol-status" => return Ok(Parsed::ProtocolStatus),
            "--list-actions" => return Ok(Parsed::ListActions),
            "--open-uri" => {
                let uri = value(&mut it, "--open-uri")?;
                return Ok(Parsed::OpenUri(uri));
            }
            "--title" => title = value(&mut it, "--title")?,
            "-l" | "--level" => level_name = value(&mut it, "--level")?,
            "--icon" => {
                let raw = value(&mut it, "--icon")?;
                if raw == "app" {
                    app_icon = true;
                } else {
                    icon = Some(raw);
                }
            }
            "--url" => url = Some(value(&mut it, "--url")?),
            "--on-click" => {
                // Everything after --on-click is the command argv.
                let rest: Vec<String> = it.collect();
                if rest.is_empty() {
                    return Err(UsageError("--on-click needs at least one argument".into()));
                }
                on_click = Some(rest);
                break;
            }
            "--button" => button_specs.push(value(&mut it, "--button")?),
            "--silent" => silent = true,
            "--print-xml" => print_xml = true,
            "--app-id" => app_id = value(&mut it, "--app-id")?,
            "--tag" => tag = Some(value(&mut it, "--tag")?),
            "--in" => delay = number(&value(&mut it, "--in")?, "--in")?,
            "--repeat" => repeat = number(&value(&mut it, "--repeat")?, "--repeat")?,
            "--every" => every = number(&value(&mut it, "--every")?, "--every")?,
            other if other.starts_with('-') && other.len() > 1 => {
                return Err(UsageError(format!("unknown option {other:?}")));
            }
            other => {
                if message.is_some() {
                    return Err(UsageError(format!(
                        "unexpected extra argument {other:?} (quote the message?)"
                    )));
                }
                message = Some(other.to_string());
            }
        }
    }

    let message = message.ok_or_else(|| UsageError("a message is required".into()))?;

    let level = level::by_name(&level_name).ok_or_else(|| {
        UsageError(format!(
            "invalid level {:?} (choose from {})",
            level_name,
            level::names().join(", ")
        ))
    })?;

    // LABEL=URL, split on the first '=' so the URL may contain its own.
    let mut buttons = Vec::new();
    for spec in &button_specs {
        match spec.split_once('=') {
            Some((label, target)) if !label.trim().is_empty() && !target.trim().is_empty() => {
                buttons.push((label.trim().to_string(), target.trim().to_string()))
            }
            _ => {
                return Err(UsageError(format!(
                    "--button expects LABEL=URL, got {spec:?}"
                )))
            }
        }
    }
    if buttons.len() > 5 {
        return Err(UsageError(
            "Windows allows at most 5 buttons per notification".into(),
        ));
    }

    if repeat == 0 {
        return Err(UsageError("--repeat must be at least 1".into()));
    }

    let icon = match icon {
        Some(raw) => {
            let path = PathBuf::from(&raw);
            if !path.is_file() {
                return Err(UsageError(format!("--icon file not found: {raw}")));
            }
            Some(path)
        }
        None => None,
    };

    Ok(Parsed::Run(Args {
        message,
        title,
        level,
        icon,
        app_icon,
        url,
        on_click,
        buttons,
        silent,
        delay,
        repeat,
        every,
        app_id,
        print_xml,
        tag,
    }))
}

fn run(args: Args) -> ExitCode {
    // Fall back to the level's generated icon when no custom one was given,
    // or to the app icon when --icon app was.
    let generated = match (&args.icon, args.app_icon) {
        (Some(_), _) => None,
        (None, true) => icon::app_icon(),
        (None, false) => icon::level_icon(args.level),
    };
    let icon: Option<&Path> = args.icon.as_deref().or(generated.as_deref());

    // If --on-click is set, register the action and use its URI as the launch target.
    let action_uri = if let Some(ref on_click_argv) = args.on_click {
        if !callback::protocol_registered() {
            eprintln!(
                "error: --on-click requires the {} scheme to be registered.\n\
                 Run: reminder --register-protocol",
                callback::SCHEME
            );
            return ExitCode::from(2);
        }
        let argv_refs: Vec<&str> = on_click_argv.iter().map(|s| s.as_str()).collect();
        let cwd = env::current_dir()
            .ok()
            .map(|p| p.to_string_lossy().into_owned());
        match callback::register_action(&argv_refs, cwd.as_deref()) {
            Ok(uri) => Some(uri),
            Err(e) => {
                eprintln!("error: cannot register click action: {e}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        None
    };

    // The effective URL: --on-click's action URI takes precedence over --url.
    let effective_url = action_uri.as_deref().or(args.url.as_deref());

    let payload = Toast {
        title: &args.title,
        message: &args.message,
        level: args.level,
        sound: !args.silent,
        url: effective_url,
        buttons: &args.buttons,
        icon,
        tag: args.tag.as_deref(),
    };

    if args.print_xml {
        println!("{}", payload.to_xml());
        return ExitCode::SUCCESS;
    }

    // WinRT wants an initialised apartment. An implicit MTA is the right choice
    // for a console tool with no message loop, and it needs no matching call
    // on the way out.
    unsafe {
        let _ = CoIncrementMTAUsage();
    }

    // When not overridden, use our own AppUserModelId: make sure its identity
    // shortcut exists and point this process at it, so Action Center shows
    // "Reminder" instead of "Windows PowerShell". Registration is one-time.
    if args.app_id == DEFAULT_APP_ID {
        if let Err(e) = appid::ensure_registered() {
            eprintln!("warning: could not register AppUserModelId: {e}");
        }
        if let Err(e) = appid::set_process_aumid() {
            eprintln!("warning: could not set process AppUserModelId: {e}");
        }
    }

    if args.delay > 0 {
        println!("Waiting {}s before reminding...", args.delay);
        thread::sleep(Duration::from_secs(args.delay));
    }

    let mut failed = false;
    for i in 0..args.repeat {
        match payload.show(&args.app_id) {
            Ok(()) => {
                if args.repeat > 1 {
                    println!("Sent ({}/{}): {}", i + 1, args.repeat, args.message);
                } else {
                    println!("Sent: {}", args.message);
                }
            }
            Err(e) => {
                eprintln!("Notification failed: {e}");
                failed = true;
            }
        }

        if i + 1 < args.repeat {
            thread::sleep(Duration::from_secs(args.every));
        }
    }

    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();

    if argv.is_empty() {
        print!("{USAGE}");
        return ExitCode::from(2);
    }

    match parse_args(argv) {
        Ok(Parsed::Run(args)) => run(args),
        Ok(Parsed::Help) => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Parsed::Version) => {
            println!("reminder {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Ok(Parsed::Register) => match appid::register() {
            Ok(()) => {
                println!(
                    "Registered AppUserModelId '{}' (Start Menu shortcut).",
                    appid::AUMID
                );
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        },
        Ok(Parsed::Unregister) => match appid::unregister() {
            Ok(()) => {
                println!("Unregistered AppUserModelId '{}'.", appid::AUMID);
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        },
        Ok(Parsed::RegisterProtocol) => {
            match callback::register_protocol() {
                Ok(()) => {
                    println!("Registered {}:// scheme.", callback::SCHEME);
                    println!("  {}", callback::protocol_command());
                    // Prune old actions while we're here.
                    let pruned = callback::prune_actions();
                    if pruned > 0 {
                        println!("  (pruned {pruned} expired action(s))");
                    }
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Ok(Parsed::UnregisterProtocol) => match callback::unregister_protocol() {
            Ok(()) => {
                println!("Unregistered {}:// scheme.", callback::SCHEME);
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        },
        Ok(Parsed::ProtocolStatus) => match callback::registered_protocol_command() {
            Some(cmd) => {
                println!("{}:// -> {cmd}", callback::SCHEME);
                ExitCode::SUCCESS
            }
            None => {
                println!("{}:// is not registered.", callback::SCHEME);
                println!("Run: reminder --register-protocol");
                ExitCode::FAILURE
            }
        },
        Ok(Parsed::ListActions) => {
            let actions = callback::list_actions();
            if actions.is_empty() {
                println!("No stored actions.");
            } else {
                println!("{} action(s):", actions.len());
                for (token, action) in &actions {
                    println!(
                        "  {} ({}s ago): {}",
                        &token[..token.len().min(12)],
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs()
                            .saturating_sub(action.created),
                        action.argv.join(" ")
                    );
                }
            }
            ExitCode::SUCCESS
        }
        Ok(Parsed::OpenUri(uri)) => match callback::parse_action_uri(&uri) {
            Ok(token) => match callback::run_action(&token) {
                Ok(code) => ExitCode::from(code as u8),
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::FAILURE
                }
            },
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        },
        Err(UsageError(message)) => {
            eprintln!("error: {message}");
            eprintln!("\nTry 'reminder --help' for usage.");
            ExitCode::from(2)
        }
    }
}
