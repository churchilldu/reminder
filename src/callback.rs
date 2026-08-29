//! Click callbacks via a registered `reminder://` URI scheme.
//!
//! Windows toasts support protocol activation: clicking a toast launches a URL.
//! We register a custom scheme (`reminder://`) whose handler is this binary.
//! The command to run on click is NEVER placed in the URI -- that would be a
//! remote code execution hole since any web page can trigger a custom scheme.
//!
//! Instead:
//! 1. The command (argv + optional cwd) is written to a local action store as
//!    a JSON file keyed by a random token.
//! 2. The toast's launch URI is `reminder://run/<token>`.
//! 3. When Windows invokes the handler, we look up the token, load the stored
//!    argv, and execute it directly (no shell).
//!
//! The worst a hostile page can do is re-fire an action one of your own scripts
//! already registered; it cannot compose a new one.

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use windows::core::PCWSTR;
use windows::Win32::Foundation::WIN32_ERROR;
use windows::Win32::System::Registry::*;

const ERROR_SUCCESS: WIN32_ERROR = WIN32_ERROR(0);

// ─── Constants ───────────────────────────────────────────────────────────────

pub const SCHEME: &str = "reminder";
const ACTION_HOST: &str = "run";
const REGISTRY_DESCRIPTION: &str = "URL:reminder action";

/// How long a stored action stays usable. 7 days: long enough to click a toast
/// from Action Center the next day, short enough that the store doesn't grow
/// forever.
const ACTION_TTL_SECONDS: u64 = 7 * 24 * 3600;

/// Token length in bytes before base64 encoding. 22 URL-safe characters.
const TOKEN_BYTES: usize = 16;

// ─── Paths ───────────────────────────────────────────────────────────────────

fn data_dir() -> PathBuf {
    let base = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    base.join("reminder")
}

fn actions_dir() -> PathBuf {
    data_dir().join("actions")
}

fn action_path(token: &str) -> Option<PathBuf> {
    // Strict validation: only URL-safe base64 characters. Prevents path
    // traversal via "../../" tokens.
    if token.len() < 16
        || token.len() > 128
        || !token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return None;
    }
    Some(actions_dir().join(format!("{token}.json")))
}

// ─── Token generation ────────────────────────────────────────────────────────

/// Generate a random URL-safe token.
fn generate_token() -> String {
    // Use CoCreateGuid which calls the OS CSPRNG. We generate two GUIDs
    // (32 bytes total) and take 16 bytes for a 22-character URL-safe token.
    use windows::Win32::System::Com::CoCreateGuid;
    let mut bytes = [0u8; TOKEN_BYTES];
    unsafe {
        if let Ok(g1) = CoCreateGuid() {
            let ptr = &g1 as *const _ as *const u8;
            let slice = std::slice::from_raw_parts(ptr, 16);
            bytes.copy_from_slice(slice);
        } else {
            // Fallback: use process id + time as entropy (weak but functional)
            let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64;
            let pid = std::process::id() as u64;
            bytes[..8].copy_from_slice(&t.to_le_bytes());
            bytes[8..16].copy_from_slice(&pid.wrapping_mul(0x517cc1b727220a95).to_le_bytes());
        }
    }
    base64_url_encode(&bytes)
}

/// URL-safe base64 (no padding) -- enough for token generation.
fn base64_url_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity((data.len() * 4).div_ceil(3));
    let mut i = 0;
    while i + 2 < data.len() {
        let n = (data[i] as u32) << 16 | (data[i + 1] as u32) << 8 | data[i + 2] as u32;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(TABLE[((n >> 6) & 63) as usize] as char);
        out.push(TABLE[(n & 63) as usize] as char);
        i += 3;
    }
    let remaining = data.len() - i;
    if remaining == 2 {
        let n = (data[i] as u32) << 16 | (data[i + 1] as u32) << 8;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(TABLE[((n >> 6) & 63) as usize] as char);
    } else if remaining == 1 {
        let n = (data[i] as u32) << 16;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
    }
    out
}

