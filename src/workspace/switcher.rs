//! The switcher: every paper, small, in one slim bar.
//!
//! It is drawn to frame 5 of the "Windows" page of `design/zinMD-UI`. Hold
//! Control and press Tab: a bar comes up in the middle of the screen, over
//! the papers, which stay as they are. In it there is one small page for
//! each paper, the one used last first. Each is the page itself, drawn
//! small: what the file shows where it was left, with its name under it
//! and a thin line along its foot for how far it has been read. The paper
//! that will come forward has a soft plate under it, which glides from
//! page to page as Tab is pressed. Letting go of Control brings that paper
//! to the front, so one quick Control Tab still goes back to the paper
//! read before.
//!
//! The bar is all there is of the switcher: nothing is drawn around it.
//!
//! The arrows move the plate too, Return or a click brings a paper
//! forward, W closes one, and Escape goes back to the paper it was opened
//! from.

use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::base::text::TextView;
use gpui_kit::base::transition;
use gpui_kit::component::text::TextViewState;
use gpui_kit::component::{ActiveTheme as _, Icon, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{LastFile, Workspace, papers};
use crate::theme::{ActivePalette as _, Palette};
use crate::{motion, preview};

/// How wide and tall a small page is, the room between two, and around
/// them all in the bar.
const WIDE: Pixels = px(112.);
const TALL: Pixels = px(148.);
const GAP: Pixels = px(14.);
const PAD: Pixels = px(16.);
/// What is under a small page: its name.
const UNDER: Pixels = px(26.);
/// How far a small page goes on past the edge of its sheet, unseen.
const BEYOND: Pixels = px(20.);
/// How far the plate under the chosen page reaches past it.
const PLATE: Pixels = px(8.);
/// How many small pages there are to a row, and the room between two rows.
const ACROSS: usize = 7;
const BETWEEN: Pixels = px(10.);
/// Room around the bar, in a window that is otherwise clear, for its shadow.
const SHADE: Pixels = px(64.);

/// How big the bar is for `count` papers.
fn bar_size(count: usize) -> Size<Pixels> {
    let across = count.clamp(1, ACROSS);
    let rows = count.div_ceil(ACROSS).max(1);
    size(
        PAD * 2. + WIDE * across as f32 + GAP * (across - 1) as f32,
        PAD * 2. - px(6.) + (TALL + UNDER) * rows as f32 + BETWEEN * (rows - 1) as f32,
    )
}

/// Where in the bar the small page of the paper numbered `at` is.
fn place_of(at: usize) -> Point<Pixels> {
    let (along, down) = (at % ACROSS, at / ACROSS);
    point(
        PAD + (WIDE + GAP) * along as f32,
        PAD + (TALL + UNDER + BETWEEN) * down as f32,
    )
}

/// A page drawn small. Much of what a page is made of is sized by the
/// measure all sizes are counted in, the rem: the box of a task, the room
/// in a table, the name of a language over its code. Drawn inside this,
/// that measure is as many times smaller as the page is, so all of it
/// shrinks with the words.
struct Small {
    rem: Pixels,
    page: AnyElement,
}

impl IntoElement for Small {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Small {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let rem = self.rem;
        let layout = window.with_rem_size(Some(rem), |window| self.page.request_layout(window, cx));
        (layout, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let rem = self.rem;
        window.with_rem_size(Some(rem), |window| self.page.prepaint(window, cx));
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let rem = self.rem;
        window.with_rem_size(Some(rem), |window| self.page.paint(window, cx));
    }
}

/// What a paper says of itself, for its small page.
pub(super) struct Brief {
    pub name: SharedString,
    /// How far it has been read, 0 to 1, once it has been read at all.
    pub read: Option<f32>,
    /// The text its page shows, and the folder its pictures are named from.
    pub text: SharedString,
    pub folder: Option<PathBuf>,
    /// The block at the top of its page, and how far into it the page is.
    pub top: ListOffset,
    /// How wide its window is, how wide the sheet of text in it, and how
    /// big its words.
    pub window: Pixels,
    pub sheet: Pixels,
    pub body: Pixels,
    /// Whether it has changes that are not saved.
    pub dirty: bool,
}

struct Entry {
    paper: AnyWindowHandle,
    brief: Brief,
    /// The page, drawn small.
    page: Entity<TextViewState>,
    /// Whether the small page has been taken to the place the paper is at.
    placed: bool,
}

pub(super) struct Switcher {
    entries: Vec<Entry>,
    /// The paper the ring is on: the one that is brought forward.
    chosen: usize,
    /// The small page the pointer is on.
    pointed: Option<usize>,
    /// The paper the switcher was opened from.
    from: AnyWindowHandle,
    /// Whether Control is being held: letting go of it chooses.
    held: bool,
    focus: FocusHandle,
    /// Looks, while Control is held, for it being let go of.
    _watch: Task<()>,
}

/// How often the switcher looks whether Control is still held.
const LOOK: Duration = Duration::from_millis(20);

impl Workspace {
    /// What this paper says of itself to the switcher.
    pub(super) fn brief(&self, frame: [f32; 4], cx: &App) -> Option<Brief> {
        let doc = self.active_doc()?;
        let name = match &doc.path {
            Some(path) => crate::files::display_name(path),
            None => doc.name.to_string(),
        };
        // A paper in the editor's wide window is shown as the page it reads as.
        let page = match self.mode {
            super::Mode::Read => size(px(frame[2]), px(frame[3])),
            super::Mode::Edit => super::page_size(size(px(frame[2]), px(frame[3]))),
        };
        let sheet = super::frame::sheet_width(page);
        let scale = cx.global::<crate::settings::Settings>().reading_scale;
        Some(Brief {
            name: name.into(),
            read: doc
                .path
                .as_deref()
                .and_then(|path| self.reader.read(path))
                .filter(|far| *far > 0.005),
            text: match &doc.links {
                Some(shown) => shown.text.clone().into(),
                None => doc.text.clone(),
            },
            folder: doc
                .path
                .as_deref()?
                .parent()
                .map(|folder| folder.to_path_buf()),
            top: doc.preview.read(cx).list_state().logical_scroll_top(),
            window: page.width,
            sheet,
            body: preview::body_size(Some(sheet), scale),
            dirty: doc.dirty(),
        })
    }

    /// Bring up the switcher, with the ring on the paper used before this
    /// one. With one paper there is nothing to switch between: Control Tab
    /// goes back to the file read before, as it always did.
    pub(super) fn switch_papers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.switch_papers_as(None, window, cx);
    }

    /// The same, with `held` saying whether Control is taken to be held:
    /// a scripted run has no keys to hold.
    pub(super) fn switch_papers_as(
        &mut self,
        held: Option<bool>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // It is up already, and this paper still has the keyboard: Control
        // Tab again moves the ring on.
        if let Ok(tab) = Keystroke::parse("tab")
            && self.switcher_key(&tab, cx)
        {
            return;
        }
        let here = window.window_handle();
        let open = papers::open_papers(cx);
        if open.len() < 2 {
            return self.last_file(window, cx);
        }
        let frames = papers::frames(cx);
        let frame_of = |paper: &AnyWindowHandle| {
            frames
                .iter()
                .find(|(window, _)| window == paper)
                .map(|(_, frame)| *frame)
        };
        // This paper says what it is itself: it is in the middle of being
        // asked, and cannot be asked again from outside.
        let briefs: Vec<(AnyWindowHandle, Brief)> = open
            .iter()
            .filter_map(|(paper, workspace)| {
                let frame = frame_of(paper)?;
                let brief = match *paper == here {
                    true => self.brief(frame, cx),
                    false => workspace.read(cx).brief(frame, cx),
                }?;
                Some((*paper, brief))
            })
            .collect();
        if briefs.len() < 2 {
            return self.last_file(window, cx);
        }
        // Asked for with Control Tab, the switcher stays up for as long as
        // Control is held. Where the system can say, it is asked whether it
        // still is: this paper was told that it went down, and may not have
        // been told yet that it came up.
        let pressed = held.unwrap_or(window.modifiers().control);
        let held = held.unwrap_or(crate::system::control_held().unwrap_or(pressed));
        if pressed && !held {
            // One quick Control Tab, already let go of: straight to the
            // paper used before this one.
            if let Some(before) = briefs
                .iter()
                .map(|(paper, _)| *paper)
                .find(|paper| *paper != here)
            {
                cx.defer(move |cx| {
                    before
                        .update(cx, |_, window, _| window.activate_window())
                        .ok();
                });
            }
            return;
        }
        let display = window.display(cx).map(|display| display.id());
        cx.defer(move |cx| open_switcher(briefs, here, held, display, cx));
    }
}

impl Workspace {
    /// Pass a key on to the switcher, if it is up: its own window may not
    /// have been given the keyboard. `true` when the key was the switcher's.
    pub(super) fn switcher_key(&mut self, key: &Keystroke, cx: &mut Context<Self>) -> bool {
        let Some((window, switcher)) = papers::switcher(cx) else {
            return false;
        };
        let wanted = matches!(
            key.key.as_str(),
            "tab" | "left" | "right" | "up" | "down" | "enter" | "space" | "escape" | "w"
        );
        if wanted {
            let key = key.clone();
            cx.defer(move |cx| {
                window
                    .update(cx, |_, window, cx| {
                        switcher.update(cx, |switcher, cx| switcher.key(&key, window, cx));
                    })
                    .ok();
            });
        }
        // While it is up, no key is this paper's.
        true
    }

    /// A modifier key went down or up while the switcher may be up: letting
    /// go of Control brings the paper its ring is on forward.
    pub(super) fn switcher_let_go(&mut self, modifiers: Modifiers, cx: &mut Context<Self>) {
        if let Some((window, switcher)) = papers::switcher(cx) {
            cx.defer(move |cx| {
                window
                    .update(cx, |_, window, cx| {
                        switcher.update(cx, |switcher, cx| switcher.let_go(modifiers, window, cx));
                    })
                    .ok();
            });
        }
    }
}

/// Open the switcher's window: in the middle of the display the paper it
/// was asked for from is on, above the papers.
fn open_switcher(
    briefs: Vec<(AnyWindowHandle, Brief)>,
    from: AnyWindowHandle,
    held: bool,
    display: Option<DisplayId>,
    cx: &mut App,
) {
    // The window is as big as the bar and its shadow, in the middle of the
    // display, and clear: the bar is all that is seen of it.
    let bar = bar_size(briefs.len());
    let room = size(bar.width + SHADE * 2., bar.height + SHADE * 2.);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(display, room, cx))),
        titlebar: None,
        focus: true,
        show: true,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        display_id: display,
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    };
    let mut made = None;
    let opened = cx
        .open_window(options, |window, cx| {
            let switcher = cx.new(|cx| Switcher::new(briefs, from, held, window, cx));
            made = Some(switcher.clone());
            // What a window is drawn on is filled with the colour of the
            // theme, which would be a box round the bar: a window with a
            // window in it. This one is left clear.
            cx.new(|cx| {
                gpui_kit::base::Root::new(switcher, window, cx).bg(gpui_kit::transparent_black())
            })
        })
        .ok()
        .map(AnyWindowHandle::from)
        .zip(made)
        .ok_or(());
    if let Ok((window, switcher)) = opened {
        papers::switcher_opened(window, switcher.downgrade(), cx);
        window
            .update(cx, |_, window, cx| {
                window.activate_window();
                switcher.read(cx).focus.clone().focus(window, cx);
            })
            .ok();
    }
}

