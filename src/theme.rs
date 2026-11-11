//! Colours, sizes and fonts.
//!
//! `Palette` mirrors the "Theme" variable collection in `design/zinMD-UI.figcraft`:
//! same names, same values, one Light and one Dark set. Everything the app paints
//! itself reads from it. `kit_config` projects the same palette onto the component
//! kit, so its editor, scrollbars and popovers match without a second colour list.
//!
//! The palette is built around one accent colour, which the user picks out of
//! five: the "Accent" collection of the design file. Each is a ramp of shades,
//! and the palette takes the same shade of whichever ramp is in use.

use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::component::{ActiveTheme as _, Theme, ThemeConfig, ThemeMode};
use gpui_kit::*;

use crate::settings::Settings;

// ---------------------------------------------------------------- primitives

/// One accent colour as a ramp of shades, light to dark. Every ramp steps
/// through the same lightness, so a shade does the same job whichever the
/// colour: 500 is a fill that dark text reads on, 800 is text on a light page.
struct Ramp {
    s50: u32,
    s100: u32,
    s200: u32,
    s300: u32,
    s400: u32,
    s500: u32,
    s600: u32,
    s700: u32,
    s800: u32,
}

// Mustard: the brand colour. 500 is the classic mustard.
const MUSTARD: Ramp = Ramp {
    s50: 0xFEFAEB,
    s100: 0xFCF2CC,
    s200: 0xF8E499,
    s300: 0xF2D261,
    s400: 0xEBC034,
    s500: 0xE1AD01,
    s600: 0xC29300,
    s700: 0x9A7400,
    s800: 0x735600,
};
const MUSTARD_950: u32 = 0x2E2300;

// The other accents. Made in OKLCH on one lightness scale, a little darker
// through the middle than mustard, since yellow is the lightest of colours.
const TERRACOTTA: Ramp = Ramp {
    s50: 0xFFF5F2,
    s100: 0xFFE8E1,
    s200: 0xFFD3C5,
    s300: 0xFEB69E,
    s400: 0xFF946D,
    s500: 0xEE7F52,
    s600: 0xCF6637,
    s700: 0xA85025,
    s800: 0x803C19,
};
const OLIVE: Ramp = Ramp {
    s50: 0xF3FAEC,
    s100: 0xE5F4D6,
    s200: 0xD0E8B4,
    s300: 0xB9D68D,
    s400: 0xA3C26A,
    s500: 0x92AF52,
    s600: 0x7B9438,
    s700: 0x637626,
    s800: 0x4C591B,
};
const SKY: Ramp = Ramp {
    s50: 0xF0F9FF,
    s100: 0xDEF1FF,
    s200: 0xBFE3FF,
    s300: 0x96D2FE,
    s400: 0x6ABEFB,
    s500: 0x54AAEA,
    s600: 0x3C8FCD,
    s700: 0x2D73A7,
    s800: 0x22567F,
};
const LAVENDER: Ramp = Ramp {
    s50: 0xF9F5FF,
    s100: 0xF2E9FF,
    s200: 0xE7D5FE,
    s300: 0xD9BAFE,
    s400: 0xC89FF7,
    s500: 0xB68BE5,
    s600: 0x9B72C8,
    s700: 0x7D5AA3,
    s800: 0x5F447C,
};

/// The colour the app is accented with. Mustard is its own; the others are
/// there for the choosing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Accent {
    #[default]
    Mustard,
    Terracotta,
    Olive,
    Sky,
    Lavender,
}

impl Accent {
    pub const ALL: [Accent; 5] = [
        Accent::Mustard,
        Accent::Terracotta,
        Accent::Olive,
        Accent::Sky,
        Accent::Lavender,
    ];