// ─── Action store ────────────────────────────────────────────────────────────

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// An action stored in the action directory.
#[derive(Debug)]
pub struct StoredAction {
    pub argv: Vec<String>,
    pub cwd: Option<String>,
    pub created: u64,
}

/// Register an action and return its launch URI.
///
/// The argv is stored as a JSON file. The returned URI can be used as the
/// toast's activation string.
pub fn register_action(argv: &[&str], cwd: Option<&str>) -> Result<String, String> {
    let dir = actions_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create action store: {e}"))?;

    let token = generate_token();
    let path = action_path(&token).ok_or("generated token failed validation")?;

    let json = format!(
        "{{\"argv\":{},\"cwd\":{},\"created\":{}}}",
        json_string_array(argv),
        match cwd {
            Some(c) => format!("\"{}\"", json_escape(c)),
            None => "null".to_string(),
        },
        now_epoch()
    );

    fs::write(&path, json.as_bytes()).map_err(|e| format!("cannot write action: {e}"))?;
    Ok(action_uri(&token))
}

/// Load a stored action by token.
pub fn load_action(token: &str) -> Option<StoredAction> {
    let path = action_path(token)?;
    let content = fs::read_to_string(&path).ok()?;
    parse_action_json(&content)
}

/// Execute a stored action. Returns the command's exit code.
pub fn run_action(token: &str) -> Result<i32, String> {
    let action = load_action(token).ok_or("action has expired or no longer exists")?;
    if action.argv.is_empty() {
        return Err("stored action has no command".into());
    }

    // Check TTL
    let age = now_epoch().saturating_sub(action.created);
    if age > ACTION_TTL_SECONDS {
        // Remove expired
        if let Some(p) = action_path(token) {
            let _ = fs::remove_file(p);
        }
        return Err("action has expired".into());
    }

    eprintln!("Running: {}", action.argv.join(" "));
    let mut cmd = Command::new(&action.argv[0]);
    if action.argv.len() > 1 {
        cmd.args(&action.argv[1..]);
    }
    if let Some(ref cwd) = action.cwd {
        cmd.current_dir(cwd);
    }

    match cmd.status() {
        Ok(status) => Ok(status.code().unwrap_or(1)),
        Err(e) => Err(format!("failed to start {:?}: {e}", action.argv[0])),
    }
}

/// Delete expired action files. Returns how many were removed.
pub fn prune_actions() -> usize {
    let dir = actions_dir();
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return 0,
    };

    let now = now_epoch();
    let mut removed = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Ok(content) = fs::read_to_string(&path) {
            if let Some(action) = parse_action_json(&content) {
                if now.saturating_sub(action.created) > ACTION_TTL_SECONDS {
                    let _ = fs::remove_file(&path);
                    removed += 1;
                }
            }
        }
    }
    removed
}

/// List all live actions (for auditing).
pub fn list_actions() -> Vec<(String, StoredAction)> {
    let dir = actions_dir();
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut out: Vec<(String, StoredAction)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let token = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if let Ok(content) = fs::read_to_string(&path) {
            if let Some(action) = parse_action_json(&content) {
                out.push((token, action));
            }
        }
    }
    out.sort_by_key(|a| std::cmp::Reverse(a.1.created));
    out
}

// ─── URI handling ────────────────────────────────────────────────────────────

fn action_uri(token: &str) -> String {
    format!("{SCHEME}://{ACTION_HOST}/{token}")
}

/// Parse the token out of a `reminder://run/<token>` URI.
pub fn parse_action_uri(uri: &str) -> Result<String, String> {
    // Accept: reminder://run/TOKEN or reminder://run/TOKEN?... (query ignored)
    let prefix = format!("{SCHEME}://{ACTION_HOST}/");
    let rest = uri
        .strip_prefix(&prefix)
        .ok_or_else(|| format!("not a valid {SCHEME}:// URI: {uri}"))?;
    let token = rest.split(['?', '#']).next().unwrap_or(rest);
    if action_path(token).is_none() {
        return Err(format!("invalid token in URI: {uri}"));
    }
    Ok(token.to_string())
}

