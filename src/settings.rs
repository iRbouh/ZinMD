//! What the user chose, kept between launches: the editor font and its size.
//!
//! Stored as a small JSON file in the user's Application Support folder. It is
//! read once, before the window opens; a missing or damaged file means defaults.

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};

use gpui_kit::*;

/// The fonts offered for the editor: the two most popular open coding fonts,
/// which ship inside the app, and the one every Mac has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorFont {
    JetBrainsMono,
    FiraCode,
    Menlo,
}

impl EditorFont {
    pub const ALL: [EditorFont; 3] = [
        EditorFont::JetBrainsMono,
        EditorFont::FiraCode,
        EditorFont::Menlo,
    ];

    /// The name shown to the user.
    pub fn label(self) -> &'static str {
        match self {
            EditorFont::JetBrainsMono => "JetBrains Mono",
            EditorFont::FiraCode => "Fira Code",
            EditorFont::Menlo => "Menlo",
        }
    }

    /// The name the text system finds the font by.
    pub fn family(self) -> &'static str {
        match self {
            EditorFont::JetBrainsMono => "JetBrains Mono",
            EditorFont::FiraCode => "Fira Code",
            // Lower case on purpose. "Menlo" is the component kit's own default,
            // and left on its default the kit lists every installed font to make
            // sure it exists (about 70 ms). The system finds "menlo" just the same.
            EditorFont::Menlo => "menlo",
        }
    }

    fn files(self) -> &'static [&'static [u8]] {
        match self {
            EditorFont::JetBrainsMono => &[
                include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf"),
                include_bytes!("../assets/fonts/JetBrainsMono-Bold.ttf"),
                include_bytes!("../assets/fonts/JetBrainsMono-Italic.ttf"),
                include_bytes!("../assets/fonts/JetBrainsMono-BoldItalic.ttf"),
            ],
            EditorFont::FiraCode => &[
                include_bytes!("../assets/fonts/FiraCode-Regular.ttf"),
                include_bytes!("../assets/fonts/FiraCode-Bold.ttf"),
            ],
            EditorFont::Menlo => &[],
        }
    }

    /// Make the font usable. Fonts that ship with the app are only handed to
    /// the text system when first needed, so the ones not in use cost nothing.
    pub fn load(self, cx: &App) {
        static LOADED: AtomicU8 = AtomicU8::new(0);
        let bit = 1u8 << self as u8;
        if LOADED.fetch_or(bit, Ordering::Relaxed) & bit != 0 || self.files().is_empty() {
            return;
        }
        let files = self
            .files()
            .iter()
            .map(|file| Cow::Borrowed(*file))
            .collect();
        if let Err(error) = cx.text_system().add_fonts(files) {
            eprintln!("could not load {}: {error}", self.label());
        }
    }
}

pub const DEFAULT_FONT_SIZE: f32 = 13.;
const FONT_SIZES: std::ops::RangeInclusive<f32> = 9. ..=32.;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub font: EditorFont,
    pub font_size: f32,
}

impl Global for Settings {}

impl Default for Settings {
    fn default() -> Self {
        Self {
            font: EditorFont::JetBrainsMono,
            font_size: DEFAULT_FONT_SIZE,
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        let text = file()
            .and_then(|file| std::fs::read_to_string(file).ok())
            .unwrap_or_default();
        Self::parse(&text)
    }

    fn parse(text: &str) -> Self {
        let mut settings = Self::default();
        let Ok(json) = serde_json::from_str::<serde_json::Value>(text) else {
            return settings;
        };
        if let Some(name) = json["editor_font"].as_str() {
            settings.font = EditorFont::ALL
                .into_iter()
                .find(|font| font.label() == name)
                .unwrap_or(settings.font);
        }
        if let Some(size) = json["editor_font_size"].as_f64() {
            settings.font_size = clamp_size(size as f32);
        }
        settings
    }

    fn to_json(self) -> String {
        let json = serde_json::json!({ "editor_font": self.font.label(), "editor_font_size": self.font_size });
        serde_json::to_string_pretty(&json).unwrap_or_default()
    }

    /// Write the settings to disk. Failing to is not worth interrupting the user for.
    pub fn save(self) {
        let Some(file) = file() else { return };
        if let Some(folder) = file.parent() {
            std::fs::create_dir_all(folder).ok();
        }
        std::fs::write(file, self.to_json()).ok();
    }
}

pub fn clamp_size(size: f32) -> f32 {
    size.round().clamp(*FONT_SIZES.start(), *FONT_SIZES.end())
}

fn file() -> Option<PathBuf> {
    // Tests and scripted runs point this elsewhere, to leave the real settings alone.
    if let Some(file) = std::env::var_os("ZWINMD_SETTINGS") {
        return Some(PathBuf::from(file));
    }
    let home = PathBuf::from(std::env::var_os("HOME")?);
    Some(home.join("Library/Application Support/ZwinMD/settings.json"))
}

#[cfg(test)]
mod tests {
    use super::{EditorFont, Settings};

    #[test]
    fn round_trips() {
        let settings = Settings {
            font: EditorFont::FiraCode,
            font_size: 15.,
        };
        assert_eq!(Settings::parse(&settings.to_json()), settings);
    }

    #[test]
    fn falls_back_to_defaults() {
        assert_eq!(Settings::parse(""), Settings::default());
        assert_eq!(Settings::parse("{ not json"), Settings::default());
        // An unknown font keeps the default; a wild size is brought into range.
        let odd = Settings::parse(r#"{"editor_font": "Comic Sans", "editor_font_size": 400}"#);
        assert_eq!(
            odd,
            Settings {
                font: EditorFont::JetBrainsMono,
                font_size: 32.
            }
        );
    }
}