    /// The name shown to the user, and the one kept in the settings.
    pub fn label(self) -> &'static str {
        match self {
            Accent::Mustard => "Mustard",
            Accent::Terracotta => "Terracotta",
            Accent::Olive => "Olive",
            Accent::Sky => "Sky",
            Accent::Lavender => "Lavender",
        }
    }

    fn ramp(self) -> &'static Ramp {
        match self {
            Accent::Mustard => &MUSTARD,
            Accent::Terracotta => &TERRACOTTA,
            Accent::Olive => &OLIVE,
            Accent::Sky => &SKY,
            Accent::Lavender => &LAVENDER,
        }
    }

    /// The colour itself, as shown where it is picked: the shade the app
    /// fills with in that appearance.
    pub fn swatch(self, dark: bool) -> Hsla {
        let ramp = self.ramp();
        c(if dark { ramp.s400 } else { ramp.s500 })
    }
}

// Stone: warm neutrals that sit well next to mustard.
const STONE_0: u32 = 0xFFFFFF;
const STONE_50: u32 = 0xFBFAF7;
const STONE_100: u32 = 0xF6F4EF;
const STONE_200: u32 = 0xECE9E1;
const STONE_300: u32 = 0xDCD8CD;
const STONE_400: u32 = 0xB8B3A6;
const STONE_500: u32 = 0x8E897C;
const STONE_600: u32 = 0x6C685E;
const STONE_700: u32 = 0x4B4841;
const STONE_850: u32 = 0x282622;
const STONE_900: u32 = 0x1E1D1A;
const STONE_925: u32 = 0x1A1917;
const STONE_950: u32 = 0x151412;

// Supporting hues, muted so they do not fight the mustard.
const BLUE_50: u32 = 0xEEF4FA;
const BLUE_300: u32 = 0x86B7E3;
const BLUE_600: u32 = 0x2E6BA6;
const BLUE_950: u32 = 0x14222F;
const TERRACOTTA_300: u32 = 0xEB9A73;
const TERRACOTTA_600: u32 = 0xB5502A;
const PLUM_300: u32 = 0xBCA4E3;
const PLUM_600: u32 = 0x7B5BA6;
const OLIVE_50: u32 = 0xEFF6EA;
const OLIVE_300: u32 = 0xA3C98E;
const OLIVE_600: u32 = 0x4F7A3A;
const OLIVE_950: u32 = 0x17251A;
const RED_50: u32 = 0xFCEFEE;
const RED_300: u32 = 0xF0958C;
const RED_600: u32 = 0xB8322A;
const RED_950: u32 = 0x2E1513;

fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

/// A primitive at a given opacity.
fn a(hex: u32, opacity: f32) -> Hsla {
    let mut color = c(hex);
    color.a = opacity;
    color
}

// ------------------------------------------------------------------- palette