// ─── Registry: URI scheme ────────────────────────────────────────────────────

/// The command string we register as the scheme handler.
pub fn protocol_command() -> String {
    let exe = env::current_exe().unwrap_or_else(|_| PathBuf::from("reminder.exe"));
    format!("\"{}\" --open-uri \"%1\"", exe.display())
}

/// Register the `reminder://` scheme for the current user (no admin needed).
pub fn register_protocol() -> Result<(), String> {
    unsafe {
        // Create the scheme key
        let key_path = to_wide(&format!("Software\\Classes\\{SCHEME}"));
        let mut key = HKEY::default();
        let rc = RegCreateKeyW(HKEY_CURRENT_USER, PCWSTR(key_path.as_ptr()), &mut key);
        if rc != ERROR_SUCCESS {
            return Err(format!("cannot create scheme key: error {}", rc.0));
        }

        // Set the default value (description)
        let desc = to_wide(REGISTRY_DESCRIPTION);
        let rc = RegSetValueExW(key, PCWSTR::null(), 0, REG_SZ, Some(as_bytes(&desc)));
        if rc != ERROR_SUCCESS {
            let _ = RegCloseKey(key);
            return Err(format!("cannot set description: error {}", rc.0));
        }

        // "URL Protocol" must exist (even empty) for Windows to recognise this
        let url_protocol = to_wide("URL Protocol");
        let empty = to_wide("");
        let rc = RegSetValueExW(
            key,
            PCWSTR(url_protocol.as_ptr()),
            0,
            REG_SZ,
            Some(as_bytes(&empty)),
        );
        if rc != ERROR_SUCCESS {
            let _ = RegCloseKey(key);
            return Err(format!("cannot set URL Protocol: error {}", rc.0));
        }
        let _ = RegCloseKey(key);

        // shell\open\command
        let cmd_path = to_wide(&format!(
            "Software\\Classes\\{SCHEME}\\shell\\open\\command"
        ));
        let mut cmd_key = HKEY::default();
        let rc = RegCreateKeyW(HKEY_CURRENT_USER, PCWSTR(cmd_path.as_ptr()), &mut cmd_key);
        if rc != ERROR_SUCCESS {
            return Err(format!("cannot create command key: error {}", rc.0));
        }

        let cmd_value = to_wide(&protocol_command());
        let rc = RegSetValueExW(
            cmd_key,
            PCWSTR::null(),
            0,
            REG_SZ,
            Some(as_bytes(&cmd_value)),
        );
        let _ = RegCloseKey(cmd_key);
        if rc != ERROR_SUCCESS {
            return Err(format!("cannot set command: error {}", rc.0));
        }
    }
    Ok(())
}

/// Remove the scheme's registry keys.
pub fn unregister_protocol() -> Result<(), String> {
    let paths = [
        format!("Software\\Classes\\{SCHEME}\\shell\\open\\command"),
        format!("Software\\Classes\\{SCHEME}\\shell\\open"),
        format!("Software\\Classes\\{SCHEME}\\shell"),
        format!("Software\\Classes\\{SCHEME}"),
    ];
    unsafe {
        for path in &paths {
            let wide = to_wide(path);
            let _ = RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(wide.as_ptr()));
        }
    }
    Ok(())
}

