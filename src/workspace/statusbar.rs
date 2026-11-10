//! The status bar: save state on the left, caret and document facts on the right.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::DropdownMenu as _;
use gpui_kit::component::{Icon, Sizable as _, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{UseFiraCode, UseJetBrainsMono, UseMenlo, Workspace};
use crate::settings::{EditorFont, Settings};
use crate::theme::{ActivePalette as _, Palette, size};

impl Workspace {
    /// The editor font and its size: click the name to choose another font,
    /// the minus and plus to shrink or grow the text.
    fn render_font_control(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let settings = *cx.global::<Settings>();
        let (hover, strong) = (p.surface_hover, p.text_primary);
        let step = |id: &'static str, icon: IconName| {
            div()
                .id(id)
                .flex()
                .items_center()
                .justify_center()
                .size(px(18.))
                .rounded(size::RADIUS_XS)
                .hover(move |style| style.bg(hover).text_color(strong))
                .child(Icon::new(icon).size(px(11.)))
        };

        let font = Button::new("editor-font")
            .ghost()
            .xsmall()
            .label(settings.font.label())
            .dropdown_menu_with_anchor(Anchor::BottomLeft, move |mut menu, _, cx| {
                for font in EditorFont::ALL {
                    // Each name is shown in its own font, so all three are needed now.
                    font.load(cx);
                    let action: Box<dyn Action> = match font {
                        EditorFont::JetBrainsMono => Box::new(UseJetBrainsMono),
                        EditorFont::FiraCode => Box::new(UseFiraCode),
                        EditorFont::Menlo => Box::new(UseMenlo),
                    };
                    menu =
                        menu.menu_element_with_check(font == settings.font, action, move |_, _| {
                            div().font_family(font.family()).child(font.label())
                        });
                }
                menu
            });

        h_flex()
            .gap(px(2.))
            .child(font)
            .child(
                step("font-smaller", IconName::Minus).on_click(cx.listener(|this, _, _, cx| {
                    this.update_settings(|settings| settings.font_size -= 1., cx)
                })),
            )
            .child(
                div()
                    .w(px(18.))
                    .text_center()
                    .child(format!("{}", settings.font_size)),
            )
            .child(
                step("font-bigger", IconName::Plus).on_click(cx.listener(|this, _, _, cx| {
                    this.update_settings(|settings| settings.font_size += 1., cx)
                })),
            )
    }

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
        let Some(doc) = self.active_doc() else {
            return bar;
        };

        let (dot, state) = if doc.dirty() {
            (p.accent, "Edited")
        } else {
            (p.status_success, "Saved")
        };
        let (sync_on, strong) = (self.sync_enabled, p.text_primary);
        let caret = doc.editor.read(cx).cursor_position();
        let words = doc.stats.words;

        let left = h_flex()
            .gap(px(16.))
            .child(
                h_flex()
                    .gap(px(6.))
                    .child(div().size(px(6.)).rounded_full().bg(dot))
                    .child(state),
            )
            .child(
                h_flex()
                    .id("scroll-sync")
                    .gap(px(5.))
                    .when(!sync_on, |item| item.text_color(p.text_disabled))
                    .hover(move |style| style.text_color(strong))
                    .child(Icon::new(IconName::Link2).size(px(13.)))
                    .child(if sync_on {
                        "Scroll sync"
                    } else {
                        "Scroll sync off"
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_scroll_sync(cx))),
            );

        let right = h_flex()
            .gap(px(18.))
            .child(format!(
                "Ln {}, Col {}",
                caret.line + 1,
                caret.character + 1
            ))
            .child(format!(
                "{words} {}",
                if words == 1 { "word" } else { "words" }
            ))
            .child(format!("{} min read", doc.stats.minutes()))
            .child("Markdown")
            .child("UTF-8")
            .child(self.render_font_control(&p, cx))
            .children(self.opened_ms.map(|ms| {
                h_flex()
                    .gap(px(4.))
                    .text_color(p.text_primary)
                    .font_weight(FontWeight::MEDIUM)
                    .child(
                        Icon::new(IconName::Zap)
                            .size(px(12.))
                            .text_color(p.accent_hover),
                    )
                    .child(format!("Opened in {ms:.0} ms"))
            }));

        bar.child(left).child(right)
    }
}
