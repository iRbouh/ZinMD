//! The status bar: save state on the left, caret and document facts on the right.

use gpui_kit::assets::IconName;
use gpui_kit::component::{Icon, h_flex};
use gpui_kit::*;

use super::Workspace;
use crate::theme::{ActivePalette as _, size};

impl Workspace {
    pub(super) fn render_statusbar(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = *cx.palette();
        let bar = h_flex()
            .flex_none()
            .justify_between()
            .h(size::STATUSBAR)
            .px(px(12.))
            .border_t_1()
            .border_color(p.border_subtle)
            .text_size(px(11.5))
            .text_color(p.text_secondary);
        let Some(doc) = self.active_doc() else { return bar };

        let (dot, state) = if doc.dirty { (p.accent, "Edited") } else { (p.status_success, "Saved") };
        let caret = doc.editor.read(cx).cursor_position();
        let words = doc.stats.words;

        let left = h_flex()
            .gap(px(16.))
            .child(h_flex().gap(px(6.)).child(div().size(px(6.)).rounded_full().bg(dot)).child(state))
            .child(h_flex().gap(px(5.)).child(Icon::new(IconName::Link2).size(px(13.))).child("Scroll sync"));

        let right = h_flex()
            .gap(px(18.))
            .child(format!("Ln {}, Col {}", caret.line + 1, caret.character + 1))
            .child(format!("{words} {}", if words == 1 { "word" } else { "words" }))
            .child(format!("{} min read", doc.stats.minutes()))
            .child("Markdown")
            .child("UTF-8")
            .children(self.opened_ms.map(|ms| {
                h_flex()
                    .gap(px(4.))
                    .text_color(p.text_primary)
                    .font_weight(FontWeight::MEDIUM)
                    .child(Icon::new(IconName::Zap).size(px(12.)).text_color(p.accent_hover))
                    .child(format!("Opened in {ms:.0} ms"))
            }));

        bar.child(left).child(right)
    }
}
