//! What the user chose, kept between launches: the accent colour, the editor
//! font and its size.
//!
//! Stored as a small JSON file in the user's Application Support folder. It is
//! read once, before the window opens; a missing or damaged file means defaults.

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};

use gpui_kit::*;

use crate::theme::Accent;

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

    /// The upright, regular weight of a font that ships with the app.
    pub fn regular(self) -> &'static [u8] {
        self.files().first().copied().unwrap_or_default()
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
    pub accent: Accent,
    pub font: EditorFont,
    pub font_size: f32,
    /// Whether the app looks for new versions by itself.
    pub updates: bool,
    /// How big the words of a page are, against the size the page would
    /// give them: 1 is that size, 1.2 a fifth bigger.
    pub reading_scale: f32,
    /// Whether a page is set in black and white: the darkest words on the
    /// lightest page in the light appearance, the other way round in the
    /// dark one. Near black and near white, not the glare of the real ones.
    pub contrast: bool,
}

impl Global for Settings {}

impl Default for Settings {
    fn default() -> Self {
        Self {
            accent: Accent::default(),
            font: EditorFont::JetBrainsMono,
            font_size: DEFAULT_FONT_SIZE,
            updates: true,
            reading_scale: 1.,
            contrast: false,
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
        if let Some(name) = json["accent"].as_str() {
            settings.accent = Accent::ALL
                .into_iter()
                .find(|accent| accent.label() == name)
                .unwrap_or(settings.accent);
        }
        if let Some(name) = json["editor_font"].as_str() {
            settings.font = EditorFont::ALL
                .into_iter()
                .find(|font| font.label() == name)
                .unwrap_or(settings.font);
        }
        if let Some(size) = json["editor_font_size"].as_f64() {
            settings.font_size = clamp_size(size as f32);
        }
        if let Some(updates) = json["look_for_updates"].as_bool() {
            settings.updates = updates;
        }
        if let Some(scale) = json["reading_text_scale"].as_f64() {
            settings.reading_scale = clamp_scale(scale as f32);
        }
        if let Some(contrast) = json["black_and_white"].as_bool() {
            settings.contrast = contrast;
        }
        settings
    }

    fn to_json(self) -> String {
        let json = serde_json::json!({
            "accent": self.accent.label(),
            "editor_font": self.font.label(),
            "editor_font_size": self.font_size,
            "look_for_updates": self.updates,
            "reading_text_scale": self.reading_scale,
            "black_and_white": self.contrast,
        });
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

/// How far the words of a page can be shrunk or grown, and by how much a
/// step changes them.
const READING_SCALES: std::ops::RangeInclusive<f32> = 0.75..=1.75;
/// One step is a sixteenth: a pixel, for words 16 pixels tall.
pub const READING_STEP: f32 = 0.0625;

pub fn clamp_scale(scale: f32) -> f32 {
    // Kept to whole steps, so ten steps up and ten down end where they began.
    ((scale / READING_STEP).round() * READING_STEP)
        .clamp(*READING_SCALES.start(), *READING_SCALES.end())
}

pub fn clamp_size(size: f32) -> f32 {
    size.round().clamp(*FONT_SIZES.start(), *FONT_SIZES.end())
}

fn file() -> Option<PathBuf> {
    Some(crate::session::data_dir().join("settings.json"))
}

#[cfg(test)]
mod tests {
    use super::{EditorFont, Settings};
    use crate::theme::Accent;

    #[test]
    fn round_trips() {
        let settings = Settings {
            accent: Accent::Sky,
            font: EditorFont::FiraCode,
            font_size: 15.,
            updates: false,
            reading_scale: 1.25,
            contrast: true,
        };
        assert_eq!(Settings::parse(&settings.to_json()), settings);
    }

    #[test]
    fn falls_back_to_defaults() {
        assert_eq!(Settings::parse(""), Settings::default());
        assert_eq!(Settings::parse("{ not json"), Settings::default());
        // An unknown colour or font keeps the default; a wild size is brought into range.
        let odd = Settings::parse(
            r#"{"accent": "Beige", "editor_font": "Comic Sans", "editor_font_size": 400}"#,
        );
        assert_eq!(
            odd,
            Settings {
                accent: Accent::Mustard,
                font: EditorFont::JetBrainsMono,
                font_size: 32.,
                updates: true,
                reading_scale: 1.,
                contrast: false,
            }
        );
        // Settings written before there were colours to choose from still load.
        let old = Settings::parse(r#"{"editor_font": "Menlo", "editor_font_size": 14}"#);
        assert_eq!(
            old,
            Settings {
                accent: Accent::Mustard,
                font: EditorFont::Menlo,
                font_size: 14.,
                // Looking for new versions is on unless it was turned off.
                updates: true,
                reading_scale: 1.,
                contrast: false,
            }
        );
    }
}
