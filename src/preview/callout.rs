//! Callouts: a blockquote that opens with a marker line such as `> [!TIP]`.
//!
//! The five GitHub alert names are here, plus the common Obsidian ones mapped
//! onto the same looks. Each is a tinted box with an icon and a title; the
//! quoted blocks stay ordinary document text inside it.

use gpui_kit::assets::IconName;
use gpui_kit::component::text::MarkdownExtensions;
use gpui_kit::component::{Icon, h_flex};
use gpui_kit::*;

use crate::theme::{ActivePalette as _, Palette, size};

#[derive(Clone, Copy)]
enum Tone {
    Info,
    Accent,
    Success,
    Warning,
    Danger,
    Neutral,
}

struct Kind {
    /// What may follow `[!` to open this callout. Matching ignores case.
    names: &'static [&'static str],
    title: &'static str,
    icon: fn() -> IconName,
    tone: Tone,
}

const KINDS: &[Kind] = &[
    Kind {
        names: &["note", "info", "abstract", "summary", "todo"],
        title: "Note",
        icon: || IconName::Info,
        tone: Tone::Info,
    },
    Kind {
        names: &["tip", "hint"],
        title: "Tip",
        icon: || IconName::Zap,
        tone: Tone::Accent,
    },
    Kind {
        names: &["important", "example"],
        title: "Important",
        icon: || IconName::MessageSquareWarning,
        tone: Tone::Accent,
    },
    Kind {
        names: &["success", "check", "done"],
        title: "Success",
        icon: || IconName::CircleCheck,
        tone: Tone::Success,
    },
    Kind {
        names: &["warning", "attention"],
        title: "Warning",
        icon: || IconName::TriangleAlert,
        tone: Tone::Warning,
    },
    Kind {
        names: &["caution", "danger", "error", "failure", "bug"],
        title: "Caution",
        icon: || IconName::OctagonAlert,
        tone: Tone::Danger,
    },
    Kind {
        names: &["quote", "cite"],
        title: "Quote",
        icon: || IconName::Quote,
        tone: Tone::Neutral,
    },
];

/// Teach `extensions` every callout name.
pub fn register(mut extensions: MarkdownExtensions) -> MarkdownExtensions {
    for kind in KINDS {
        for name in kind.names {
            extensions = extensions.block_surface(*name, move |_, cx| surface(kind, cx.palette()));
        }
    }
    extensions
}

/// The box a callout's blocks are placed in, with its title row already inside.
fn surface(kind: &Kind, p: &Palette) -> Div {
    // The marker colour, the tint behind the text, and the title colour.
    let (bar, tint, title) = match kind.tone {
        Tone::Info => (p.status_info, p.status_info_soft, p.status_info),
        Tone::Accent => (p.accent, p.accent_soft, p.accent_text),
        Tone::Success => (p.status_success, p.status_success_soft, p.status_success),
        Tone::Warning => (p.status_warning, p.status_warning_soft, p.status_warning),
        Tone::Danger => (p.status_danger, p.status_danger_soft, p.status_danger),
        Tone::Neutral => (p.border_strong, p.surface_sunken, p.text_secondary),
    };

    div()
        .relative()
        .w_full()
        .pl(px(26.))
        .pr(px(16.))
        .py(px(12.))
        .rounded(size::RADIUS_MD)
        .bg(tint)
        .child(
            div()
                .absolute()
                .left(px(12.))
                .top(px(12.))
                .bottom(px(12.))
                .w(px(3.))
                .rounded_full()
                .bg(bar),
        )
        .child(
            h_flex()
                .gap(px(6.))
                .text_size(size::FONT_UI)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(title)
                .child(Icon::new((kind.icon)()).size(px(14.)))
                .child(kind.title),
        )
}