impl Switcher {
    fn new(
        briefs: Vec<(AnyWindowHandle, Brief)>,
        from: AnyWindowHandle,
        held: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // The paper it was opened from comes first, whatever was used last.
        let mut briefs = briefs;
        briefs.sort_by_key(|(paper, _)| *paper != from);
        let entries = briefs
            .into_iter()
            .map(|(paper, brief)| {
                let page = cx.new(|cx| TextViewState::markdown(&brief.text, cx));
                Entry {
                    paper,
                    brief,
                    page,
                    placed: false,
                }
            })
            .collect();
        // Clicking away from the switcher puts it away.
        cx.observe_window_activation(window, |this: &mut Self, window, cx| {
            if !window.is_window_active() {
                this.leave(None, window, cx);
            }
        })
        .detach();
        // Letting go of Control is not something a window is always told:
        // the system gives the news to the window with the keyboard, which
        // may be the paper. So while Control holds the switcher up, it asks.
        let watch = cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(LOOK).await;
                let done = this.update_in(cx, |this: &mut Self, window, cx| {
                    match (this.held, crate::system::control_held()) {
                        (true, Some(false)) => {
                            this.held = false;
                            this.choose(window, cx);
                            true
                        }
                        (true, _) => false,
                        (false, _) => true,
                    }
                });
                if !matches!(done, Ok(false)) {
                    break;
                }
            }
        });
        Self {
            entries,
            // The paper used before the one in front.
            chosen: 1,
            pointed: None,
            from,
            held,
            focus: cx.focus_handle(),
            _watch: watch,
        }
    }

    /// Move the ring by `by` papers, round from the last to the first.
    fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        let count = self.entries.len() as isize;
        self.chosen = (self.chosen as isize + by).rem_euclid(count) as usize;
        cx.notify();
    }

    /// Put the switcher away, and bring `paper` to the front: the one it
    /// was opened from when none is given.
    fn leave(
        &mut self,
        paper: Option<AnyWindowHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let paper = paper.unwrap_or(self.from);
        crate::startup::trace(|| {
            let name = self
                .entries
                .iter()
                .find(|entry| entry.paper == paper)
                .map(|entry| entry.brief.name.clone());
            format!("switcher: {name:?} to the front")
        });
        window.remove_window();
        cx.defer(move |cx| {
            paper
                .update(cx, |_, window, _| window.activate_window())
                .ok();
        });
    }

    /// Bring the paper the ring is on to the front.
    fn choose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paper = self.entries.get(self.chosen).map(|entry| entry.paper);
        self.leave(paper, window, cx);
    }

    /// Close the paper at `at`. One with changes that are not saved is
    /// brought to the front instead, to be asked about there.
    fn close(&mut self, at: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.entries.get(at) else {
            return;
        };
        let paper = entry.paper;
        if entry.brief.dirty {
            return self.leave(Some(paper), window, cx);
        }
        self.entries.remove(at);
        cx.defer(move |cx| {
            paper.update(cx, |_, window, _| window.remove_window()).ok();
        });
        // With one paper left there is nothing to switch between.
        match self.entries.as_slice() {
            [] => window.remove_window(),
            [only] => {
                let only = only.paper;
                self.leave(Some(only), window, cx)
            }
            _ => {
                if self.from == paper {
                    self.from = self.entries[0].paper;
                }
                self.chosen = self.chosen.min(self.entries.len() - 1);
                cx.notify();
            }
        }
    }

    /// Control was let go of, or another modifier changed: with the ring
    /// held by Control, letting go brings its paper forward.
    pub(super) fn let_go(
        &mut self,
        modifiers: Modifiers,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.held && !modifiers.control {
            self.held = false;
            self.choose(window, cx);
        }
    }

    /// A key for the switcher. It is asked from its own window, and from
    /// the paper it was opened from, which may still have the keyboard.
    pub(super) fn key(&mut self, key: &Keystroke, window: &mut Window, cx: &mut Context<Self>) {
        match key.key.as_str() {
            "tab" if key.modifiers.shift => self.step(-1, cx),
            "tab" | "right" => self.step(1, cx),
            "left" => self.step(-1, cx),
            "down" => self.step(ACROSS as isize, cx),
            "up" => self.step(-(ACROSS as isize), cx),
            "enter" | "space" => self.choose(window, cx),
            "escape" => self.leave(None, window, cx),
            "w" => self.close(self.chosen, window, cx),
            _ => {}
        }
    }

    /// One small page, with its name under it, at its place in the bar.
    fn render_page(
        &mut self,
        at: usize,
        p: &Palette,
        dark: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let (chosen, pointed) = (self.chosen == at, self.pointed == Some(at));
        let entry = &mut self.entries[at];
        let brief = &entry.brief;
        // Everything as many times smaller as the small page is narrower
        // than the paper: the same lines, broken at the same words.
        let small = WIDE / brief.window;
        let (body, margin) = (brief.body * small, (WIDE - brief.sheet * small) / 2.);
        let rem = window.rem_size() * small;
        // The small page is taken to where the paper is, once it has blocks.
        if !entry.placed {
            let list = entry.page.read(cx).list_state().clone();
            if list.item_count() > brief.top.item_ix {
                list.scroll_to(ListOffset {
                    item_ix: brief.top.item_ix,
                    offset_in_item: brief.top.offset_in_item * small,
                });
                entry.placed = true;
            }
        }
        let folder = brief.folder.clone();
        // The room in a table is not counted in the small measure by what
        // lays its columns out, so it is said outright.
        let style = preview::style_at(p, body, dark, rem).with_table_cell(
            StyleRefinement::default()
                .px(px(12.) * small)
                .py(px(8.) * small),
        );
        let page = TextView::new(&entry.page)
            .scrollable(true)
            .selectable(false)
            .style(style)
            .markdown_extensions(preview::extensions())
            .image_source(move |uri| preview::image_source(uri, folder.as_deref()))
            // Nothing over a block of code: its name would not be small.
            .code_block_actions(|_, _, _| div())
            .text_size(body)
            .line_height(relative(1.55))
            .pl(margin)
            .pr(margin + BEYOND)
            .pt(px(9.));
        // The page fills the sheet, and is drawn with everything small. It
        // is a little wider than the sheet, which cuts off what is beyond
        // its edge: the bar a page shows as it is scrolled.
        let page = Small {
            rem,
            page: div()
                .absolute()
                .top_0()
                .bottom_0()
                .left_0()
                .w(WIDE + BEYOND)
                .child(page)
                .into_any_element(),
        };
        let sheet = div()
            .id(("small-page", at))
            .relative()
            .w(WIDE)
            .h(TALL)
            .rounded(px(6.))
            .overflow_hidden()
            .bg(p.surface_preview)
            .border_1()
            .border_color(p.border_subtle)
            .shadow_sm()
            .child(page)
            // Nothing in a small page is clicked, selected or scrolled: the
            // page as a whole is what is chosen.
            .child(div().absolute().inset_0().occlude())
            // How far it has been read: a thin line along its foot.
            .children(brief.read.map(|far| {
                div()
                    .absolute()
                    .left_0()
                    .bottom_0()
                    .h(px(2.))
                    .w(relative(far.clamp(0., 1.)))
                    .bg(p.accent)
            }))
            .on_hover(cx.listener(move |this, over: &bool, _, cx| {
                this.pointed = match (*over, this.pointed) {
                    (true, _) => Some(at),
                    (false, Some(was)) if was == at => None,
                    (false, other) => other,
                };
                cx.notify();
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.chosen = at;
                this.choose(window, cx);
            }));
        let name = div()
            .flex()
            .items_center()
            .justify_center()
            .w(WIDE)
            .h(UNDER)
            .child(
                div()
                    .max_w(WIDE)
                    .truncate()
                    .text_size(px(11.))
                    .map(|name| match chosen {
                        true => name
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.text_primary),
                        false => name.text_color(p.text_tertiary),
                    })
                    .child(brief.name.clone()),
            );
        // Under the pointer, a paper can be closed from here.
        let close = pointed.then(|| {
            let hover = p.surface_hover;
            div()
                .id(("close-paper", at))
                .absolute()
                .top(px(-7.))
                .right(px(-7.))
                .flex()
                .items_center()
                .justify_center()
                .size(px(18.))
                .rounded_full()
                .border_1()
                .border_color(p.border_strong)
                .bg(p.surface_raised)
                .shadow_sm()
                .text_color(p.text_secondary)
                .hover(move |style| style.bg(hover))
                .child(Icon::new(IconName::X).size(px(9.)))
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.close(at, window, cx);
                }))
        });
        let place = place_of(at);
        v_flex()
            .absolute()
            .left(place.x)
            .top(place.y)
            .w(WIDE)
            .child(sheet)
            .child(name)
            .children(close)
    }
}

