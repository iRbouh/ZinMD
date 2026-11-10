//! The title bar: sidebar toggle and tabs on the left; view switch, search,
//! appearance and export on the right.

use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme as _, Icon, TitleBar, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{ViewMode, Workspace};
use crate::theme::{self, ActivePalette as _, Palette, size};
use crate::ui::icon_button;

/// Room for the macOS window buttons.
const TRAFFIC_LIGHTS: Pixels = px(78.);

impl Workspace {
    pub(super) fn render_titlebar(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let p = *cx.palette();
        let mut tabs = Vec::with_capacity(self.docs.len());
        for ix in 0..self.docs.len() {
            tabs.push(self.render_tab(ix, &p, cx));
        }
        let appearance_icon = if cx.theme().is_dark() {
            IconName::Sun
        } else {
            IconName::Moon
        };

        let left = h_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .gap(px(4.))
            .child(
                icon_button("toggle-sidebar", IconName::PanelLeft, &p)
                    .mr(px(6.))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_sidebar(cx))),
            )
            .children(tabs)
            .child(
                icon_button("new-tab", IconName::Plus, &p).on_click(cx.listener(
                    |this, _, window, cx| {
                        this.open_untitled(window, cx);
                    },
                )),
            );

        let right = h_flex()
            .flex_none()
            .h_full()
            .gap(px(4.))
            .pl(px(16.))
            .pr(px(12.))
            .child(self.render_view_switch(&p, cx))
            .child(
                icon_button("search", IconName::Search, &p)
                    .ml(px(8.))
                    .on_click(cx.listener(|this, _, window, cx| this.open_palette(window, cx))),
            )
            .child(
                icon_button("appearance", appearance_icon, &p)
                    .on_click(|_, _, cx| theme::toggle(cx)),
            )
            .child(
                self.render_export_button(&p)
                    .on_click(cx.listener(|this, _, window, cx| this.export_html(window, cx))),
            );

        TitleBar::new()
            .h(size::TITLEBAR)
            .pl(if window.is_fullscreen() {
                px(12.)
            } else {
                TRAFFIC_LIGHTS
            })
            .bg(p.surface_app)
            .border_color(p.border_subtle)
            .child(left)
            .child(right)
    }

    fn render_tab(
        &self,
        ix: usize,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let doc = &self.docs[ix];
        let active = ix == self.active;
        let (hover, strong) = (p.surface_hover, p.text_primary);

        // The dot that marks unsaved changes gives way to the close button on hover.
        let trailing = div()
            .relative()
            .flex_none()
            .size(px(16.))
            .when(doc.dirty(), |slot| {
                slot.child(
                    div()
                        .absolute()
                        .top(px(5.))
                        .left(px(5.))
                        .size(px(6.))
                        .rounded_full()
                        .bg(p.accent)
                        .group_hover("tab", |style| style.invisible()),
                )
            })
            .child(
                div()
                    .id("close")
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(size::RADIUS_XS)
                    .text_color(p.text_tertiary)
                    .invisible()
                    .group_hover("tab", |style| style.visible())
                    .hover(move |style| style.bg(hover).text_color(strong))
                    .child(Icon::new(IconName::X).size(px(12.)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.request_close(ix, window, cx);
                    })),
            );

        h_flex()
            .id(("tab", doc.id))
            .group("tab")
            .min_w(px(72.))
            .max_w(px(220.))
            .h(px(28.))
            .pl(px(10.))
            .pr(px(6.))
            .gap(px(6.))
            .rounded(size::RADIUS_SM)
            .border_1()
            .map(|tab| {
                if active {
                    tab.bg(p.surface_raised)
                        .border_color(p.border_subtle)
                        .font_weight(FontWeight::MEDIUM)
                } else {
                    tab.border_color(gpui_kit::transparent_black())
                        .text_color(p.text_secondary)
                        .hover(move |style| style.bg(hover))
                }
            })
            .child(
                Icon::new(IconName::FileText)
                    .size(px(14.))
                    .text_color(if active {
                        p.accent_hover
                    } else {
                        p.text_tertiary
                    }),
            )
            .child(div().flex_1().min_w_0().truncate().child(doc.name.clone()))
            .child(trailing)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, window, cx| this.activate(ix, window, cx)))
    }

    /// Editor / Split / Preview.
    fn render_view_switch(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let item = |mode: ViewMode, label: &'static str, icon: IconName| {
            let strong = p.text_primary;
            h_flex()
                .id(label)
                .h_full()
                .px(px(9.))
                .gap(px(6.))
                .rounded(px(5.))
                .map(|item| {
                    if self.view == mode {
                        item.bg(p.accent)
                            .text_color(p.text_on_accent)
                            .font_weight(FontWeight::SEMIBOLD)
                    } else {
                        item.text_color(p.text_secondary)
                            .hover(move |style| style.text_color(strong))
                    }
                })
                .child(Icon::new(icon).size(px(14.)))
                .child(label)
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, _, cx| this.set_view(mode, cx)))
        };

        h_flex()
            .flex_none()
            .h(px(28.))
            .p(px(1.))
            .rounded(size::RADIUS_SM)
            .border_1()
            .border_color(p.border_subtle)
            .child(item(ViewMode::Editor, "Editor", IconName::Pencil))
            .child(item(ViewMode::Split, "Split", IconName::Columns2))
            .child(item(ViewMode::Preview, "Preview", IconName::Eye))
    }

    fn render_export_button(&self, p: &Palette) -> Stateful<Div> {
        let hover = p.surface_hover;
        h_flex()
            .id("export")
            .flex_none()
            .h(px(28.))
            .ml(px(8.))
            .px(px(10.))
            .gap(px(6.))
            .rounded(size::RADIUS_SM)
            .border_1()
            .border_color(p.border_subtle)
            .bg(p.surface_raised)
            .font_weight(FontWeight::MEDIUM)
            .hover(move |style| style.bg(hover))
            .child(Icon::new(IconName::Share).size(px(14.)))
            .child("Export")
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
    }
}