macro_rules! palette {
    ($ramp:ident; $($(#[$doc:meta])* $name:ident: $light:expr, $dark:expr;)*) => {
        /// Every colour the app paints with, for one appearance.
        // The full token set from the design file; not every token is in use yet.
        #[allow(dead_code)]
        #[derive(Clone, Copy, Debug)]
        pub struct Palette {
            $($(#[$doc])* pub $name: Hsla,)*
        }

        impl Palette {
            pub fn light(accent: Accent) -> Self {
                let $ramp = accent.ramp();
                Self { $($name: $light,)* }
            }

            pub fn dark(accent: Accent) -> Self {
                let $ramp = accent.ramp();
                Self { $($name: $dark,)* }
            }

            /// The palette `progress` of the way from `from` to `to`.
            fn between(from: &Self, to: &Self, progress: f32) -> Self {
                Self { $($name: blend(from.$name, to.$name, progress),)* }
            }
        }
    };
}

palette! {
    ramp;
    /// Window chrome: title bar, sidebar, status bar.
    surface_app: c(STONE_100), c(STONE_950);
    surface_editor: c(STONE_50), c(STONE_925);
    surface_preview: c(STONE_0), c(STONE_900);
    /// Things that sit above a surface: the active tab, buttons, popovers.
    surface_raised: c(STONE_0), c(STONE_850);
    /// Things set into a surface: code blocks, table headers.
    surface_sunken: c(STONE_100), c(STONE_950);
    surface_hover: c(STONE_200), c(STONE_850);
    surface_selected: c(ramp.s100), a(ramp.s500, 0.18);

    border_subtle: c(STONE_200), c(STONE_850);
    border_strong: c(STONE_300), c(STONE_700);
    // Lines drawn on a raised surface, such as the rules in a menu. In the
    // dark appearance the subtle border is the raised surface's own colour.
    border_raised: c(STONE_200), a(STONE_700, 0.7);

    text_primary: c(STONE_900), c(STONE_200);
    text_secondary: c(STONE_600), c(STONE_400);
    text_tertiary: c(STONE_500), c(STONE_500);
    text_disabled: c(STONE_400), c(STONE_600);
    text_on_accent: c(STONE_900), c(STONE_950);

    accent: c(ramp.s500), c(ramp.s400);
    accent_hover: c(ramp.s600), c(ramp.s300);
    accent_soft: c(ramp.s50), a(ramp.s500, 0.12);
    accent_muted: c(ramp.s200), a(ramp.s500, 0.30);
    accent_text: c(ramp.s800), c(ramp.s300);
    accent_border: c(ramp.s400), c(ramp.s700);

    syntax_heading: c(ramp.s800), c(ramp.s300);
    syntax_link: c(BLUE_600), c(BLUE_300);
    syntax_code: c(TERRACOTTA_600), c(TERRACOTTA_300);
    syntax_marker: c(PLUM_600), c(PLUM_300);
    syntax_quote: c(OLIVE_600), c(OLIVE_300);
    syntax_meta: c(STONE_500), c(STONE_500);
    syntax_keyword: c(RED_600), c(RED_300);
    syntax_string: c(OLIVE_600), c(OLIVE_300);
    syntax_function: c(BLUE_600), c(BLUE_300);
    syntax_number: c(PLUM_600), c(PLUM_300);

    status_info: c(BLUE_600), c(BLUE_300);
    status_info_soft: c(BLUE_50), c(BLUE_950);
    status_success: c(OLIVE_600), c(OLIVE_300);
    status_success_soft: c(OLIVE_50), c(OLIVE_950);
    // A warning is yellow whatever the accent.
    status_warning: c(MUSTARD.s700), c(MUSTARD.s400);
    status_warning_soft: c(MUSTARD.s50), c(MUSTARD_950);
    status_danger: c(RED_600), c(RED_300);
    status_danger_soft: c(RED_50), c(RED_950);
}

// --------------------------------------------------------------------- sizes

/// The "Size" variable collection: spacing, radii, type and layout.
#[allow(dead_code)]
pub mod size {
    use gpui_kit::{Pixels, px};

    pub const RADIUS_XS: Pixels = px(4.);
    pub const RADIUS_SM: Pixels = px(6.);
    pub const RADIUS_MD: Pixels = px(8.);
    pub const RADIUS_LG: Pixels = px(12.);

    pub const FONT_UI: Pixels = px(13.);
    pub const FONT_UI_SMALL: Pixels = px(11.);
    pub const FONT_EDITOR: Pixels = px(13.);
    pub const FONT_BODY: Pixels = px(15.);
    pub const FONT_H1: Pixels = px(30.);
    pub const FONT_H2: Pixels = px(21.);
    pub const FONT_H3: Pixels = px(17.);

    pub const TITLEBAR: Pixels = px(44.);
    pub const SIDEBAR: Pixels = px(248.);
    pub const STATUSBAR: Pixels = px(28.);
}

// ------------------------------------------------------------------- install

struct Palettes {
    /// The accent the two palettes are built around.
    accent: Accent,
    light: Palette,
    dark: Palette,
    /// What is on screen now: one of the two, or a blend while fading to one of them.
    current: Palette,
    fade: Option<Fade>,
}

/// A switch of appearance or of accent in progress.
struct Fade {
    from: Palette,
    started: Instant,
}

/// How long switching between light and dark, or to another accent, takes.
const FADE: Duration = Duration::from_millis(220);

impl Global for Palettes {}

/// Reach the palette for the current appearance from anywhere there is an `App`.
pub trait ActivePalette {
    fn palette(&self) -> &Palette;
}

impl ActivePalette for App {
    fn palette(&self) -> &Palette {
        &self.global::<Palettes>().current
    }
}

/// A colour between two others, mixed channel by channel so it never takes a
/// detour through other hues.
pub fn blend(from: Hsla, to: Hsla, progress: f32) -> Hsla {
    let (from, to) = (Rgba::from(from), Rgba::from(to));
    let mix = |from: f32, to: f32| from + (to - from) * progress;
    Rgba {
        r: mix(from.r, to.r),
        g: mix(from.g, to.g),
        b: mix(from.b, to.b),
        a: mix(from.a, to.a),
    }
    .into()
}

/// The interface font, by the name the OS itself uses for it.
///
/// Naming fonts matters for startup: left on its defaults, the component kit
/// lists every installed font to check its default exists, which costs about
/// 70 ms. A theme that names its fonts skips that check.
#[cfg(target_os = "macos")]
const UI_FONT: Option<&str> = Some(".AppleSystemUIFont");
#[cfg(not(target_os = "macos"))]
const UI_FONT: Option<&str> = None;

/// Register both appearances. Call once, *before* `gpui_kit::init`, so the kit
/// starts from our theme instead of loading its own first.
pub fn install(settings: &Settings, cx: &mut App) {
    settings.font.load(cx);
    let light = Palette::light(settings.accent);
    let dark = Palette::dark(settings.accent);
    let mut theme = Theme::default();
    theme.light_theme = Rc::new(kit_config(&light, ThemeMode::Light, settings));
    theme.dark_theme = Rc::new(kit_config(&dark, ThemeMode::Dark, settings));
    cx.set_global(theme);
    cx.set_global(Palettes {
        accent: settings.accent,
        light,
        dark,
        current: light,
        fade: None,
    });
}

/// The accent colour in use.
pub fn accent(cx: &App) -> Accent {
    cx.global::<Palettes>().accent
}

/// The palette the app is in, or is fading to. What is drawn once and kept,
/// as a diagram's picture is, is drawn with this one: `palette()` may be
/// partway between two.
pub fn settled(cx: &App) -> Palette {
    let palettes = cx.global::<Palettes>();
    if cx.theme().is_dark() {
        palettes.dark
    } else {
        palettes.light
    }
}

/// Use another accent colour, in every open window, fading over to it.
pub fn set_accent(settings: &Settings, cx: &mut App) {
    let accent = settings.accent;
    let fade = !cx.reduce_motion();
    let mode = if cx.theme().is_dark() {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    let (light, dark) = (Palette::light(accent), Palette::dark(accent));
    let palettes = cx.global_mut::<Palettes>();
    palettes.accent = accent;
    palettes.light = light;
    palettes.dark = dark;
    palettes.fade = fade.then(|| Fade {
        from: palettes.current,
        started: Instant::now(),
    });
    if !fade {
        palettes.current = if mode.is_dark() { dark } else { light };
    }
    Theme::update(cx, |theme| {
        theme.light_theme = Rc::new(kit_config(&light, ThemeMode::Light, settings));
        theme.dark_theme = Rc::new(kit_config(&dark, ThemeMode::Dark, settings));
    });
    // Asking for the appearance again puts the theme just registered for it to use.
    Theme::change(mode, None, cx);
}

/// Use another editor font or size, in every open window, at once.
pub fn set_editor_font(settings: &Settings, cx: &mut App) {
    settings.font.load(cx);
    let (light, dark) = {
        let palettes = cx.global::<Palettes>();
        (palettes.light, palettes.dark)
    };
    let family = SharedString::from(settings.font.family());
    let size = px(settings.font_size);
    Theme::update(cx, |theme| {
        // Both appearances carry the font, so switching appearance keeps it.
        theme.light_theme = Rc::new(kit_config(&light, ThemeMode::Light, settings));
        theme.dark_theme = Rc::new(kit_config(&dark, ThemeMode::Dark, settings));
        theme.mono_font_family = family;
        theme.mono_font_size = size;
    });
}

/// Switch to the appearance the system is using. Call after `gpui_kit::init`,
/// which always starts in light.
pub fn follow_system(cx: &mut App) {
    let mode = ThemeMode::from(cx.window_appearance());
    if mode.is_dark() {
        set_mode(mode, false, cx);
    }
}

/// Flip between light and dark, fading from one to the other.
pub fn toggle(cx: &mut App) {
    let mode = if cx.theme().is_dark() {
        ThemeMode::Light
    } else {
        ThemeMode::Dark
    };
    set_mode(mode, true, cx);
}

/// Switch appearance. With `fade`, our own surfaces blend over to the new
/// colours; text switches at once, which keeps it readable throughout.
pub fn set_mode(mode: ThemeMode, fade: bool, cx: &mut App) {
    let fade = fade && !cx.reduce_motion();
    let palettes = cx.global_mut::<Palettes>();
    let target = if mode.is_dark() {
        palettes.dark
    } else {
        palettes.light
    };
    palettes.fade = fade.then(|| Fade {
        from: palettes.current,
        started: Instant::now(),
    });
    if !fade {
        palettes.current = target;
    }
    Theme::change(mode, None, cx);
}

/// Move a fade to another appearance or accent one frame on. Call once per frame, before
/// anything reads the palette.
pub fn tick(window: &mut Window, cx: &mut App) {
    let dark = cx.theme().is_dark();
    let palettes = cx.global_mut::<Palettes>();
    let Some(fade) = &palettes.fade else { return };
    let target = if dark { palettes.dark } else { palettes.light };
    let progress = fade.started.elapsed().as_secs_f32() / FADE.as_secs_f32();
    if progress >= 1. {
        palettes.current = target;
        palettes.fade = None;
    } else {
        palettes.current = Palette::between(&fade.from, &target, crate::motion::ease(progress));
        window.request_animation_frame();
    }
}

fn hex(color: Hsla) -> String {
    let Rgba { r, g, b, a } = color.into();
    let byte = |v: f32| (v.clamp(0., 1.) * 255.).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}{:02x}",
        byte(r),
        byte(g),
        byte(b),
        byte(a)
    )
}

/// The palette as the component kit understands it.
fn kit_config(p: &Palette, mode: ThemeMode, settings: &Settings) -> ThemeConfig {
    let dark = mode.is_dark();
    // The current line in the editor: one small step away from the editor surface.
    // A veil rather than a colour, so it sits right on the editor surface
    // whatever that is, including midway through a fade between appearances.
    let active_line = if dark {
        a(0xFFFFFF, 0.035)
    } else {
        a(0x000000, 0.035)
    };
    let clear = hex(a(0x000000, 0.));
    let style = |color: Hsla| serde_json::json!({ "color": hex(color) });
    let italic = |color: Hsla| serde_json::json!({ "color": hex(color), "font_style": "italic" });
    let bold = |color: Hsla| serde_json::json!({ "color": hex(color), "font_weight": 700 });

    let config = serde_json::json!({
        "name": if dark { "ZinMD Dark" } else { "ZinMD Light" },
        "mode": if dark { "dark" } else { "light" },
        "font.family": UI_FONT,
        "font.size": 13,
        "mono_font.family": settings.font.family(),
        "mono_font.size": settings.font_size,
        "radius": 6,
        "radius.lg": 8,
        "colors": {
            "background": hex(p.surface_preview),
            "foreground": hex(p.text_primary),
            "border": hex(p.border_raised),
            "input.border": hex(p.border_subtle),
            "ring": hex(p.accent_border),
            "caret": hex(p.accent),
            "selection.background": hex(p.accent_muted),
            "drag_border": hex(p.accent),
            "drop_target.background": hex(p.accent_soft),
            "overlay": hex(a(0x000000, if dark { 0.4 } else { 0.12 })),
            "window.border": hex(p.border_subtle),

            "muted.background": hex(p.surface_sunken),
            "muted.foreground": hex(p.text_secondary),
            // What the kit highlights the row under the pointer with, in menus and
            // lists. Those sit on raised surfaces, where our plain hover colour
            // is invisible in the dark appearance; the accent's tint shows on both.
            "accent.background": hex(p.surface_selected),
            "accent.foreground": hex(p.text_primary),
            "primary.background": hex(p.accent),
            "primary.hover.background": hex(p.accent_hover),
            "primary.active.background": hex(p.accent_hover),
            "primary.foreground": hex(p.text_on_accent),
            "secondary.background": hex(p.surface_hover),
            "secondary.hover.background": hex(p.surface_hover),
            "secondary.active.background": hex(p.border_strong),
            "secondary.foreground": hex(p.text_primary),

            "popover.background": hex(p.surface_raised),
            "popover.foreground": hex(p.text_primary),
            // Links are text: they take the accent's text shade, which reads
            // on the page, not the fill, which is made to carry dark text.
            "link": hex(p.accent_text),
            "link.hover": hex(p.accent_text),
            "link.active": hex(p.accent_text),

            "list.background": hex(p.surface_raised),
            "list.hover.background": hex(p.surface_hover),
            "list.active.background": hex(p.surface_selected),
            "list.active.border": hex(p.accent_border),
            "list.even.background": hex(p.surface_raised),
            "list.head.background": hex(p.surface_sunken),

            "table.background": hex(p.surface_preview),
            "table.hover.background": hex(p.surface_preview),
            "table.even.background": hex(p.surface_preview),
            "table.active.background": hex(p.surface_selected),
            "table.active.border": hex(p.accent_border),
            "table.head.background": hex(p.surface_sunken),
            "table.head.foreground": hex(p.text_secondary),
            "table.row.border": hex(p.border_subtle),

            "scrollbar.background": clear,
            "scrollbar.thumb.background": hex(p.border_strong),
            "scrollbar.thumb.hover.background": hex(p.text_tertiary),

            "sidebar.background": hex(p.surface_app),
            "sidebar.foreground": hex(p.text_primary),
            "sidebar.border": hex(p.border_subtle),
            "sidebar.accent.background": hex(p.surface_hover),
            "sidebar.accent.foreground": hex(p.text_primary),
            "sidebar.primary.background": hex(p.accent),
            "sidebar.primary.foreground": hex(p.text_on_accent),
            "title_bar.background": hex(p.surface_app),
            "title_bar.border": hex(p.border_subtle),
            "status_bar.background": hex(p.surface_app),
            "status_bar.border": hex(p.border_subtle),
            "tab_bar.background": hex(p.surface_app),
            "tab.active.background": hex(p.surface_raised),
            "tab.active.foreground": hex(p.text_primary),
            "tab.foreground": hex(p.text_secondary),

            "info.background": hex(p.status_info),
            "info.foreground": hex(p.surface_preview),
            "success.background": hex(p.status_success),
            "success.foreground": hex(p.surface_preview),
            "warning.background": hex(p.status_warning),
            "warning.foreground": hex(p.surface_preview),
            "danger.background": hex(p.status_danger),
            "danger.foreground": hex(p.surface_preview),

            "base.red": hex(p.status_danger),
            "base.green": hex(p.status_success),
            "base.blue": hex(p.status_info),
            "base.yellow": hex(p.status_warning),
            "base.magenta": hex(p.syntax_number),
            "base.cyan": hex(p.syntax_link),
        },
        "highlight": {
            "editor.foreground": hex(p.text_primary),
            // The pane behind the editor paints the surface, so it can fade.
            "editor.background": clear,
            // Lines wrap rather than scroll sideways, so nothing runs under the
            // line numbers and they need no backing of their own.
            "editor.gutter.background": clear,
            "editor.active_line.background": hex(active_line),
            "editor.line_number": hex(p.text_disabled),
            "editor.active_line_number": hex(p.text_primary),
            "editor.invisible": hex(p.text_disabled),
            "syntax": {
                "title": bold(p.syntax_heading),
                "emphasis": { "font_style": "italic" },
                "emphasis.strong": { "font_weight": 700 },
                "link_text": style(p.syntax_link),
                "link_uri": style(p.syntax_meta),
                "text.literal": style(p.syntax_code),
                "text.code.span": style(p.syntax_code),
                "punctuation": style(p.syntax_meta),
                "punctuation.bracket": style(p.syntax_meta),
                "punctuation.delimiter": style(p.syntax_meta),
                "punctuation.special": bold(p.syntax_heading),
                "punctuation.list_marker": style(p.syntax_marker),
                "comment": italic(p.syntax_meta),
                "comment.doc": italic(p.syntax_meta),
                "keyword": style(p.syntax_keyword),
                "boolean": style(p.syntax_number),
                "number": style(p.syntax_number),
                "constant": style(p.syntax_number),
                "string": style(p.syntax_string),
                "string.escape": style(p.syntax_code),
                "string.special": style(p.syntax_code),
                "function": style(p.syntax_function),
                "constructor": style(p.syntax_function),
                "type": style(p.syntax_number),
                "enum": style(p.syntax_number),
                "variant": style(p.syntax_number),
                "property": style(p.syntax_link),
                "attribute": style(p.syntax_marker),
                "tag": style(p.syntax_keyword),
                "label": style(p.syntax_marker),
                "operator": style(p.text_secondary),
                "variable": style(p.text_primary),
                "variable.special": style(p.syntax_keyword),
                "embedded": style(p.text_primary),
                "preproc": style(p.syntax_marker),
            },
        },
    });
    serde_json::from_value(config).expect("the built-in theme is valid")
}

#[cfg(test)]
mod tests {
    use super::{Accent, Palette};
    use gpui_kit::{Hsla, Rgba};

    /// How strongly two colours contrast, as WCAG measures it: 1 to 21.
    fn contrast(a: Hsla, b: Hsla) -> f32 {
        let luminance = |color: Hsla| {
            let Rgba { r, g, b, .. } = color.into();
            let linear = |v: f32| {
                if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
        };
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    /// Whatever the accent, in light and in dark: the words on a filled
    /// button read, and so do links and headings on every surface they sit on.
    #[test]
    fn text_reads_on_every_accent() {
        for accent in Accent::ALL {
            for (look, p) in [
                ("light", Palette::light(accent)),
                ("dark", Palette::dark(accent)),
            ] {
                let name = format!("{} in {look}", accent.label());
                assert!(
                    contrast(p.text_on_accent, p.accent) >= 4.5,
                    "{name}: on a button"
                );
                assert!(
                    contrast(p.text_on_accent, p.accent_hover) >= 4.5,
                    "{name}: on a hovered button"
                );
                for surface in [p.surface_preview, p.surface_editor, p.surface_app] {
                    assert!(
                        contrast(p.accent_text, surface) >= 4.5,
                        "{name}: accent text"
                    );
                    assert!(
                        contrast(p.syntax_heading, surface) >= 4.5,
                        "{name}: a heading"
                    );
                }
            }
        }
    }

    #[test]
    fn every_accent_is_its_own() {
        for (ix, accent) in Accent::ALL.into_iter().enumerate() {
            for other in &Accent::ALL[ix + 1..] {
                assert_ne!(accent.label(), other.label());
                assert_ne!(accent.swatch(false), other.swatch(false));
                assert_ne!(accent.swatch(true), other.swatch(true));
            }
            // The circle shows the very colour the app fills with.
            assert_eq!(accent.swatch(false), Palette::light(accent).accent);
            assert_eq!(accent.swatch(true), Palette::dark(accent).accent);
        }
        // Mustard is where the app starts, and a warning stays yellow under any accent.
        assert_eq!(Accent::default(), Accent::Mustard);
        assert_eq!(
            Palette::light(Accent::Sky).status_warning,
            Palette::light(Accent::Mustard).status_warning
        );
    }
}
