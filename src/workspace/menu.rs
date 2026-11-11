//! The bar: the few buttons that are always within reach, each kind in a
//! place of its own. Where you are is on the left, how you see it in the
//! middle, what you do with it on the right, and the way to the other mode in
//! the far corner. A page and the editor keep the same ends, so nothing changes
//! place when one becomes the other; on a page the buttons step aside while
//! you read.

use std::path::Path;
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::base::{Transition, transition};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, Icon, TitleBar, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::hover::Hot;
use super::panel::fading;
use super::{Doc, Mode, ViewMode, Workspace};
use crate::system::shortcut;
use crate::theme::{self, Accent, ActivePalette as _, Palette, size};
use crate::ui::icon_button;
use crate::{files, motion};

/// Room for the macOS window buttons, at the left end of the bar. Elsewhere
/// the window's buttons are at the right end, and that end makes room itself.
const TRAFFIC_LIGHTS: Pixels = if cfg!(target_os = "macos") {
    px(78.)
} else {
    EDGE
};
/// Room kept clear at the right end of the bar.
const EDGE: Pixels = px(12.);

/// Minimize, maximize and close, for the top of a page, on the systems that
/// leave them to the app: Windows always, Linux when the desktop puts no
/// frame of its own around the window. The Mac has its own, at the other end.
/// The editor's bar comes with these; the page has no bar, so they are here.
fn window_buttons(p: &Palette, window: &Window) -> Option<Div> {
    let ours = if cfg!(target_os = "macos") {
        false
    } else if cfg!(target_os = "windows") {
        !window.is_fullscreen()
    } else {
        matches!(window.window_decorations(), Decorations::Client { .. })
    };
    if !ours {
        return None;
    }

    let (quiet, strong, hover) = (p.text_secondary, p.text_primary, p.surface_hover);
    let (danger, on_danger) = (p.status_danger, p.surface_preview);
    let button = |id: &'static str, icon: IconName, area: WindowControlArea| {
        let closes = area == WindowControlArea::Close;
        div()
            .id(id)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .w(px(40.))
            .h_full()
            .text_color(quiet)
            .hover(move |style| match closes {
                true => style.bg(danger).text_color(on_danger),
                false => style.bg(hover).text_color(strong),
            })
            .child(Icon::new(icon).size(px(14.)))
            .map(|button| {
                // Windows takes the click itself once told what the spot is.
                if cfg!(target_os = "windows") {
                    return button.window_control_area(area);
                }
                button
                    .on_mouse_down(MouseButton::Left, |_, window, cx| {
                        window.prevent_default();
                        cx.stop_propagation();
                    })
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        match area {
                            WindowControlArea::Min => window.minimize_window(),
                            WindowControlArea::Max => window.zoom_window(),
                            // Asks about unsaved work first.
                            WindowControlArea::Close => {
                                window.dispatch_action(Box::new(super::Quit), cx)
                            }
                            WindowControlArea::Drag => {}
                        }
                    })
            })
    };

    // A desktop that tiles its windows may offer neither of the first two.
    let can = window.window_controls();
    let grow = match window.is_maximized() {
        true => IconName::WindowRestore,
        false => IconName::WindowMaximize,
    };
    Some(
        h_flex()
            .flex_none()
            .h_full()
            .when(can.minimize, |row| {
                row.child(button(
                    "window-minimize",
                    IconName::WindowMinimize,
                    WindowControlArea::Min,
                ))
            })
            .when(can.maximize, |row| {
                row.child(button("window-maximize", grow, WindowControlArea::Max))
            })
            .child(button(
                "window-close",
                IconName::WindowClose,
                WindowControlArea::Close,
            )),
    )
}
/// How long the page's buttons take to come and go.
const MENU_FADE: Duration = Duration::from_millis(220);
/// How wide the menu of colours is: five circles and the room around them.
const COLOURS_WIDTH: Pixels = px(180.);
/// How far under its button the menu of colours rests, and how far above
/// that it starts from as it comes down.
const COLOURS_DROP: (Pixels, Pixels) = (px(6.), px(6.));