impl Render for Switcher {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = *cx.palette();
        let dark = cx.theme().is_dark();
        // The keys are the switcher's for as long as it is up.
        if !self.focus.is_focused(window) {
            self.focus.focus(window, cx);
        }
        let pages: Vec<Div> = (0..self.entries.len())
            .map(|at| self.render_page(at, &p, dark, window, cx))
            .collect();
        // The plate under the paper that will come forward. It glides from
        // one page to the next.
        let under = place_of(self.chosen);
        let glide = motion::glide(motion::QUICK, cx);
        let left = transition("switcher-plate-x", under.x, glide.clone(), window, cx);
        let top = transition("switcher-plate-y", under.y, glide, window, cx);
        let plate = div()
            .absolute()
            .left(left - PLATE)
            .top(top - PLATE)
            .w(WIDE + PLATE * 2.)
            .h(TALL + UNDER + PLATE + px(2.))
            .rounded(px(12.))
            .bg(p.surface_selected);
        let size = bar_size(self.entries.len());
        let bar = div()
            .relative()
            .w(size.width)
            .h(size.height)
            .rounded(px(20.))
            .border_1()
            .border_color(p.border_raised)
            .bg(p.surface_raised)
            .shadow_lg()
            .child(plate)
            .children(pages)
            // It comes up with a short fade, settling a few points down.
            .with_animation(
                "switcher-arrives",
                Animation::new(match cx.reduce_motion() {
                    true => Duration::from_millis(1),
                    false => motion::QUICK,
                })
                .with_easing(motion::ease),
                |bar, progress| bar.opacity(progress).mt(px(-8.) * (1. - progress)),
            );
        div()
            .id("switcher")
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .track_focus(&self.focus)
            .text_color(p.text_primary)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                this.key(&event.keystroke, window, cx);
                cx.stop_propagation();
            }))
            // Control Tab again, while Control is still held: the next paper.
            .on_action(cx.listener(|this, _: &LastFile, _, cx| this.step(1, cx)))
            // Letting go of Control brings the paper on the plate forward.
            .on_modifiers_changed(
                cx.listener(|this, event: &ModifiersChangedEvent, window, cx| {
                    this.let_go(event.modifiers, window, cx)
                }),
            )
            .child(bar)
    }
}
