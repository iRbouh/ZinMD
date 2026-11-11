//! The window that searches the text of every file (Shift Cmd F).
//!
//! It is drawn to the design on the "Search" page of `design/zinMD-UI`: a
//! field to type in, then what was found, a file at a time with the best
//! first and the page being read on top, then a foot that counts it and
//! names the keys. Choosing a row opens the file at that place, with the
//! words marked on the page.
//!
//! The window takes the mouse: a click or a scroll over it never reaches the
//! page behind. The keys work from the field: the arrows move through the
//! rows, Return opens one, Escape closes the window.

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui_kit::assets::IconName;
use gpui_kit::base::input::{Escape, MoveDown, MoveUp};
use gpui_kit::base::transition;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme as _, Icon, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::Workspace;
use super::menu::key_cap;
use crate::search::{self, Rank};
use crate::theme::{ActivePalette as _, Palette, size};
use crate::{files, motion};

/// How wide the window is, and how far from the top of the app's window.
const WIDTH: Pixels = px(640.);
const FROM_TOP: Pixels = px(96.);
/// How tall the field, a row and the foot are.
const FIELD: Pixels = px(54.);
const ROW: Pixels = px(32.);
const FOOT: Pixels = px(38.);
/// The most room the rows are given before they scroll.
const ROWS: Pixels = px(442.);
/// How big the words of a row are, and the smaller words beside them.
const WORDS: Pixels = px(13.);
const SMALL: Pixels = px(11.5);
/// How many rows Page Up and Page Down move by.
const PAGE: isize = 8;

/// One row of the window.
enum Row {
    /// The file the rows under it were found in: its name, and the folders
    /// on the way to it. `here` is the page being read.
    Title {
        name: SharedString,
        way: SharedString,
        here: bool,
    },
    /// A part of a file that was found.
    Found(Rc<Found>),
}

/// What is shown of a part of a file that was found, and where it is.
struct Found {
    path: PathBuf,
    line: u32,
    /// Its best line, or its heading.
    text: SharedString,
    /// The parts of `text` that were searched for.
    marks: Vec<Range<usize>>,
    /// The heading it is under, when the line is not the heading.
    heading: SharedString,
    in_heading: bool,
}

pub(super) struct SearchWindow {
    field: Entity<InputState>,
    rows: Vec<Row>,
    /// The row the keys are on: always a place that was found.
    chosen: Option<usize>,
    /// The words the rows on show were found for.
    sought: SharedString,
    /// What the foot says: how much was found.
    told: SharedString,
    scroll: ScrollHandle,
    _typed: Subscription,
}