/// How much the bar spells out, for the room the window gives it. A narrow
/// window keeps every button, and drops words before it drops anything else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fit {
    /// Everything said in full.
    Full,
    /// Icons where a word was: the views, export, and no key cap.
    Compact,
    /// Icons only, and no room to say where you are.
    Tight,
}

impl Fit {
    /// For the editor's bar, which has the switch between views in its middle.
    fn editor(window: Pixels) -> Self {
        if window >= px(820.) {
            Self::Full
        } else if window >= px(540.) {
            Self::Compact
        } else {
            Self::Tight
        }
    }

    /// For the bar of a page, which has nothing in its middle.
    fn page(window: Pixels) -> Self {
        if window >= px(520.) {
            Self::Full
        } else if window >= px(440.) {
            Self::Compact
        } else {
            Self::Tight
        }
    }
}

/// A key to press, drawn as a key cap.
pub(super) fn key_cap(keys: &'static str, text: Hsla, fill: Hsla) -> Div {
    div()
        .flex_none()
        .px(px(5.))
        .py(px(1.))
        .rounded(size::RADIUS_XS)
        .bg(fill)
        .text_size(px(10.5))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(text)
        .child(keys)
}

impl Workspace {
    fn menu_button(
        &self,
        id: &'static str,
        icon: IconName,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        self.hoverable(
            icon_button(id, icon, p),
            Hot::Button(id),
            p.surface_hover,
            window,
            cx,
        )
    }

