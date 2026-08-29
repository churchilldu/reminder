//! App identity: register our own AppUserModelID so toasts show "Reminder"
//! as the sender instead of the Windows PowerShell AppID we used to borrow.
//!
//! Windows only accepts an AUMID for an unpackaged desktop app if it is
//! registered. The documented, reliable way is a Start Menu shortcut carrying
//! the `System.AppUserModel.ID` property (the same trick `toasty` uses):
//!
//!   1. Create `<Start Menu>\Reminder.lnk` pointing at this exe.
//!   2. Stamp `PKEY_AppUserModel_ID` = "Reminder.App" onto that shortcut.
//!   3. Every process that shows a toast calls
//!      `SetCurrentProcessExplicitAppUserModelID("Reminder.App")`.
//!
//! Only the first registration needs the shortcut; afterwards Action Center
//! attributes toasts to "Reminder".

use std::env;
use std::fs;
use std::path::PathBuf;

use windows::core::{Interface, PCWSTR, PROPVARIANT};
use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
use windows::Win32::System::Com::{
    CoCreateInstance, CoIncrementMTAUsage, IPersistFile, CLSCTX_INPROC_SERVER,
};
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::Win32::UI::Shell::{IShellLinkW, SetCurrentProcessExplicitAppUserModelID, ShellLink};

/// Our AppUserModelId. Paired with the `Reminder.lnk` Start Menu shortcut.
pub const AUMID: &str = "Reminder.App";

/// Name of the Start Menu shortcut. The on-disk name doubles as the display
/// name Action Center shows for the app.
const SHORTCUT_NAME: &str = "Reminder.lnk";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// `<Start Menu>\Programs` (the "current user" Programs folder).
fn start_menu_dir() -> Option<PathBuf> {
    env::var_os("APPDATA").map(PathBuf::from).map(|p| {
        p.join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
    })
}

/// Full path of the identity shortcut we create.
fn shortcut_path() -> Option<PathBuf> {
    start_menu_dir().map(|d| d.join(SHORTCUT_NAME))
}

/// Register the AppUserModelId by creating the Start Menu shortcut.
pub fn register() -> Result<(), String> {
    // ShellLink is a COM object; make sure an MTA is set up on this thread.
    unsafe {
        let _ = CoIncrementMTAUsage();
    }

    let exe = env::current_exe().map_err(|e| format!("cannot locate this executable: {e}"))?;
    let lnk = shortcut_path().ok_or("cannot find the Start Menu folder (no APPDATA)?")?;

    let mut exe_wide = wide(&exe.to_string_lossy());
    let mut lnk_wide = wide(&lnk.to_string_lossy());

    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
            .map_err(|e| format!("cannot create ShellLink COM object: {e}"))?;

        link.SetPath(PCWSTR(exe_wide.as_mut_ptr()))
            .map_err(|e| format!("cannot set shortcut target: {e}"))?;
        link.SetDescription(PCWSTR(wide("Reminder - native Windows notification reminder").as_mut_ptr()))
            .map_err(|e| format!("cannot set shortcut description: {e}"))?;

        // Stamp the AppUserModelID property onto the shortcut.
        let prop_store: IPropertyStore = link
            .cast()
            .map_err(|e| format!("cannot query IPropertyStore: {e}"))?;
        let prop = PROPVARIANT::from(AUMID);
        prop_store
            .SetValue(&PKEY_AppUserModel_ID, &prop)
            .map_err(|e| format!("cannot set AppUserModelID property: {e}"))?;
        prop_store
            .Commit()
            .map_err(|e| format!("cannot commit shortcut properties: {e}"))?;

        // Persist the shortcut to disk.
        let persist: IPersistFile = link
            .cast()
            .map_err(|e| format!("cannot query IPersistFile: {e}"))?;
        persist
            .Save(PCWSTR(lnk_wide.as_mut_ptr()), true)
            .map_err(|e| format!("cannot write shortcut: {e}"))?;
    }

    Ok(())
}

/// Remove the identity shortcut (stops toasts being attributed to "Reminder").
pub fn unregister() -> Result<(), String> {
    let Some(lnk) = shortcut_path() else {
        return Ok(());
    };
    if lnk.exists() {
        fs::remove_file(&lnk).map_err(|e| format!("cannot remove shortcut {}: {e}", lnk.display()))?;
    }
    Ok(())
}

/// Whether the identity shortcut currently exists.
pub fn is_registered() -> bool {
    shortcut_path().map_or(false, |p| p.exists())
}

/// Register on first use. Silent, and only acts once per machine -- the
/// shortcut needs to exist exactly once for its AUMID to be accepted.
pub fn ensure_registered() -> Result<(), String> {
    if is_registered() {
        Ok(())
    } else {
        register()
    }
}

/// Point this process at our AUMID so the toasts it shows are not attributed
/// to another app (e.g. PowerShell). Must be called before showing toasts.
pub fn set_process_aumid() -> Result<(), String> {
    let mut wide = wide(AUMID);
    unsafe {
        SetCurrentProcessExplicitAppUserModelID(PCWSTR(wide.as_mut_ptr()))
            .map_err(|e| format!("cannot set process AppUserModelID: {e}"))
    }
}