/// Read the currently registered handler command, if any.
pub fn registered_protocol_command() -> Option<String> {
    let cmd_path = to_wide(&format!(
        "Software\\Classes\\{SCHEME}\\shell\\open\\command"
    ));
    unsafe {
        let mut key = HKEY::default();
        let rc = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(cmd_path.as_ptr()),
            0,
            KEY_READ,
            &mut key,
        );
        if rc != ERROR_SUCCESS {
            return None;
        }

        let mut buf = vec![0u16; 1024];
        let mut size = (buf.len() * 2) as u32;
        let rc = RegQueryValueExW(
            key,
            PCWSTR::null(),
            None,
            None,
            Some(buf.as_mut_ptr() as *mut u8),
            Some(&mut size),
        );
        let _ = RegCloseKey(key);

        if rc != ERROR_SUCCESS {
            return None;
        }

        let len = (size as usize) / 2;
        // Trim null terminator
        let trimmed = if len > 0 && buf[len - 1] == 0 {
            &buf[..len - 1]
        } else {
            &buf[..len]
        };
        Some(String::from_utf16_lossy(trimmed))
    }
}

/// True if click callbacks can work on this machine.
pub fn protocol_registered() -> bool {
    registered_protocol_command().is_some()
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn as_bytes(wide: &[u16]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2) }
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < '\x20' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn json_string_array(items: &[&str]) -> String {
    let parts: Vec<String> = items
        .iter()
        .map(|s| format!("\"{}\"", json_escape(s)))
        .collect();
    format!("[{}]", parts.join(","))
}

/// Minimal JSON parser for our action format.
fn parse_action_json(content: &str) -> Option<StoredAction> {
    // We wrote it, so the format is known:
    // {"argv":["a","b"],"cwd":"path"|null,"created":12345}
    let argv = extract_string_array(content, "argv")?;
    let cwd = extract_string_value(content, "cwd");
    let created = extract_number(content, "created").unwrap_or(0);
    Some(StoredAction { argv, cwd, created })
}

fn extract_string_array(json: &str, key: &str) -> Option<Vec<String>> {
    let pattern = format!("\"{}\":", key);
    let start = json.find(&pattern)? + pattern.len();
    let rest = json[start..].trim_start();
    if !rest.starts_with('[') {
        return None;
    }
    let bracket_start = json.len() - rest.len();
    let bracket_end = find_matching_bracket(json, bracket_start)?;
    let inner = &json[bracket_start + 1..bracket_end];

    let mut items = Vec::new();
    let mut i = 0;
    let bytes = inner.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'"' {
            let s = parse_json_string(inner, &mut i)?;
            items.push(s);
        } else {
            i += 1;
        }
    }
    Some(items)
}

fn extract_string_value(json: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{}\":", key);
    let start = json.find(&pattern)? + pattern.len();
    let rest = json[start..].trim_start();
    if rest.starts_with("null") {
        return None;
    }
    if rest.starts_with('"') {
        let offset = json.len() - rest.len();
        let mut i = 0;
        let s = parse_json_string(rest, &mut i)?;
        let _ = offset; // suppress unused
        return Some(s);
    }
    None
}

fn extract_number(json: &str, key: &str) -> Option<u64> {
    let pattern = format!("\"{}\":", key);
    let start = json.find(&pattern)? + pattern.len();
    let rest = json[start..].trim_start();
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

fn find_matching_bracket(s: &str, start: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let open = bytes[start];
    let close = match open {
        b'[' => b']',
        b'{' => b'}',
        _ => return None,
    };
    let mut depth = 0i32;
    let mut in_string = false;
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if in_string => {
                i += 1;
            }
            b'"' => in_string = !in_string,
            c if c == open && !in_string => depth += 1,
            c if c == close && !in_string => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn parse_json_string(s: &str, pos: &mut usize) -> Option<String> {
    let bytes = s.as_bytes();
    if bytes.get(*pos)? != &b'"' {
        return None;
    }
    *pos += 1;
    let mut out = String::new();
    while *pos < bytes.len() {
        match bytes[*pos] {
            b'"' => {
                *pos += 1;
                return Some(out);
            }
            b'\\' => {
                *pos += 1;
                match bytes.get(*pos)? {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'/' => out.push('/'),
                    _ => {
                        out.push('\\');
                        out.push(bytes[*pos] as char);
                    }
                }
            }
            c => out.push(c as char),
        }
        *pos += 1;
    }
    None // unterminated string
}
