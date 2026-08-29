//! Building and showing a toast.
//!
//! The XML payload is assembled by hand and handed to WinRT. Element order
//! inside `<toast>` is fixed by the schema: visual, audio, then actions.

use std::path::Path;

use windows::core::HSTRING;
use windows::Data::Xml::Dom::XmlDocument;
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};

use crate::level::Level;

/// Escape for XML text nodes and for single- or double-quoted attributes.
///
/// Apostrophes matter: attributes below are single-quoted, so a button label
/// like "It's fine" would otherwise produce a malformed payload that Windows
/// discards without reporting an error.
pub fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Convert a filesystem path into the file:// URI a toast image requires.
/// A bare Windows path is not accepted there.
pub fn path_to_file_uri(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut out = String::from("file:///");
    for ch in text.chars() {
        match ch {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '.' | '_' | '~' | '/' | ':' => out.push(ch),
            _ => {
                let mut buf = [0u8; 4];
                for byte in ch.encode_utf8(&mut buf).as_bytes() {
                    out.push_str(&format!("%{byte:02X}"));
                }
            }
        }
    }
    out
}

/// Everything that varies between one toast and the next.
pub struct Toast<'a> {
    pub title: &'a str,
    pub message: &'a str,
    pub level: &'a Level,
    pub sound: bool,
    /// Clicking the toast body launches this, via protocol activation.
    pub url: Option<&'a str>,
    /// (label, url) pairs rendered as buttons. Windows allows at most five.
    pub buttons: &'a [(String, String)],
    pub icon: Option<&'a Path>,
}

impl Toast<'_> {
    /// Render the XML payload.
    pub fn to_xml(&self) -> String {
        // Protocol activation hands the launch string to the shell, which
        // resolves it with the registered handler -- http(s) goes to the
        // default browser.
        let activation = match self.url {
            Some(url) => format!(" activationType='protocol' launch='{}'", xml_escape(url)),
            None => String::new(),
        };

        let image = match self.icon {
            Some(path) => format!(
                "<image placement='appLogoOverride' src='{}'/>",
                xml_escape(&path_to_file_uri(path))
            ),
            None => String::new(),
        };

        let audio = if self.sound {
            format!("<audio src='{}'/>", xml_escape(self.level.sound))
        } else {
            "<audio silent='true'/>".to_string()
        };

        let actions = if self.buttons.is_empty() {
            String::new()
        } else {
            let items: String = self
                .buttons
                .iter()
                .map(|(label, target)| {
                    format!(
                        "<action content='{}' activationType='protocol' arguments='{}'/>",
                        xml_escape(label),
                        xml_escape(target)
                    )
                })
                .collect();
            format!("<actions>{items}</actions>")
        };

        format!(
            "<toast duration='long'{activation}>\
             <visual><binding template='ToastGeneric'>\
             <text>{}</text><text>{}</text>{image}\
             </binding></visual>\
             {audio}{actions}</toast>",
            xml_escape(self.title),
            xml_escape(self.message),
        )
    }

    /// Push the toast into the Windows Notification Center.
    pub fn show(&self, app_id: &str) -> windows::core::Result<()> {
        let document = XmlDocument::new()?;
        document.LoadXml(&HSTRING::from(self.to_xml()))?;

        let notification = ToastNotification::CreateToastNotification(&document)?;
        let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(app_id))?;
        notifier.Show(&notification)
    }
}
