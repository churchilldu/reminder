//! Severity levels.
//!
//! Windows toasts have no native notion of severity, so each level is
//! expressed with an icon colour, a glyph, and a system sound.
//!
//! The `scenario` attribute is deliberately unused: its only severity-ish
//! value, "urgent", is Windows 11 only, and an unrecognised value risks the
//! whole payload being dropped.

/// Which mark to draw inside the icon's disc.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    /// Lower-case i: a dot above a bar.
    Info,
    /// A tick.
    Check,
    /// Exclamation mark: a bar above a dot.
    Bang,
    /// A cross.
    Cross,
}

pub struct Level {
    pub name: &'static str,
    pub colour: (u8, u8, u8),
    pub glyph: Glyph,
    pub sound: &'static str,
}

pub const LEVELS: &[Level] = &[
    Level {
        name: "info",
        colour: (0x00, 0x78, 0xD4),
        glyph: Glyph::Info,
        sound: "ms-winsoundevent:Notification.Default",
    },
    Level {
        name: "success",
        colour: (0x10, 0x7C, 0x10),
        glyph: Glyph::Check,
        sound: "ms-winsoundevent:Notification.Default",
    },
    Level {
        name: "warning",
        colour: (0xE8, 0xA1, 0x00),
        glyph: Glyph::Bang,
        sound: "ms-winsoundevent:Notification.Reminder",
    },
    Level {
        name: "error",
        colour: (0xC4, 0x2B, 0x1C),
        glyph: Glyph::Cross,
        sound: "ms-winsoundevent:Notification.Looping.Alarm2",
    },
];

/// Look a level up by name, or None if it isn't one of ours.
pub fn by_name(name: &str) -> Option<&'static Level> {
    LEVELS.iter().find(|level| level.name == name)
}

/// Every level name, for help text and error messages.
pub fn names() -> Vec<&'static str> {
    LEVELS.iter().map(|level| level.name).collect()
}
