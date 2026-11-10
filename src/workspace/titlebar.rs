//! The title bar: sidebar toggle and tabs on the left; view switch, search,
//! appearance and export on the right.

use std::time::Instant;

use gpui_kit::assets::IconName;
use gpui_kit::base::transition;
use gpui_kit::component::{ActiveTheme as _, Icon, TitleBar, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::hover::Hot;
use super::{ViewMode, Workspace};
use crate::motion;
use crate::theme::{self, ActivePalette as _, Palette, size};
use crate::ui::icon_button;

/// Room for the macOS window buttons.
const TRAFFIC_LIGHTS: Pixels = px(78.);
const TAB_HEIGHT: Pixels = px(28.);
const TAB_GAP: Pixels = px(4.);
/// Roughly everything in the title bar that is not a tab: what the tabs have
/// to leave room for. Past that, they all narrow together.
const OTHER_CHROME: Pixels = px(650.);

/// A closed tab, still shrinking out of the strip.
pub(super) struct GhostTab {
    /// The position in the strip it was closed at.
    pub at: usize,
    pub name: SharedString,
    pub width: Pixels,
    pub closed: Instant,
}

/// Where one tab sits in the strip on this frame.
struct TabSlot {
    x: Pixels,
    /// Its width once fully there.
    width: Pixels,
    /// How much of it is there: grows from 0 when opened, shrinks to 0 when closed.
    presence: f32,
    /// Index of the open document, or of the ghost, it shows.
    doc: Option<usize>,
    ghost: Option<usize>,
}

/// How wide a tab is for a file name of the given width.
pub(super) fn tab_width(name: Pixels) -> Pixels {
    // Icon, close button, gaps and padding around the name.
    (name + px(62.)).clamp(px(96.), px(240.))
}

impl Workspace {
    pub(super) fn render_titlebar(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let p = *cx.palette();
        let tabs = self.render_tabs(&p, window, cx);
        let appearance_icon = if cx.theme().is_dark() {
            IconName::Sun
        } else {
            IconName::Moon
        };
        let fill = p.surface_hover;
        let button = |this: &Self,
                      id: &'static str,
                      icon: IconName,
                      window: &mut Window,
                      cx: &mut Context<Self>| {
            this.hoverable(icon_button(id, icon, &p), Hot::Button(id), fill, window, cx)
        };

        let left = h_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .gap(TAB_GAP)
            .child(
                button(self, "toggle-sidebar", IconName::PanelLeft, window, cx)
                    .mr(px(6.))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_sidebar(cx))),
            )
            .child(tabs)
            .child(
                button(self, "new-tab", IconName::Plus, window, cx)
                    .on_click(cx.listener(|this, _, window, cx| this.open_untitled(window, cx))),
            );

        let right = h_flex()
            .flex_none()
            .h_full()
            .gap(px(4.))
            .pl(px(16.))
            .pr(px(12.))
            .child(self.render_view_switch(&p, window, cx))
            .child(
                button(self, "search", IconName::Search, window, cx)
                    .ml(px(8.))
                    .on_click(cx.listener(|this, _, window, cx| this.open_palette(window, cx))),
            )
            .child(
                button(self, "appearance", appearance_icon, window, cx)
                    .on_click(|_, _, cx| theme::toggle(cx)),
            )
            .child(self.render_export_button(&p, window, cx));

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

    /// Place every tab for this frame. A tab that just opened is still growing
    /// in and one that just closed still shrinking out, and the tabs after
    /// them slide along. Returns the slots, the width of the whole strip, and
    /// where the active tab comes to rest.
    fn lay_out_tabs(
        &mut self,
        window: &mut Window,
        cx: &App,
    ) -> (Vec<TabSlot>, Pixels, Option<(Pixels, Pixels)>) {
        self.ghost_tabs
            .retain(|ghost| motion::progress(ghost.closed, motion::QUICK, cx) < 1.);

        let natural = self
            .docs
            .iter()
            .fold(px(0.), |sum, doc| sum + doc.tab_width + TAB_GAP);
        let room = (window.viewport_size().width - OTHER_CHROME).max(px(120.));
        let squeeze = (room / natural).min(1.);

        let mut slots = Vec::with_capacity(self.docs.len() + self.ghost_tabs.len());
        let (mut x, mut resting_x) = (px(0.), px(0.));
        let mut active = None;
        let mut moving = false;
        for ix in 0..=self.docs.len() {
            for (ghost_ix, ghost) in self.ghost_tabs.iter().enumerate() {
                if ghost.at.min(self.docs.len()) == ix {
                    let presence = 1. - motion::progress(ghost.closed, motion::QUICK, cx);
                    let width = ghost.width * squeeze;
                    slots.push(TabSlot {
                        x,
                        width,
                        presence,
                        doc: None,
                        ghost: Some(ghost_ix),
                    });
                    x += (width + TAB_GAP) * presence;
                    moving = true;
                }
            }
            let Some(doc) = self.docs.get(ix) else { break };
            let presence = doc
                .born
                .map_or(1., |born| motion::progress(born, motion::QUICK, cx));
            let width = doc.tab_width * squeeze;
            if ix == self.active {
                active = Some((resting_x, width));
            }
            slots.push(TabSlot {
                x,
                width,
                presence,
                doc: Some(ix),
                ghost: None,
            });
            x += (width + TAB_GAP) * presence;
            resting_x += width + TAB_GAP;
            moving |= presence < 1.;
        }
        if moving {
            window.request_animation_frame();
        }
        (slots, (x - TAB_GAP).max(px(0.)), active)
    }

    fn render_tabs(
        &mut self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let (slots, strip_width, active) = self.lay_out_tabs(window, cx);

        // The raised background of the active tab is one piece that slides to
        // whichever tab is active, rather than each tab switching its own on.
        let highlight = active.map(|(x, width)| {
            let x = transition(
                "tab-highlight-x",
                x,
                motion::glide(motion::QUICK, cx),
                window,
                cx,
            );
            let width = transition(
                "tab-highlight-width",
                width,
                motion::glide(motion::QUICK, cx),
                window,
                cx,
            );
            div()
                .absolute()
                .top_0()
                .left(x)
                .w(width)
                .h(TAB_HEIGHT)
                .rounded(size::RADIUS_SM)
                .border_1()
                .border_color(p.border_subtle)
                .bg(p.surface_raised)
        });

        let mut tabs = Vec::with_capacity(slots.len());
        for slot in &slots {
            let tab = match (slot.doc, slot.ghost) {
                (Some(ix), _) => self
                    .render_tab(ix, slot.width, p, window, cx)
                    .into_any_element(),
                (_, Some(ix)) => ghost_tab(&self.ghost_tabs[ix], slot.width, p).into_any_element(),
                _ => continue,
            };
            // The tab keeps its full width inside a window that opens or closes
            // over it, so its label never re-flows while it moves.
            tabs.push(
                div()
                    .absolute()
                    .top_0()
                    .left(slot.x)
                    .w(slot.width * slot.presence)
                    .h(TAB_HEIGHT)
                    .overflow_hidden()
                    .opacity(slot.presence)
                    .child(tab),
            );
        }

        div()
            .relative()
            .flex_none()
            .w(strip_width)
            .h(TAB_HEIGHT)
            .children(highlight)
            .children(tabs)
    }

    fn render_tab(
        &self,
        ix: usize,
        width: Pixels,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
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

        let tab = h_flex()
            .id(("tab", doc.id))
            .group("tab")
            .w(width)
            .h(TAB_HEIGHT)
            .pl(px(10.))
            .pr(px(6.))
            .gap(px(6.))
            .rounded(size::RADIUS_SM)
            .map(|tab| {
                if active {
                    tab.font_weight(FontWeight::MEDIUM)
                } else {
                    tab.text_color(p.text_secondary)
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
            .on_click(cx.listener(move |this, _, window, cx| this.activate(ix, window, cx)));
        // The active tab already stands out; only the others answer the pointer.
        if active {
            tab
        } else {
            self.hoverable(tab, Hot::Tab(doc.id), hover, window, cx)
        }
    }

    /// Editor / Split / Preview. The yellow pill slides to the chosen view.
    fn render_view_switch(
        &self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        /// Every segment is this wide, so the pill only ever has to move.
        const SEGMENT: Pixels = px(86.);
        let modes = [
            (ViewMode::Editor, "Editor", IconName::Pencil),
            (ViewMode::Split, "Split", IconName::Columns2),
            (ViewMode::Preview, "Preview", IconName::Eye),
        ];
        let chosen = modes
            .iter()
            .position(|(mode, ..)| *mode == self.view)
            .unwrap_or(0);
        let pill_x = transition(
            "view-pill",
            SEGMENT * chosen as f32,
            motion::glide(motion::QUICK, cx),
            window,
            cx,
        );

        let items = modes.into_iter().map(|(mode, label, icon)| {
            let strong = p.text_primary;
            h_flex()
                .id(label)
                .w(SEGMENT)
                .h_full()
                .justify_center()
                .gap(px(6.))
                .map(|item| {
                    if self.view == mode {
                        item.text_color(p.text_on_accent)
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
        });

        div()
            .flex_none()
            .h(px(28.))
            .p(px(1.))
            .rounded(size::RADIUS_SM)
            .border_1()
            .border_color(p.border_subtle)
            .child(
                h_flex()
                    .relative()
                    .h_full()
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .left(pill_x)
                            .w(SEGMENT)
                            .rounded(px(5.))
                            .bg(p.accent),
                    )
                    .children(items),
            )
    }

    fn render_export_button(
        &self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let label = h_flex()
            .id("export")
            .h_full()
            .px(px(10.))
            .gap(px(6.))
            .child(Icon::new(IconName::Share).size(px(14.)))
            .child("Export")
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, window, cx| this.export_html(window, cx)));
        div()
            .flex_none()
            .h(px(28.))
            .ml(px(8.))
            .rounded(size::RADIUS_SM)
            .border_1()
            .border_color(p.border_subtle)
            .bg(p.surface_raised)
            .font_weight(FontWeight::MEDIUM)
            .overflow_hidden()
            .child(self.hoverable(label, Hot::Button("export"), p.surface_hover, window, cx))
    }
}

/// A closed tab on its way out: it looks like the tab did, and does nothing.
fn ghost_tab(ghost: &GhostTab, width: Pixels, p: &Palette) -> Div {
    h_flex()
        .w(width)
        .h(TAB_HEIGHT)
        .pl(px(10.))
        .pr(px(6.))
        .gap(px(6.))
        .text_color(p.text_secondary)
        .child(
            Icon::new(IconName::FileText)
                .size(px(14.))
                .text_color(p.text_tertiary),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(ghost.name.clone()),
        )
}