impl Workspace {
    /// Open the window that searches the text of every file. Words being
    /// looked for on the page are looked for everywhere.
    pub(super) fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let words = self.words_being_found(cx);
        self.close_find(window, cx);
        self.close_palette(window, cx);
        // It is already there: back to its field.
        if let Some(search) = self.search.as_ref().filter(|_| self.search_open) {
            search.field.update(cx, |field, cx| {
                field.focus(window, cx);
                field.select_all(window, cx);
            });
            return;
        }
        let field = cx.new(|cx| InputState::new(window, cx).placeholder("Search in every file"));
        field.update(cx, |field, cx| field.focus(window, cx));
        let typed = cx.subscribe_in(
            &field,
            window,
            |this, field, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    let text = field.read(cx).value().to_string();
                    this.search_files(text, cx);
                }
                // Return opens the row the keys are on.
                InputEvent::PressEnter { .. } => {
                    let chosen = this.search.as_ref().and_then(|search| search.chosen);
                    if let Some(chosen) = chosen.filter(|_| this.searching()) {
                        this.open_found(chosen, window, cx);
                    }
                }
                _ => {}
            },
        );
        self.search = Some(SearchWindow {
            field,
            rows: Vec::new(),
            chosen: None,
            sought: "".into(),
            told: "".into(),
            scroll: ScrollHandle::new(),
            _typed: typed,
        });
        self.search_open = true;
        if let Some(words) = words {
            self.type_in_search(&words, window, cx);
        }
        cx.notify();
    }

    /// Put the window away. It fades as it goes.
    pub(super) fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.search_open {
            return;
        }
        self.search_open = false;
        self.search_stop.store(true, Ordering::Relaxed);
        self.focus_document(window, cx);
        cx.notify();
    }

    /// Whether the window is open.
    pub(super) fn searching(&self) -> bool {
        self.search_open && self.search.is_some()
    }

    /// Put words in the field as if they had been typed.
    pub(super) fn type_in_search(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(search) = &self.search else { return };
        let text = text.to_string();
        search
            .field
            .update(cx, |field, cx| field.set_value(text.clone(), window, cx));
        self.search_files(text, cx);
    }

    /// Search the files for what was typed, off the main thread, and show
    /// what was found.
    fn search_files(&mut self, query: String, cx: &mut Context<Self>) {
        // The search before this one is of no use any more.
        self.search_stop.store(true, Ordering::Relaxed);
        let stop = Arc::new(AtomicBool::new(false));
        self.search_stop = stop.clone();
        let files = self.files_to_search();
        let here = self.active_doc().and_then(|doc| doc.path.clone());
        self._search = cx.spawn(async move |this, cx| {
            // Let a burst of typing settle: only the last query is worth running.
            cx.background_executor()
                .timer(std::time::Duration::from_millis(80))
                .await;
            let sought = SharedString::from(query.trim().to_string());
            let hits = cx
                .background_executor()
                .spawn(async move { search::run(&files, &query, 200, &stop) })
                .await;
            this.update(cx, |this, cx| {
                if !this.searching() {
                    return;
                }
                // Nearly-right answers are only given when nothing is right.
                let close = hits.first().is_some_and(|hit| hit.rank == Rank::Close);
                // One group a file. The hits come best first, so the files do
                // too: the one with the best answer leads. The page being read
                // goes before them all.
                let mut files: Vec<(PathBuf, Vec<search::Hit>)> = Vec::new();
                let mut place: HashMap<PathBuf, usize> = HashMap::new();
                for hit in hits {
                    let at = *place.entry(hit.path.clone()).or_insert_with(|| {
                        files.push((hit.path.clone(), Vec::new()));
                        files.len() - 1
                    });
                    files[at].1.push(hit);
                }
                if let Some(at) = files
                    .iter()
                    .position(|(path, _)| Some(path) == here.as_ref())
                {
                    let page = files.remove(at);
                    files.insert(0, page);
                }
                let (places, count) = (
                    files.iter().map(|(_, hits)| hits.len()).sum::<usize>(),
                    files.len(),
                );
                let mut rows = Vec::with_capacity(places + count);
                for (path, hits) in files {
                    let is_here = Some(&path) == here.as_ref();
                    let (name, way) = match is_here {
                        true => ("On this page".into(), "".into()),
                        false => this.name_and_way(&path),
                    };
                    rows.push(Row::Title {
                        name,
                        way,
                        here: is_here,
                    });
                    rows.extend(hits.into_iter().map(|hit| {
                        Row::Found(Rc::new(Found {
                            path: path.clone(),
                            line: hit.line,
                            text: hit.text.into(),
                            marks: hit.marks,
                            heading: hit.heading.into(),
                            in_heading: hit.in_heading,
                        }))
                    }));
                }
                let Some(search) = this.search.as_mut() else {
                    return;
                };
                // The keys start on the best answer, at the top of the rows.
                search.chosen = rows.iter().position(|row| matches!(row, Row::Found(_)));
                search.rows = rows;
                search.scroll.set_offset(point(px(0.), px(0.)));
                search.sought = sought;
                search.told = match (close, places, count) {
                    (_, 0, _) => "".into(),
                    (true, ..) => "Nothing as typed. These are close.".into(),
                    (_, 1, _) => "1 place".into(),
                    (_, places, 1) => format!("{places} places in 1 file").into(),
                    (_, places, files) => format!("{places} places in {files} files").into(),
                };
                cx.notify();
            })
            .ok();
        });
    }

    /// A file as the panel names it, and the folders on the way to it from
    /// the folder of the panel it is in: `overview` and `docs / api`. A long
    /// way is said by its end, which is what tells two files apart.
    fn name_and_way(&self, path: &Path) -> (SharedString, SharedString) {
        const STEPS: usize = 3;
        let above = path.parent().unwrap_or(Path::new(""));
        let mut steps: Vec<String> = match self.folder_of(path) {
            Some(folder) => {
                let folder = &self.folders[folder];
                let inside = above.strip_prefix(&folder.root).unwrap_or(Path::new(""));
                std::iter::once(folder.name.to_string())
                    .chain(
                        inside
                            .iter()
                            .map(|step| step.to_string_lossy().into_owned()),
                    )
                    .collect()
            }
            None => above
                .iter()
                .map(|step| step.to_string_lossy().into_owned())
                .collect(),
        };
        if steps.len() > STEPS {
            steps.drain(..steps.len() - STEPS);
            steps.insert(0, "…".into());
        }
        (files::display_name(path).into(), steps.join(" / ").into())
    }

    /// A key was pressed while the window is open: the arrows move through
    /// the rows, Return opens the one they are on, Escape closes the window.
    /// `true` when the key was the window's.
    pub(super) fn search_key(
        &mut self,
        key: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.searching() || key.modifiers.modified() {
            return false;
        }
        match key.key.as_str() {
            "escape" => self.close_search(window, cx),
            "down" => self.move_in_search(1, cx),
            "up" => self.move_in_search(-1, cx),
            "pagedown" => self.move_in_search(PAGE, cx),
            "pageup" => self.move_in_search(-PAGE, cx),
            "enter" => {
                let chosen = self.search.as_ref().and_then(|search| search.chosen);
                if let Some(chosen) = chosen {
                    self.open_found(chosen, window, cx);
                }
            }
            _ => return false,
        }
        true
    }

    /// Move the row the keys are on by `by` places that were found, stopping
    /// at the first and the last, and keep it in view.
    fn move_in_search(&mut self, by: isize, cx: &mut Context<Self>) {
        let Some(search) = self.search.as_mut() else {
            return;
        };
        let found: Vec<usize> = search
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| matches!(row, Row::Found(_)))
            .map(|(at, _)| at)
            .collect();
        let Some(last) = found.len().checked_sub(1) else {
            return;
        };
        let now = search
            .chosen
            .and_then(|chosen| found.iter().position(|at| *at == chosen));
        let next = match now {
            Some(now) => (now as isize + by).clamp(0, last as isize) as usize,
            None => 0,
        };
        search.chosen = Some(found[next]);
        // The first place of a file is shown with the name of the file.
        let above = found[next].saturating_sub(1);
        let show = match search.rows.get(above) {
            Some(Row::Title { .. }) if by < 0 => above,
            _ => found[next],
        };
        search.scroll.scroll_to_item(show);
        cx.notify();
    }

    /// Open the file a row was found in, at its place, with the words that
    /// were searched for marked on the page.
    fn open_found(&mut self, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(search) = &self.search else { return };
        let Some(Row::Found(found)) = search.rows.get(row) else {
            return;
        };
        let (found, sought) = (found.clone(), search.sought.clone());
        self.close_search(window, cx);
        self.open(found.path.clone(), window, cx);
        self.arrive_at(found.line, Some(sought), window, cx);
    }

    /// Choose the n-th place that was found, counting from 0. For scripted
    /// runs.
    #[cfg(feature = "shot")]
    pub(super) fn debug_pick_found(
        &mut self,
        nth: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let row = self.search.as_ref().and_then(|search| {
            search
                .rows
                .iter()
                .enumerate()
                .filter(|(_, row)| matches!(row, Row::Found(_)))
                .map(|(at, _)| at)
                .nth(nth)
        });
        if let Some(row) = row {
            self.open_found(row, window, cx);
        }
    }

    /// The window, over the page, while it is open or on its way out.
    pub(super) fn render_search_window(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let presence = transition(
            "search-window",
            if self.search_open { 1. } else { 0. },
            motion::glide(motion::QUICK, cx),
            window,
            cx,
        );
        if presence <= 0.001 {
            if !self.search_open {
                self.search = None;
            }
            return None;
        }
        let p = *cx.palette();
        let dark = cx.theme().is_dark();
        let viewport = window.viewport_size();
        let search = self.search.as_ref()?;
        let typed = !search.field.read(cx).value().trim().is_empty();

        // 1. The field: taller than a row, with words to read back at a glance.
        let field = h_flex()
            .flex_none()
            .h(FIELD)
            .px(px(18.))
            .gap(px(12.))
            .border_b_1()
            .border_color(p.border_raised)
            .child(
                Icon::new(IconName::Search)
                    .size(px(17.))
                    .text_color(p.text_tertiary),
            )
            .child(
                div().flex_1().min_w_0().child(
                    Input::new(&search.field)
                        .appearance(false)
                        .bordered(false)
                        .px_0()
                        .text_size(px(16.)),
                ),
            );

        // 2. What was found: a file, then its places, each a row.
        let mut rows = Vec::with_capacity(search.rows.len());
        for (at, row) in search.rows.iter().enumerate() {
            rows.push(match row {
                Row::Title { name, way, here } => title_row(at, name, way, *here, &p),
                Row::Found(found) => found_row(at, found, search.chosen == Some(at), &p)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.open_found(at, window, cx);
                    }))
                    .into_any_element(),
            });
        }
        // The rows scroll once they are taller than their room, which a
        // short window makes smaller.
        let room = ROWS.min(viewport.height - FROM_TOP - FIELD - FOOT - px(48.));
        let more_below = {
            let (at, most) = (search.scroll.offset().y, search.scroll.max_offset().y);
            -at < most - px(1.)
        };
        let list = match search.rows.is_empty() {
            // Said in place of the rows when there are none.
            true => div()
                .px(px(18.))
                .py(px(16.))
                .text_size(WORDS)
                .text_color(p.text_tertiary)
                .child(match typed && !search.sought.is_empty() {
                    true => "Nothing found.",
                    false => "Type words to look for in every file of your folders.",
                })
                .into_any_element(),
            false => div()
                .relative()
                .child(
                    v_flex()
                        .id("search-rows")
                        .max_h(room.max(ROW * 3.))
                        .overflow_y_scroll()
                        .track_scroll(&search.scroll)
                        .pt(px(8.))
                        .pb(px(10.))
                        .px(px(8.))
                        .gap(px(2.))
                        .children(rows),
                )
                // Rows going out at the foot fade, rather than being cut.
                .when(more_below, |list| {
                    list.child(
                        div()
                            .absolute()
                            .bottom_0()
                            .left_0()
                            .right_0()
                            .h(px(18.))
                            .bg(linear_gradient(
                                180.,
                                linear_color_stop(p.surface_raised.opacity(0.), 0.),
                                linear_color_stop(p.surface_raised, 1.),
                            )),
                    )
                })
                .into_any_element(),
        };

        // 3. The foot: how much was found, and the keys.
        let foot = (!search.rows.is_empty()).then(|| {
            let hint = |keys: &'static str, does: &'static str| {
                h_flex()
                    .flex_none()
                    .gap(px(6.))
                    .child(key_cap(keys, p.text_secondary, p.surface_sunken))
                    .child(does)
            };
            h_flex()
                .flex_none()
                .h(FOOT)
                .pl(px(18.))
                .pr(px(16.))
                .justify_between()
                .border_t_1()
                .border_color(p.border_raised)
                .text_size(SMALL)
                .text_color(p.text_tertiary)
                .child(div().min_w_0().truncate().child(search.told.clone()))
                .child(
                    h_flex()
                        .flex_none()
                        .gap(px(14.))
                        .child(hint("↑↓", "move"))
                        .child(hint("↵", "open"))
                        .child(hint("esc", "close")),
                )
        });

        let panel = v_flex()
            .id("search-window")
            .w(WIDTH)
            .max_w(relative(0.92))
            .mt(px(-8.) * (1. - presence))
            .rounded(size::RADIUS_LG)
            .border_1()
            .border_color(p.border_strong)
            .bg(p.surface_raised)
            .shadow_lg()
            .overflow_hidden()
            // A click inside the window is not a click beside it.
            .on_click(|_, _, cx| cx.stop_propagation())
            // In the field the arrows belong to the rows, not to the caret,
            // and Escape to the window.
            .capture_action(cx.listener(|this, _: &MoveUp, _, cx| {
                this.move_in_search(-1, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &MoveDown, _, cx| {
                this.move_in_search(1, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &Escape, window, cx| {
                this.close_search(window, cx);
                cx.stop_propagation();
            }))
            .child(field)
            .child(list)
            .children(foot);

        Some(
            div()
                .id("search-backdrop")
                .absolute()
                .inset_0()
                .flex()
                .justify_center()
                .items_start()
                .pt(FROM_TOP)
                .opacity(presence)
                .bg(gpui_kit::black().opacity(if dark { 0.35 } else { 0.12 }))
                // The window takes the mouse: nothing behind it is clicked,
                // selected or scrolled while it is open.
                .occlude()
                .on_scroll(|_, _, cx| cx.stop_propagation())
                // A click beside the window closes it.
                .on_click(cx.listener(|this, _, window, cx| this.close_search(window, cx)))
                .child(panel),
        )
    }
}

/// The row that says which file the rows under it were found in: its sign,
/// its name, then, fainter, the folders on the way to it. It is said, not
/// chosen, and a file after the first stands a little apart from the one
/// above.
fn title_row(
    at: usize,
    name: &SharedString,
    way: &SharedString,
    here: bool,
    p: &Palette,
) -> AnyElement {
    let first = at == 0;
    h_flex()
        .flex_none()
        .h(if first { px(28.) } else { px(38.) })
        .when(!first, |row| row.pt(px(10.)))
        .px(px(10.))
        .gap(px(8.))
        .child(sign(
            Some(if here {
                IconName::BookOpen
            } else {
                IconName::FileText
            }),
            p,
        ))
        .child(
            div()
                .flex_none()
                .max_w(relative(0.6))
                .truncate()
                .text_size(WORDS)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(p.text_primary)
                .child(name.clone()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(SMALL)
                .text_color(p.text_tertiary)
                .child(way.clone()),
        )
        .into_any_element()
}

/// The row of a part of a file that was found: its best line, with what was
/// searched for standing out, and at the end of the row the heading it is
/// under. A heading that was found is the row itself, with its sign.
fn found_row(at: usize, found: &Rc<Found>, chosen: bool, p: &Palette) -> Stateful<Div> {
    let stands_out = HighlightStyle {
        color: Some(p.accent_text),
        font_weight: Some(FontWeight::SEMIBOLD),
        ..Default::default()
    };
    let marks = found.marks.iter().map(|mark| (mark.clone(), stands_out));
    let hover = p.surface_hover;
    h_flex()
        .id(("found", at))
        .flex_none()
        .h(ROW)
        .px(px(10.))
        .gap(px(8.))
        .rounded(px(6.))
        .text_size(WORDS)
        .text_color(p.text_primary)
        .cursor_pointer()
        .map(|row| match chosen {
            true => row.bg(p.surface_selected),
            false => row.hover(move |style| style.bg(hover)),
        })
        .child(sign(found.in_heading.then_some(IconName::Hash), p))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .when(found.in_heading, |line| {
                    line.font_weight(FontWeight::MEDIUM)
                })
                .child(StyledText::new(found.text.clone()).with_highlights(marks)),
        )
        .when(!found.heading.is_empty(), |row| {
            row.child(
                div()
                    .flex_none()
                    .max_w(px(190.))
                    .truncate()
                    .text_size(SMALL)
                    .text_color(p.text_tertiary)
                    .child(found.heading.clone()),
            )
        })
}

/// The column at the start of a row that says what the row is: a file, a
/// heading, or, with no sign, a line. It is as wide either way, so that the
/// words of every row begin at the same place.
fn sign(icon: Option<IconName>, p: &Palette) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(14.))
        .children(icon.map(|icon| Icon::new(icon).size(px(14.)).text_color(p.text_tertiary)))
}