    /// Back and forward along the trail of where the reader has been. They
    /// are only there once there is somewhere to go.
    fn trail_buttons(
        &self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        if !self.can_go(false) && !self.can_go(true) {
            return None;
        }
        let mut buttons = h_flex().flex_none();
        for forward in [false, true] {
            let (id, icon, says) = match forward {
                true => (
                    "go-forward",
                    IconName::ArrowRight,
                    concat!("Forward  ", shortcut!("]")),
                ),
                false => (
                    "go-back",
                    IconName::ArrowLeft,
                    concat!("Back  ", shortcut!("[")),
                ),
            };
            let button = match self.can_go(forward) {
                true => self
                    .menu_button(id, icon, p, window, cx)
                    .tooltip(move |window, cx| Tooltip::new(says).build(window, cx))
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.go_along(forward, window, cx)),
                    ),
                // Nowhere to go that way: it keeps its place, faintly.
                false => icon_button(id, icon, p).opacity(0.3),
            };
            buttons = buttons.child(button);
        }
        Some(buttons)
    }

    /// Shows and hides the side panel: the first thing on the left, in both modes.
    fn panel_button(
        &self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        self.menu_button("toggle-panel", IconName::PanelLeft, p, window, cx)
            .when(self.panel_shown(), |button| {
                button.text_color(p.text_primary)
            })
            .on_click(cx.listener(|this, _, window, cx| this.toggle_sidebar(window, cx)))
    }

    fn search_button(
        &self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        // On a page it looks for words in what is being read; beside the
        // editor, for a file by its name. The tip says which.
        let reading = self.mode == Mode::Read;
        let finding = reading && self.finding();
        self.menu_button("search", IconName::Search, p, window, cx)
            .when(finding, |button| button.text_color(p.text_primary))
            .tooltip(move |window, cx| {
                let says = match reading {
                    true => concat!("Find on this page  ", shortcut!("F")),
                    false => concat!("Find a file by its name  ", shortcut!("K")),
                };
                Tooltip::new(says).build(window, cx)
            })
            .on_click(cx.listener(|this, _, window, cx| match this.mode {
                // A second click puts the field away again.
                Mode::Read if this.finding() => this.close_find(window, cx),
                Mode::Read => this.find_on_page(window, cx),
                Mode::Edit => this.focus_search(window, cx),
            }))
    }

    /// The circle that wears the accent colour. A click lets down the menu
    /// the colour is picked from.
    fn colour_button(&self, p: &Palette, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let open = self.colours_open;
        let presence = transition(
            "colours",
            if open { 1. } else { 0. },
            motion::glide(motion::QUICK, cx),
            window,
            cx,
        );
        let circle = div()
            .size(px(14.))
            .rounded_full()
            .border_1()
            .border_color(gpui_kit::black().opacity(0.12))
            .bg(p.accent);
        let button = div()
            .id("colours")
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(28.))
            .rounded(size::RADIUS_SM)
            .child(circle)
            // The bar drags the window from any press that reaches it.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        let button = self
            .hoverable(button, Hot::Button("colours"), p.surface_hover, window, cx)
            .when(open, |button| button.bg(p.surface_hover))
            .on_click(cx.listener(|this, _, _, cx| {
                this.colours_open = !this.colours_open;
                cx.notify();
            }));
        // Still drawn while it fades away after being shut.
        let menu = (open || presence > 0.001).then(|| self.render_colours(presence, p, window, cx));
        div().relative().flex_none().child(button).children(menu)
    }

    /// The menu of colours, let down from the circle: a circle for each, a
    /// ring round the one in use, and under them the name of the one pointed
    /// at. A click on a colour changes the whole app; the menu stays, so
    /// another can be tried.
    fn render_colours(
        &self,
        presence: f32,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let dark = cx.theme().is_dark();
        let chosen = theme::accent(cx);
        let named = Accent::ALL
            .into_iter()
            .find(|accent| self.hot == Some(Hot::Colour(*accent)))
            .unwrap_or(chosen);

        let colours = Accent::ALL.into_iter().map(|accent| {
            let colour = accent.swatch(dark);
            // The ring glides from the colour that was in use to the one that is.
            let ring = transition(
                ElementId::from(("colour-ring", accent as usize)),
                if accent == chosen { 1. } else { 0. },
                motion::glide(motion::QUICK, cx),
                window,
                cx,
            );
            // Under the pointer a colour shows a faint ring: what a click would give.
            let warmth = self.warmth(Hot::Colour(accent), window, cx);
            div()
                .id(("colour", accent as usize))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(28.))
                .rounded_full()
                .border_2()
                .border_color(colour.opacity(ring.max(0.4 * warmth)))
                .child(
                    div()
                        .size(px(20.) - px(2.) * ring)
                        .rounded_full()
                        .bg(colour),
                )
                .on_hover(cx.listener(move |this, over: &bool, _, cx| {
                    let key = Hot::Colour(accent);
                    if *over {
                        this.hot = Some(key);
                    } else if this.hot == Some(key) {
                        this.hot = None;
                    }
                    cx.notify();
                }))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.update_settings(|settings| settings.accent = accent, cx);
                }))
        });

        let menu = v_flex()
            .id("colours-menu")
            .relative()
            .w(COLOURS_WIDTH)
            .pt(px(11.))
            .pb(px(9.))
            .gap(px(7.))
            .items_center()
            .rounded(px(13.))
            .border_1()
            .border_color(p.border_strong)
            .bg(p.surface_raised)
            .shadow_lg()
            .opacity(presence)
            // What is under the menu is not clicked through it.
            .occlude()
            .child(h_flex().gap(px(4.)).children(colours))
            .child(
                div()
                    .text_size(px(11.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(p.text_secondary)
                    .child(named.label()),
            )
            .child(shut_colours(cx));

        // Drawn over everything else in the window, centred under the circle,
        // and kept clear of the window's edges.
        let (rest, travel) = COLOURS_DROP;
        deferred(
            anchored()
                .offset(point(
                    (px(28.) - COLOURS_WIDTH) / 2.,
                    rest - travel * (1. - presence),
                ))
                .snap_to_window_with_margin(px(8.))
                .child(menu),
        )
        .with_priority(1)
    }

    /// Export, as an icon: on a page, and wherever the editor's bar is short of room.
    fn export_icon(
        &self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        self.menu_button("export", IconName::Share, p, window, cx)
            .on_click(cx.listener(|this, _, window, cx| this.export_html(window, cx)))
    }

    /// Where the open document is: the folder it is in, its name as the side
    /// panel says it, and whether it has changes that are not saved yet.
    fn place(&self) -> Option<(Option<SharedString>, SharedString, bool)> {
        let doc = self.active_doc().filter(|doc| !Doc::blank(doc))?;
        let (folder, name) = match &doc.path {
            Some(path) => (
                path.parent()
                    .and_then(Path::file_name)
                    .map(|folder| SharedString::from(folder.to_string_lossy().into_owned())),
                SharedString::from(files::display_name(path)),
            ),
            None => (None, doc.name.clone()),
        };
        Some((folder, name, doc.dirty()))
    }

    /// The place written out, `folder / name`, with a dot while there are
    /// unsaved changes. Where its room ends it fades out into `backdrop`.
    fn render_place(
        &self,
        with_folder: bool,
        name_color: Hsla,
        backdrop: Hsla,
        p: &Palette,
    ) -> Option<Div> {
        let (folder, name, dirty) = self.place()?;
        let folder = folder.filter(|_| with_folder).map(|folder| {
            h_flex()
                .flex_none()
                .gap(px(6.))
                .child(div().text_color(p.text_tertiary).child(folder))
                .child(div().text_color(p.text_disabled).child("/"))
        });
        let words = h_flex()
            .flex_none()
            .gap(px(6.))
            .text_size(px(12.5))
            .children(folder)
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(name_color)
                    .child(name),
            )
            .children(dirty.then(|| div().flex_none().size(px(6.)).rounded_full().bg(p.accent)));
        Some(fading(words, backdrop))
    }

    fn menu_appearance(
        &self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let icon = if cx.theme().is_dark() {
            IconName::Sun
        } else {
            IconName::Moon
        };
        self.menu_button("appearance", icon, p, window, cx)
            .on_click(|_, _, cx| theme::toggle(cx))
    }

    /// The bar across the top of the editor.
    pub(super) fn render_titlebar(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let p = *cx.palette();
        let fit = Fit::editor(window.viewport_size().width);
        // Full screen has no window buttons, and there the kit's bar already
        // keeps its left end clear: the right end is given the same.
        let fullscreen = window.is_fullscreen();
        let (lights, edge) = match fullscreen {
            true => (px(0.), px(0.)),
            false => (TRAFFIC_LIGHTS, EDGE),
        };

        // Back to the page, in the far corner: the same key that brought the editor up.
        let read = h_flex()
            .id("to-page")
            .flex_none()
            .h(px(28.))
            .gap(px(7.))
            .map(|button| match fit {
                Fit::Full => button.pl(px(10.)).pr(px(6.)),
                Fit::Compact => button.px(px(10.)),
                Fit::Tight => button.w(px(28.)).justify_center(),
            })
            .rounded(size::RADIUS_SM)
            .border_1()
            .border_color(p.accent_border)
            .bg(p.accent_soft)
            .text_color(p.accent_text)
            .font_weight(FontWeight::SEMIBOLD)
            .hover(|style| style.opacity(0.85))
            .active(|style| style.opacity(0.7))
            .child(Icon::new(IconName::BookOpen).size(px(14.)))
            .when(fit != Fit::Tight, |button| button.child("Read"))
            .when(fit == Fit::Full, |button| {
                button.child(key_cap(shortcut!("E"), p.accent_text, p.accent_muted))
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, window, cx| this.toggle_mode(window, cx)));

        let place = match fit {
            Fit::Tight => None,
            _ => self.render_place(fit == Fit::Full, p.text_primary, p.surface_app, &p),
        };
        // The room the ends keep clear is put inside them as spacers, not as
        // padding: padding would be added to an end's share of the bar, and
        // the views would no longer be in the middle.
        let start = h_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(div().flex_none().w(lights))
            .child(self.panel_button(&p, window, cx))
            .children(self.trail_buttons(&p, window, cx))
            .children(place.map(|place| place.ml(px(4.))));
        let export = match fit {
            Fit::Full => self.render_export_button(&p, window, cx).into_any_element(),
            _ => self.export_icon(&p, window, cx).into_any_element(),
        };
        // Never narrower than what it holds: when room runs out it is the
        // place, on the left, that gives way.
        // The narrowest window has room for every button only shoulder to shoulder.
        let gap = if fit == Fit::Tight { px(2.) } else { px(4.) };
        let end = h_flex()
            .flex_1()
            .h_full()
            .justify_end()
            .child(self.search_button(&p, window, cx))
            .child(self.menu_appearance(&p, window, cx).ml(gap))
            .child(self.colour_button(&p, window, cx).ml(gap))
            .child(div().flex_none().ml(gap).child(export))
            .child(read.ml(gap + px(6.)))
            .child(div().flex_none().w(edge));

        // The two ends take the same room, which puts the views in the
        // middle of the window whatever stands either side of them.
        TitleBar::new()
            // Linux leaves the close button to the app: it asks about unsaved
            // work, as closing the window does everywhere else.
            .on_close_window(|_, window, cx| window.dispatch_action(Box::new(super::Quit), cx))
            .h(size::TITLEBAR)
            .pl(px(0.))
            .pr(if fullscreen { EDGE } else { px(0.) })
            .bg(p.surface_app)
            .border_color(p.border_subtle)
            .child(start)
            .child(self.render_view_switch(fit, &p, window, cx))
            .child(end)
    }

    /// What sits in the middle of a page's bar: smaller and bigger words with
    /// their size between them, and the switch to black and white.
    fn render_reading_controls(
        &self,
        worded: bool,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let settings = *cx.global::<crate::settings::Settings>();
        let sheet = super::frame::sheet_width(window.viewport_size());
        let body = crate::preview::body_size(Some(sheet), settings.reading_scale);
        let dark = cx.theme().is_dark();
        let (strong, hover) = (p.text_primary, p.surface_hover);

        let smaller = self
            .menu_button("words-smaller", IconName::AArrowDown, p, window, cx)
            .tooltip(|window, cx| {
                Tooltip::new(concat!("Smaller words  ", shortcut!("−"))).build(window, cx)
            })
            .on_click(|_, window, cx| window.dispatch_action(Box::new(super::FontSmaller), cx));
        let bigger = self
            .menu_button("words-bigger", IconName::AArrowUp, p, window, cx)
            .tooltip(|window, cx| {
                Tooltip::new(concat!("Bigger words  ", shortcut!("+"))).build(window, cx)
            })
            .on_click(|_, window, cx| window.dispatch_action(Box::new(super::FontBigger), cx));
        // The size itself, between them. A click goes back to the page's own.
        let size_now = div()
            .id("words-size")
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .w(px(34.))
            .h(px(28.))
            .rounded(size::RADIUS_SM)
            .text_color(p.text_secondary)
            .hover(move |style| style.bg(hover).text_color(strong))
            .child(format!("{}", f32::from(body).round()))
            .tooltip(|window, cx| {
                Tooltip::new(concat!("The page's own size  ", shortcut!("0"))).build(window, cx)
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(|_, window, cx| window.dispatch_action(Box::new(super::FontReset), cx));

        // Black and white: lit while it is on, and saying in words what a
        // click will do.
        let on = settings.contrast;
        let says = match (on, dark) {
            (true, _) => "Back to the usual colours",
            (false, false) => "Darker words on a paper-white page: sharper, with no glare",
            (false, true) => "Lighter words on a near-black page: sharper, with no glare",
        };
        let contrast = h_flex()
            .id("black-and-white")
            .flex_none()
            .h(px(28.))
            .gap(px(6.))
            .px(px(if worded { 9. } else { 6. }))
            .rounded(size::RADIUS_SM)
            .border_1()
            .map(|button| match on {
                true => button
                    .border_color(p.accent_border)
                    .bg(p.accent_soft)
                    .text_color(p.accent_text),
                false => button
                    .border_color(transparent_black())
                    .text_color(p.text_secondary)
                    .hover(move |style| style.bg(hover).text_color(strong)),
            })
            .child(Icon::new(IconName::Contrast).size(px(15.)))
            .when(worded, |button| button.child("Black & white"))
            .tooltip(move |window, cx| Tooltip::new(says).build(window, cx))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(|_, window, cx| window.dispatch_action(Box::new(super::ToggleContrast), cx));

        h_flex()
            .id("page-menu-middle")
            .gap(px(2.))
            .on_hover(cx.listener(|this, over: &bool, _, cx| {
                this.reader.over_menu(*over, cx);
            }))
            .child(smaller)
            .child(size_now)
            .child(bigger)
            .child(div().flex_none().w(px(8.)))
            .child(contrast)
    }

    /// The bar across the top of a page: the same ends as the editor's, with
    /// nothing between them. Its buttons are there while the pointer moves
    /// and go once the pointer has been still for a moment; the name stays.
    pub(super) fn render_page_menu(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let p = *cx.palette();
        let width = window.viewport_size().width;
        let fit = Fit::page(width);
        let shown = self.reader.menu_shown() || self.colours_open;
        let presence = transition(
            "page-menu",
            if shown { 1. } else { 0. },
            Transition::new(MENU_FADE).ease(motion::ease),
            window,
            cx,
        );
        // With the side panel over the page, the left end keeps to what is left of the page.
        let aside = transition(
            "floating-panel",
            if self.reader.panel_open { 1. } else { 0. },
            motion::glide(motion::PANEL, cx),
            window,
            cx,
        );
        let lights = match window.is_fullscreen() {
            true => EDGE,
            false => TRAFFIC_LIGHTS,
        };
        let left = match width - super::reader::PANEL_WIDTH > px(280.) {
            true => lights.max(super::reader::PANEL_WIDTH * aside + px(14.)),
            false => lights,
        };

        let (bright, dark) = (p.accent_hover, p.text_on_accent);
        let edit = h_flex()
            .id("to-editor")
            .flex_none()
            .h(px(30.))
            .gap(px(7.))
            .map(|button| match fit {
                Fit::Full => button.pl(px(11.)).pr(px(7.)),
                Fit::Compact => button.px(px(11.)),
                Fit::Tight => button.w(px(30.)).justify_center(),
            })
            .rounded(size::RADIUS_MD)
            .bg(p.accent)
            .text_color(dark)
            .font_weight(FontWeight::SEMIBOLD)
            .hover(move |style| style.bg(bright))
            .active(|style| style.opacity(0.8))
            .child(Icon::new(IconName::Pencil).size(px(14.)))
            .when(fit != Fit::Tight, |button| button.child("Edit"))
            .when(fit == Fit::Full, |button| {
                button.child(key_cap(shortcut!("E"), dark, dark.opacity(0.12)))
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, window, cx| this.toggle_mode(window, cx)));

        // The name is quiet while you read and comes forward with the buttons.
        let name_color = theme::blend(p.text_tertiary, p.text_primary, presence);
        let place = self.render_place(false, name_color, p.surface_preview, &p);
        let start = h_flex()
            .id("page-menu-start")
            .flex_1()
            .min_w_0()
            .h_full()
            .gap(px(4.))
            .on_hover(cx.listener(|this, over: &bool, _, cx| {
                this.reader.over_menu(*over, cx);
            }))
            .child(
                div()
                    .flex_none()
                    .opacity(presence)
                    .child(self.panel_button(&p, window, cx)),
            )
            .children(
                self.trail_buttons(&p, window, cx)
                    .map(|buttons| buttons.opacity(presence)),
            )
            .children(place);
        let end = h_flex()
            .id("page-menu-end")
            .flex_none()
            .h_full()
            .pl(px(8.))
            .gap(px(4.))
            .opacity(presence)
            .on_hover(cx.listener(|this, over: &bool, _, cx| {
                this.reader.over_menu(*over, cx);
            }))
            .child(self.search_button(&p, window, cx))
            .child(self.menu_appearance(&p, window, cx))
            .child(self.colour_button(&p, window, cx))
            .child(self.export_icon(&p, window, cx))
            .child(div().flex_none().w(px(2.)))
            .child(edit);

        // In the middle of the bar: how big the words are, and whether the
        // page is in black and white. Only where the bar has the room.
        let reading = (width >= px(620.)).then(|| {
            let controls = self.render_reading_controls(width >= px(780.), &p, window, cx);
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .opacity(presence)
                .child(controls)
        });
        // The name gives way before it runs under what is in the middle.
        let start = start.when(reading.is_some(), |start| {
            start.max_w(((width - px(300.)) / 2. - left).max(px(60.)))
        });

        // Where the window's buttons are ours to draw they close the bar,
        // and stay when the rest goes: a window can always be shut.
        let buttons = window_buttons(&p, window);
        div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(size::TITLEBAR)
            .flex()
            .items_center()
            .justify_between()
            .pl(left)
            .pr(if buttons.is_some() { px(0.) } else { EDGE })
            .child(start)
            .children(reading)
            .child(end)
            .children(buttons.map(|buttons| buttons.ml(px(6.))))
    }

    /// Editor / Split / Preview. The yellow pill slides to the chosen view.
    fn render_view_switch(
        &self,
        fit: Fit,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        // Every segment is as wide as the others, so the pill only ever has
        // to move. Short of room, the views go by their icons alone.
        let worded = fit == Fit::Full;
        let segment = match fit {
            Fit::Full => px(86.),
            Fit::Compact => px(36.),
            Fit::Tight => px(34.),
        };
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
            segment * chosen as f32,
            motion::glide(motion::QUICK, cx),
            window,
            cx,
        );

        let items = modes.into_iter().map(|(mode, label, icon)| {
            let strong = p.text_primary;
            h_flex()
                .id(label)
                .w(segment)
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
                .when(worded, |item| item.child(label))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.set_view(mode, cx);
                    this.focus_document(window, cx);
                }))
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
                            .w(segment)
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
            .rounded(size::RADIUS_SM)
            .border_1()
            .border_color(p.border_subtle)
            .bg(p.surface_raised)
            .font_weight(FontWeight::MEDIUM)
            .overflow_hidden()
            .child(self.hoverable(label, Hot::Button("export"), p.surface_hover, window, cx))
    }
}

