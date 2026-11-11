//! What differs from one operating system to the next: which key gives the
//! commands, what things are called, and where things are kept.

use std::path::PathBuf;

/// A key binding given with the main command key: Command on a Mac, Control
/// everywhere else. `command!("shift-s")` is `cmd-shift-s` or `ctrl-shift-s`.
#[cfg(target_os = "macos")]
macro_rules! command {
    ($keys:literal) => {
        concat!("cmd-", $keys)
    };
}
#[cfg(not(target_os = "macos"))]
macro_rules! command {
    ($keys:literal) => {
        concat!("ctrl-", $keys)
    };
}

/// The same shortcut as it is shown to the user: `⌘E` on a Mac, `Ctrl+E` elsewhere.
#[cfg(target_os = "macos")]
macro_rules! shortcut {
    ($key:literal) => {
        concat!("⌘", $key)
    };
}
#[cfg(not(target_os = "macos"))]
macro_rules! shortcut {
    ($key:literal) => {
        concat!("Ctrl+", $key)
    };
}

pub(crate) use {command, shortcut};

/// Whether the main command key is held, and no other: Command on a Mac,
/// Control everywhere else.
pub fn only_command(held: &gpui_kit::Modifiers) -> bool {
    let (command, other) = match cfg!(target_os = "macos") {
        true => (held.platform, held.control),
        false => (held.control, held.platform),
    };
    command && !(other || held.alt || held.shift)
}

/// Whether the main command key is among the keys held.
pub fn command_held(held: &gpui_kit::Modifiers) -> bool {
    match cfg!(target_os = "macos") {
        true => held.platform,
        false => held.control,
    }
}

/// Whether the Control key is down right now, as the system says it is,
/// not as the last event a window was sent said. A window that is not the
/// one with the keyboard is never told that a key was let go of. `None`
/// where the system is not asked.
#[cfg(target_os = "macos")]
pub fn control_held() -> Option<bool> {
    use objc2::{class, msg_send};
    /// The Control key, among the flags of the keys that are held.
    const CONTROL: usize = 1 << 18;
    // SAFETY: a message to the class that takes nothing and returns the
    // flags of the keys held now, an unsigned word.
    let held: usize = unsafe { msg_send![class!(NSEvent), modifierFlags] };
    Some(held & CONTROL != 0)
}

#[cfg(not(target_os = "macos"))]
pub fn control_held() -> Option<bool> {
    None
}

/// What showing a file where it lives is called.
pub const REVEAL: &str = if cfg!(target_os = "macos") {
    "Show in Finder"
} else if cfg!(target_os = "windows") {
    "Show in Explorer"
} else {
    "Show in Files"
};

/// What throwing a file away, to where it can be got back from, is called.
pub const TRASH: &str = if cfg!(target_os = "windows") {
    "Move to Recycle Bin"
} else {
    "Move to Trash"
};

/// The name under which the system knows the app: its windows, and the entry
/// that starts it, carry the same one, so the two are shown as one thing.
pub const APP_ID: &str = "app.zinmd.ZinMD";

/// Which ZinMD this is. A release is built under the name of its tag, which
/// may say more than the number in Cargo.toml does: `0.1.0-rc.1`.
pub const VERSION: &str = match option_env!("ZINMD_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

/// The user's own folder.
pub fn home() -> Option<PathBuf> {
    let name = if cfg!(target_os = "windows") {
        "USERPROFILE"
    } else {
        "HOME"
    };
    folder_named(name)
}

/// Where the app keeps what it remembers, by each system's custom.
pub fn data_dir() -> PathBuf {
    let home = || home().unwrap_or_default();
    if cfg!(target_os = "macos") {
        home().join("Library/Application Support/ZinMD")
    } else if cfg!(target_os = "windows") {
        folder_named("APPDATA")
            .unwrap_or_else(|| home().join("AppData").join("Roaming"))
            .join("ZinMD")
    } else {
        folder_named("XDG_DATA_HOME")
            .unwrap_or_else(|| home().join(".local/share"))
            .join("zinmd")
    }
}

/// The folder an environment variable names, if it names one.
fn folder_named(variable: &str) -> Option<PathBuf> {
    std::env::var_os(variable)
        .filter(|folder| !folder.is_empty())
        .map(PathBuf::from)
}

/// A plain sans-serif font that comes with the system, for the labels of
/// diagrams. `None` where none of the usual ones is installed.
pub fn sans_font() -> Option<Vec<u8>> {
    let files: Vec<PathBuf> = if cfg!(target_os = "macos") {
        vec!["/System/Library/Fonts/Helvetica.ttc".into()]
    } else if cfg!(target_os = "windows") {
        let fonts = folder_named("WINDIR")
            .unwrap_or_else(|| "C:\\Windows".into())
            .join("Fonts");
        vec![fonts.join("arial.ttf"), fonts.join("segoeui.ttf")]
    } else {
        // The same few families, in the places the big distributions put them.
        [
            "truetype/dejavu/DejaVuSans.ttf",
            "dejavu-sans-fonts/DejaVuSans.ttf",
            "dejavu/DejaVuSans.ttf",
            "TTF/DejaVuSans.ttf",
            "truetype/liberation/LiberationSans-Regular.ttf",
            "liberation-sans/LiberationSans-Regular.ttf",
            "liberation-sans-fonts/LiberationSans-Regular.ttf",
            "liberation/LiberationSans-Regular.ttf",
            "truetype/noto/NotoSans-Regular.ttf",
            "google-noto/NotoSans-Regular.ttf",
            "noto/NotoSans-Regular.ttf",
        ]
        .into_iter()
        .map(|file| PathBuf::from("/usr/share/fonts").join(file))
        .collect()
    };
    files.into_iter().find_map(|file| std::fs::read(file).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_key_is_the_systems_own() {
        let (binding, shown) = (command!("shift-s"), shortcut!("E"));
        if cfg!(target_os = "macos") {
            assert_eq!((binding, shown), ("cmd-shift-s", "⌘E"));
        } else {
            assert_eq!((binding, shown), ("ctrl-shift-s", "Ctrl+E"));
        }
    }

    #[test]
    fn what_is_remembered_is_kept_under_the_apps_name() {
        let folder = data_dir();
        let name = folder.file_name().and_then(|name| name.to_str());
        assert!(matches!(name, Some("ZinMD" | "zinmd")), "{folder:?}");
    }
}
