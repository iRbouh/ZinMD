//! The page: a document to read, and nothing else around it.
//!
//! The window is the sheet. Its bar shows its buttons while the pointer
//! moves, the side panel slides over it when asked for, and down its right
//! edge runs a rail with one mark for each heading: point at a mark to see
//! what that section is about, click it to go there.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui_kit::assets::IconName;
use gpui_kit::base::{Transition, transition};
use gpui_kit::component::{ActiveTheme as _, Icon, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::menu::key_cap;
use super::{Doc, Mode, OpenFile, Workspace};
use crate::session::PROGRESS_KEPT;
use crate::system::shortcut;
use crate::theme::{ActivePalette as _, Palette, size};
use crate::{doc, motion};

/// Clear space above the text of a page, under the menu.
pub(super) const PAGE_TOP: Pixels = px(62.);
/// Clear space under the text of a page: a sheet has a margin at its foot
/// too, and the text never runs into the edge of the window.
pub(super) const PAGE_BOTTOM: Pixels = px(40.);
/// How wide the side panel is when it floats over a page.
pub(super) const PANEL_WIDTH: Pixels = px(284.);
/// How long the menu stays once the pointer has stopped.
const MENU_STAYS: Duration = Duration::from_millis(2600);
/// The strip down the right edge that the marks sit in.
const RAIL: Pixels = px(44.);
/// How far apart the marks are, when there is room.
const PITCH: Pixels = px(12.);
const CARD_WIDTH: Pixels = px(308.);
/// How long the page takes to glide to a place nearby.
const GLIDE: Duration = Duration::from_millis(340);
/// How long it takes to change to a place far away: out, then in.
const CUT: Duration = Duration::from_millis(300);
/// The share of a cut spent fading out.
const CUT_OUT: f32 = 0.35;
/// How long a move to a line waits for a page that is not ready: the file
/// was only just opened.
const PATIENCE: Duration = Duration::from_secs(2);
/// How many lines down a block a line can be and still be on the page when
/// the top of the block is at the top of the page.
const DEEP: f32 = 10.;

/// A line of a document to go to on the page, with words to mark there when
/// it was found by the search.
struct Arriving {
    doc: usize,
    line: u32,
    words: Option<SharedString>,
    since: Instant,
}

/// What the card beside the rail says about a section.
struct Card {
    doc: usize,
    at: usize,
    place: SharedString,
    title: SharedString,
    about: SharedString,
}

/// The page on its way to another place in the document.
enum PageMove {
    /// Somewhere near: it slides there.
    Glide {
        doc: usize,
        total: Pixels,
        done: Pixels,
        /// Where exactly to end up, when that is known.
        exact: Option<ListOffset>,
        started: Instant,
    },
    /// Somewhere far: it fades out, changes place, and fades back in.
    Cut {
        doc: usize,
        target: ListOffset,
        jumped: bool,
        started: Instant,
    },
}

pub(super) struct Reader {
    /// Whether the side panel is floating over the page.
    pub panel_open: bool,
    /// When the pointer last moved over the page.
    seen: Instant,
    over_menu: bool,
    /// Whether something is waiting for the pointer to have been still long enough.
    watching: bool,
    _hide: Task<()>,
    /// The top of the page was pressed: the next move of the pointer moves the window.
    grabbed: bool,
    /// Whether the pointer is on the rail, and which mark it is at.
    on_rail: bool,
    mark: Option<usize>,
    card: Option<Card>,
    motion: Option<PageMove>,
    arriving: Option<Arriving>,
    /// How far each file has been read, 0 to 1, most recently read first.
    pub progress: Vec<(PathBuf, f32)>,
}

impl Reader {
    pub(super) fn new(progress: Vec<(PathBuf, f32)>) -> Self {
        Self {
            panel_open: false,
            seen: Instant::now(),
            over_menu: false,
            watching: false,
            _hide: Task::ready(()),
            grabbed: false,
            on_rail: false,
            mark: None,
            card: None,
            motion: None,
            arriving: None,
            progress,
        }
    }

    /// The pointer moved, or something else happened that the menu should be
    /// there for. It stays a while, then steps aside.
    pub(super) fn wake(&mut self, cx: &mut Context<Workspace>) {
        let asleep = self.seen.elapsed() >= MENU_STAYS;
        self.seen = Instant::now();
        if asleep {
            cx.notify();
        }
        // Moves come by the hundred: one watch is kept, which waits until the
        // pointer has been still for long enough, however often it moves meanwhile.
        if self.watching {
            return;
        }
        self.watching = true;
        self._hide = cx.spawn(async move |this, cx| {
            loop {
                let Ok(left) = this.update(cx, |this, _| {
                    MENU_STAYS.saturating_sub(this.reader.seen.elapsed())
                }) else {
                    return;
                };
                if left.is_zero() {
                    break;
                }
                cx.background_executor()
                    .timer(left + Duration::from_millis(30))
                    .await;
            }
            this.update(cx, |this, cx| {
                this.reader.watching = false;
                cx.notify();
            })
            .ok();
        });
    }

    pub(super) fn over_menu(&mut self, over: bool, cx: &mut Context<Workspace>) {
        self.over_menu = over;
        self.wake(cx);
        cx.notify();
    }

    /// Whether the menu is there: while the pointer moves or rests on it, and
    /// while the side panel, which it opens and closes, is over the page.
    pub(super) fn menu_shown(&self) -> bool {
        self.over_menu || self.panel_open || self.seen.elapsed() < MENU_STAYS
    }

    /// Point at a mark of the rail, or at none. For scripted runs.
    #[cfg(feature = "shot")]
    pub(super) fn point_at(&mut self, mark: Option<usize>) {
        self.on_rail = mark.is_some();
        self.mark = mark;
    }

    /// How far `path` has been read: 0 to 1, or `None` if it never was.
    pub(super) fn read(&self, path: &Path) -> Option<f32> {
        self.progress
            .iter()
            .find(|(read, _)| read == path)
            .map(|(_, far)| *far)
    }

    /// Note how far down `path` the reader is. Only the furthest point counts.
    fn note(&mut self, path: &Path, far: f32) {
        match self.progress.iter().position(|(read, _)| read == path) {
            // A step too small to matter is not worth writing the session down for.
            Some(0) if far < self.progress[0].1 + 0.02 => {}
            Some(ix) => {
                let (path, before) = self.progress.remove(ix);
                self.progress.insert(0, (path, before.max(far)));
            }
            None => {
                self.progress.insert(0, (path.to_path_buf(), far));
                self.progress.truncate(PROGRESS_KEPT);
            }
        }
    }
}

/// How long a mark of the rail is for a heading this many levels below the top one.
fn mark_length(depth: u8) -> Pixels {
    match depth {
        0 => px(18.),
        1 => px(14.),
        _ => px(8.),
    }
}

impl Workspace {
    // ------------------------------------------------------------ sections

    /// The heading the reader is under: on a page, the last one at or above
    /// the top of the page; in the editor, the last one at or above the caret.
    pub(super) fn current_section(&self, cx: &App) -> Option<usize> {
        let doc = self.active_doc().filter(|doc| !doc.headings.is_empty())?;
        let line = match self.mode {
            Mode::Edit => doc.editor.read(cx).cursor_position().line as f32,
            Mode::Read => {
                let list = doc.preview.read(cx).list_state().clone();
                crate::sync::preview_top_line(&list, &doc.blocks).unwrap_or(0.)
            }
        };
        // Half a line of grace: a heading that has just touched the top counts.
        doc.headings
            .iter()
            .rposition(|heading| heading.line as f32 <= line + 0.5)
            .or(Some(0))
    }

    /// Go to the n-th heading of the document.
    pub(super) fn go_to_section(
        &mut self,
        nth: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(doc) = self.active_doc() else { return };
        let Some(line) = doc.headings.get(nth).map(|heading| heading.line) else {
            return;
        };
        match self.mode {
            Mode::Edit => self.jump_to_line(line, window, cx),
            Mode::Read => {
                if let Some((item, _)) = doc.blocks.locate(line as f32) {
                    self.move_page_to(
                        ListOffset {
                            item_ix: item,
                            offset_in_item: px(0.),
                        },
                        cx,
                    );
                }
            }
        }
    }

    /// Go to the heading before or after the one the reader is under.
    pub(super) fn step_section(
        &mut self,
        step: isize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(count) = self.active_doc().map(|doc| doc.headings.len()) else {
            return;
        };
        let Some(here) = self.current_section(cx) else {
            return;
        };
        let next = (here as isize + step).clamp(0, count as isize - 1) as usize;
        self.go_to_section(next, window, cx);
    }

    // ------------------------------------------------------------ movement

    /// Go to a line of the document on show: with the caret in the editor,
    /// by moving the page while reading.
    pub(super) fn go_to_line(&mut self, line: u32, window: &mut Window, cx: &mut Context<Self>) {
        self.arrive_at(line, None, window, cx);
    }

    /// Go to a line of the document on show and mark `words` on the page
    /// there: how what the search of all files found is shown. The page of a
    /// file that was only just opened is not ready to be moved, so the move
    /// waits for it.
    pub(super) fn arrive_at(
        &mut self,
        line: u32,
        words: Option<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(doc) = self.active_doc().map(|doc| doc.id) else {
            return;
        };
        match self.mode {
            Mode::Edit => self.jump_to_line(line, window, cx),
            Mode::Read => {
                self.reader.arriving = Some(Arriving {
                    doc,
                    line,
                    words: words.filter(|words| !words.is_empty()),
                    since: Instant::now(),
                });
                // The move is made as the next frame is drawn.
                cx.notify();
            }
        }
    }

    /// Make the move that is waiting, if the page is ready for it. Called as
    /// each frame is drawn, and only then: it asks for the next one.
    pub(super) fn arrive(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(arriving) = &self.reader.arriving else {
            return;
        };
        let doc = self.active_doc().filter(|doc| doc.id == arriving.doc);
        // The block of the page the line is in, once the lines are mapped,
        // and whether the line is far down it. Words can only be marked on a
        // page whose text is there.
        let place = doc.and_then(|doc| {
            let (block, _) = doc.blocks.locate(arriving.line as f32)?;
            let start = doc.blocks.line_at(block, 0.)?;
            let deep = arriving.line as f32 - start > DEEP;
            // A heading just above the block belongs with it: the page
            // starts there, and says what the block is about.
            let above = block.checked_sub(1).filter(|above| {
                let line = doc.blocks.line_at(*above, 0.);
                doc.headings
                    .iter()
                    .any(|heading| Some(heading.line as f32) == line)
            });
            let written = !doc.preview.read(cx).rendered_text().is_empty();
            (arriving.words.is_none() || written).then_some((above.unwrap_or(block), deep))
        });
        let Some((block, deep)) = place else {
            match doc.is_some() && arriving.since.elapsed() < PATIENCE {
                true => window.request_animation_frame(),
                false => self.reader.arriving = None,
            }
            return;
        };
        let Some(arriving) = self.reader.arriving.take() else {
            return;
        };
        // The page goes to the top of the block the line is in, which shows
        // what follows it too. A line far down a long block (a long list, a
        // long listing) would be off the foot of the page: there the words,
        // once marked, are brought into view themselves.
        let marked = arriving
            .words
            .is_some_and(|words| self.find_from_search(&words, arriving.line, deep, window, cx));
        if !(marked && deep) {
            self.move_page_to(
                ListOffset {
                    item_ix: block,
                    offset_in_item: px(0.),
                },
                cx,
            );
        }
    }

    /// Take the page to `target`. Somewhere near, it glides; somewhere far,
    /// gliding would mean a blur of everything in between, so it fades out
    /// and back in at the new place.
    pub(super) fn move_page_to(&mut self, target: ListOffset, cx: &mut Context<Self>) {
        let Some(doc) = self.active_doc() else { return };
        let id = doc.id;
        let list = doc.preview.read(cx).list_state().clone();
        let view = list.viewport_bounds();
        let top = list.logical_scroll_top();
        // How far it is, when the rows on the way have been measured.
        let distance = match list.bounds_for_item(target.item_ix) {
            Some(bounds) => Some(bounds.top() - view.top() + target.offset_in_item),
            None if target.item_ix < top.item_ix => {
                let here = list.scroll_px_offset_for_scrollbar().y;
                list.scroll_to(target);
                let there = list.scroll_px_offset_for_scrollbar().y;
                list.scroll_to(top);
                Some(here - there)
            }
            None => None,
        };
        let near = distance.filter(|far| far.abs() < view.size.height * 2.5);
        self.reader.motion = Some(match near.filter(|_| !cx.reduce_motion()) {
            Some(total) => PageMove::Glide {
                doc: id,
                total,
                done: px(0.),
                exact: Some(target),
                started: Instant::now(),
            },
            None => PageMove::Cut {
                doc: id,
                target,
                jumped: false,
                started: Instant::now(),
            },
        });
        cx.notify();
    }

    /// Scroll the page by `distance`, smoothly: what the keyboard does.
    pub(super) fn scroll_page(&mut self, distance: Pixels, cx: &mut Context<Self>) {
        let Some(doc) = self.active_doc() else { return };
        // What was left of a glide still under way is carried into the new one.
        let left = match &self.reader.motion {
            Some(PageMove::Glide {
                total,
                done,
                doc: id,
                ..
            }) if *id == doc.id => *total - *done,
            _ => px(0.),
        };
        if cx.reduce_motion() {
            doc.preview.read(cx).list_state().scroll_by(distance + left);
            return cx.notify();
        }
        self.reader.motion = Some(PageMove::Glide {
            doc: doc.id,
            total: distance + left,
            done: px(0.),
            exact: None,
            started: Instant::now(),
        });
        cx.notify();
    }

    /// Move the page one frame on, and say how it should be drawn meanwhile:
    /// how faded, and how far out of place.
    fn advance_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> (f32, Pixels) {
        let Some(list) = self
            .active_doc()
            .map(|doc| doc.preview.read(cx).list_state().clone())
        else {
            return (1., px(0.));
        };
        let shown = self.active_doc().map(|doc| doc.id);
        let Some(motion) = &mut self.reader.motion else {
            return (1., px(0.));
        };
        let look = match motion {
            PageMove::Glide { doc, .. } | PageMove::Cut { doc, .. } if Some(*doc) != shown => None,
            PageMove::Glide {
                total,
                done,
                exact,
                started,
                ..
            } => {
                let progress = (started.elapsed().as_secs_f32() / GLIDE.as_secs_f32()).min(1.);
                let reached = *total * motion::ease(progress);
                list.scroll_by(reached - *done);
                *done = reached;
                if progress >= 1.
                    && let Some(exact) = exact.take()
                {
                    list.scroll_to(exact);
                }
                (progress < 1.).then_some((1., px(0.)))
            }
            PageMove::Cut {
                target,
                jumped,
                started,
                ..
            } => {
                let progress = match cx.reduce_motion() {
                    true => 1.,
                    false => (started.elapsed().as_secs_f32() / CUT.as_secs_f32()).min(1.),
                };
                if progress >= CUT_OUT && !*jumped {
                    list.scroll_to(*target);
                    *jumped = true;
                }
                // Out: fading, sinking a little. In: rising into place.
                let (shown, shift) = if progress < CUT_OUT {
                    let out = progress / CUT_OUT;
                    (1. - out, px(-6.) * out)
                } else {
                    let back = motion::ease((progress - CUT_OUT) / (1. - CUT_OUT));
                    (back, px(10.) * (1. - back))
                };
                (progress < 1.).then_some((shown, shift))
            }
        };
        match look {
            Some(look) => {
                window.request_animation_frame();
                look
            }
            None => {
                self.reader.motion = None;
                (1., px(0.))
            }
        }
    }

    /// Note how far the document on the page has been read.
    fn track_reading(&mut self, cx: &mut Context<Self>) {
        let Some(doc) = self.active_doc() else { return };
        let Some(path) = doc.path.clone() else { return };
        let list = doc.preview.read(cx).list_state().clone();
        let count = list.item_count();
        if count == 0 || list.viewport_bounds().size.height <= px(0.) {
            return;
        }
        let far = match list.is_scrolled_to_end() {
            Some(true) => 1.,
            _ => {
                let top = list.logical_scroll_top();
                (top.item_ix as f32 / count as f32).min(0.99)
            }
        };
        self.reader.note(&path, far);
    }

    // ------------------------------------------------------------- drawing

    pub(super) fn render_reader(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = *cx.palette();
        let viewport = window.viewport_size();
        let (shown, shift) = self.advance_page(window, cx);
        self.track_reading(cx);

        let blank = self.active_doc().is_none_or(Doc::blank);
        let page = match self.active_doc().filter(|_| !blank) {
            Some(doc) => self
                .render_preview(
                    doc,
                    viewport.width,
                    Some(super::frame::sheet_width(viewport)),
                    &p,
                    cx,
                )
                .into_any_element(),
            None => self.render_welcome(&p),
        };
        // A document coming on show fades up, rather than cutting in.
        let page = match self.switches {
            0 => page,
            _ if cx.reduce_motion() => page,
            switches => div()
                .size_full()
                .child(page)
                .with_animation(
                    ("page-arrives", switches),
                    Animation::new(motion::PANEL).with_easing(motion::ease),
                    |page, progress| page.opacity(0.15 + 0.85 * progress),
                )
                .into_any_element(),
        };
        let notice = self
            .active_doc()
            .and_then(|doc| Some((doc.id, doc.notice?)))
            .map(|(id, notice)| {
                div()
                    .mt(size::TITLEBAR)
                    .child(self.render_notice(id, notice, &p, cx))
            });

        let rail = (!blank).then(|| self.render_rail(viewport, &p, window, cx));
        let menu = self.render_page_menu(window, cx);
        let panel = self.render_floating_panel(&p, window, cx);

        div()
            .id("page")
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(p.surface_preview)
            .on_mouse_move(cx.listener(|this, _, _, cx| this.reader.wake(cx)))
            .children(notice)
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .top(shift)
                    .opacity(shown)
                    .child(page),
            )
            // The top of the page is what the window is moved by.
            .child(
                div()
                    .id("page-top")
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(size::TITLEBAR)
                    // Windows moves a window itself, from wherever it is told the bar is.
                    .when(cfg!(target_os = "windows"), |top| {
                        top.window_control_area(WindowControlArea::Drag)
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, _| this.reader.grabbed = true),
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, _, _| this.reader.grabbed = false),
                    )
                    .on_mouse_move(cx.listener(|this, _, window, _| {
                        if std::mem::take(&mut this.reader.grabbed) {
                            window.start_window_move();
                        }
                    }))
                    .on_click(|event, window, _| {
                        if event.click_count() == 2 {
                            // What a double click on a bar does is the system's
                            // to say on a Mac; on Linux it fills the screen.
                            match cfg!(target_os = "linux") {
                                true => window.zoom_window(),
                                false => window.titlebar_double_click(),
                            }
                        }
                    }),
            )
            .children(rail)
            .child(menu)
            .children(panel)
            .into_any_element()
    }

    /// What a page shows when there is nothing to read yet.
    fn render_welcome(&self, p: &Palette) -> AnyElement {
        let (hover, strong) = (p.surface_hover, p.text_primary);
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .pb(px(80.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(40.))
                    .mb(px(6.))
                    .rounded(size::RADIUS_LG)
                    .bg(p.surface_sunken)
                    .text_color(p.text_secondary)
                    .child(Icon::new(IconName::BookOpen).size(px(18.))),
            )
            .child(
                div()
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Nothing open yet"),
            )
            .child(
                div()
                    .text_color(p.text_secondary)
                    .child("Open a file, or drop a folder here."),
            )
            .child(
                h_flex()
                    .id("welcome-open")
                    .mt(px(12.))
                    .h(px(30.))
                    .pl(px(12.))
                    .pr(px(7.))
                    .gap(px(8.))
                    .rounded(size::RADIUS_MD)
                    .border_1()
                    .border_color(p.border_strong)
                    .bg(p.surface_raised)
                    .font_weight(FontWeight::MEDIUM)
                    .hover(move |style| style.bg(hover).text_color(strong))
                    .child("Open…")
                    .child(key_cap(shortcut!("O"), p.text_tertiary, p.surface_sunken))
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenFile), cx)),
            )
            .into_any_element()
    }

    /// The side panel sliding over the page from the left, with the page dimmed behind it.
    fn render_floating_panel(
        &mut self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let presence = transition(
            "floating-panel",
            if self.reader.panel_open { 1. } else { 0. },
            motion::glide(motion::PANEL, cx),
            window,
            cx,
        );
        // Drawn from the moment it is asked for, so the keyboard has somewhere to go at once.
        if presence <= 0.001 && !self.reader.panel_open {
            return None;
        }
        let dim = if cx.theme().is_dark() { 0.38 } else { 0.12 };
        let panel = self.render_panel(true, window, cx);
        Some(
            div()
                .absolute()
                .inset_0()
                .child(
                    div()
                        .id("page-scrim")
                        .absolute()
                        .inset_0()
                        .bg(gpui_kit::black().opacity(dim * presence))
                        .occlude()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.reader.panel_open = false;
                            this.focus_document(window, cx);
                            cx.notify();
                        })),
                )
                .child(
                    div()
                        .id("floating-panel")
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(-PANEL_WIDTH * (1. - presence))
                        .w(PANEL_WIDTH)
                        .bg(p.surface_app)
                        .border_r_1()
                        .border_color(p.border_strong)
                        .shadow_lg()
                        .occlude()
                        .child(panel),
                )
                .into_any_element(),
        )
    }

    /// The rail down the right edge: one mark for each heading, the card for
    /// the mark under the pointer, and the arrows to the sections either side.
    fn render_rail(
        &mut self,
        viewport: Size<Pixels>,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let current = self.current_section(cx);
        let Some(doc) = self.active_doc().filter(|doc| doc.headings.len() > 1) else {
            return div().into_any_element();
        };
        let count = doc.headings.len();
        let top_level = doc
            .headings
            .iter()
            .map(|heading| heading.level)
            .min()
            .unwrap_or(1);
        // The marks keep their distance while there is room, and close up when there is not.
        let room = (viewport.height - px(220.)).max(px(120.));
        let pitch = PITCH.min(room / (count - 1) as f32).max(px(3.));
        let span = pitch * (count - 1) as f32;
        let top = ((viewport.height - span) / 2.).round();

        let on_rail = self.reader.on_rail;
        let pointed = self.reader.mark.filter(|_| on_rail);
        let warmth = transition(
            "rail-warmth",
            if on_rail { 1. } else { 0. },
            Transition::new(motion::QUICK).ease(motion::ease),
            window,
            cx,
        );

        // What the card says is worked out once for each mark pointed at.
        if let Some(at) = pointed {
            let stale = self
                .reader
                .card
                .as_ref()
                .is_none_or(|card| card.doc != doc.id || card.at != at);
            if stale {
                let heading = &doc.headings[at];
                let next = doc.headings.get(at + 1).map(|next| next.line);
                let words = doc::words_between(&doc.text, heading.line, next);
                let minutes = words.div_ceil(220).max(1);
                let card = Card {
                    doc: doc.id,
                    at,
                    place: format!("{} OF {count}   ·   {minutes} MIN READ", at + 1).into(),
                    title: heading.title.clone(),
                    about: doc::excerpt(&doc.text, heading.line + 1, next, 150).into(),
                };
                self.reader.card = Some(card);
            }
        }
        let doc = &self.docs[self.active];

        let mut marks = Vec::with_capacity(count);
        for (ix, heading) in doc.headings.iter().enumerate() {
            let (here, under) = (current == Some(ix), pointed == Some(ix));
            let length = transition(
                ElementId::from(("rail-mark", ix)),
                if here || under {
                    px(24.)
                } else {
                    mark_length(heading.level - top_level)
                },
                motion::glide(motion::QUICK, cx),
                window,
                cx,
            );
            let (thick, color) = match (here, under) {
                (true, _) => (px(3.), p.accent),
                (_, true) => (px(3.), p.text_primary),
                // The rail wakes up a little under the pointer.
                _ => (
                    px(2.),
                    p.text_disabled
                        .opacity(if heading.level - top_level > 1 {
                            0.65
                        } else {
                            1.
                        })
                        .blend(p.text_tertiary.opacity(warmth)),
                ),
            };
            marks.push(
                div()
                    .absolute()
                    .right(px(14.))
                    .top(top + pitch * ix as f32 - thick / 2.)
                    .w(length)
                    .h(thick)
                    .rounded(px(1.5))
                    .bg(color),
            );
        }

        let arrow =
            |id: &'static str, icon: IconName, y: Pixels, step: isize, cx: &mut Context<Self>| {
                let (hover, strong) = (p.surface_hover, p.text_primary);
                div()
                    .id(id)
                    .absolute()
                    .right(px(9.))
                    .top(y)
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(26.))
                    .rounded(size::RADIUS_MD)
                    .border_1()
                    .border_color(p.border_strong)
                    .bg(p.surface_raised)
                    .text_color(p.text_secondary)
                    .opacity(warmth)
                    .hover(move |style| style.bg(hover).text_color(strong))
                    .child(Icon::new(icon).size(px(14.)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.step_section(step, window, cx);
                    }))
            };
        let before = arrow("section-before", IconName::ChevronUp, top - px(48.), -1, cx);
        let after = arrow(
            "section-after",
            IconName::ChevronDown,
            top + span + px(22.),
            1,
            cx,
        );

        let card = self.render_card(top, pitch, viewport, p, window, cx);
        // The strip the pointer has to be in: the marks and the arrows either side.
        let strip = div()
            .id("rail")
            .absolute()
            .right_0()
            .top(top - px(56.))
            .w(RAIL)
            .h(span + px(112.))
            .on_hover(cx.listener(|this, over: &bool, _, cx| {
                this.reader.on_rail = *over;
                if !*over {
                    this.reader.mark = None;
                }
                this.reader.wake(cx);
                cx.notify();
            }))
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                let nearest = ((event.position.y - top) / pitch).round();
                let mark = (nearest >= -1. && nearest <= count as f32)
                    .then(|| (nearest.max(0.) as usize).min(count - 1));
                if this.reader.mark != mark {
                    this.reader.mark = mark;
                    cx.notify();
                }
            }))
            .on_click(cx.listener(|this, _, window, cx| {
                if let Some(mark) = this.reader.mark {
                    this.go_to_section(mark, window, cx);
                }
            }));

        div()
            .absolute()
            .inset_0()
            // A quiet backing, so the rail reads as one thing while the pointer is on it.
            .child(
                div()
                    .absolute()
                    .right_0()
                    .top(top - px(14.))
                    .w(RAIL)
                    .h(span + px(28.))
                    .rounded_l(px(10.))
                    .bg(p.surface_sunken.opacity(warmth)),
            )
            .children(marks)
            .child(strip)
            .child(before)
            .child(after)
            .children(card)
            .into_any_element()
    }

    /// The card beside the rail. It follows the pointer from mark to mark and
    /// fades when the pointer leaves, still saying what it last said.
    fn render_card(
        &self,
        top: Pixels,
        pitch: Pixels,
        viewport: Size<Pixels>,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let card = self.reader.card.as_ref()?;
        let pointed = self.reader.on_rail && self.reader.mark == Some(card.at);
        let presence = transition(
            "rail-card",
            if pointed { 1. } else { 0. },
            Transition::new(motion::QUICK).ease(motion::ease),
            window,
            cx,
        );
        if presence <= 0.001 {
            return None;
        }
        let y = (top + pitch * card.at as f32 - px(34.))
            .max(px(12.))
            .min(viewport.height - px(150.));
        let y = transition(
            "rail-card-y",
            y,
            motion::glide(motion::QUICK, cx),
            window,
            cx,
        );
        Some(
            v_flex()
                .absolute()
                .right(RAIL + px(8.) - px(6.) * (1. - presence))
                .top(y)
                .w(CARD_WIDTH)
                .max_w(viewport.width - RAIL - px(24.))
                .px(px(16.))
                .pt(px(13.))
                .pb(px(14.))
                .gap(px(5.))
                .rounded(size::RADIUS_LG)
                .border_1()
                .border_color(p.border_strong)
                .bg(p.surface_raised)
                .shadow_lg()
                .opacity(presence)
                .child(
                    div()
                        .text_size(px(10.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(p.text_tertiary)
                        .child(card.place.clone()),
                )
                .child(
                    div()
                        .text_size(px(14.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(card.title.clone()),
                )
                .when(!card.about.is_empty(), |body| {
                    body.child(
                        div()
                            .text_size(px(12.5))
                            .line_height(px(19.))
                            .text_color(p.text_secondary)
                            .child(card.about.clone()),
                    )
                }),
        )
    }
}