/// Paints nothing: it fills the menu of colours and listens for a press
/// anywhere outside it, which shuts it. (Escape does too: the workspace sees
/// to that, since a key only goes to what holds the keyboard.)
fn shut_colours(cx: &mut Context<Workspace>) -> impl IntoElement + use<> {
    let workspace = cx.weak_entity();
    canvas(
        |_, _, _| {},
        move |menu, _, window, _| {
            let shut = move |cx: &mut App| {
                workspace
                    .update(cx, |this, cx| {
                        // A press on the circle is the circle's to answer: it shuts the menu itself.
                        if this.colours_open && this.hot != Some(Hot::Button("colours")) {
                            this.colours_open = false;
                            cx.notify();
                        }
                    })
                    .ok();
            };
            window.on_mouse_event(move |event: &MouseDownEvent, phase, _, cx| {
                if phase == DispatchPhase::Capture && !menu.contains(&event.position) {
                    shut(cx);
                }
            });
        },
    )
    .absolute()
    .inset_0()
}

#[cfg(test)]
mod tests {
    use super::Fit;
    use gpui_kit::px;

    #[test]
    fn the_bar_says_less_as_the_window_narrows() {
        // The editor as it opens, a narrow one, and the narrowest the window goes.
        assert_eq!(Fit::editor(px(1280.)), Fit::Full);
        assert_eq!(Fit::editor(px(700.)), Fit::Compact);
        assert_eq!(Fit::editor(px(380.)), Fit::Tight);
        // A page the shape of a sheet, a narrow one, and the narrowest.
        assert_eq!(Fit::page(px(874.)), Fit::Full);
        assert_eq!(Fit::page(px(480.)), Fit::Compact);
        assert_eq!(Fit::page(px(380.)), Fit::Tight);
    }
}
