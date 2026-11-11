//! The command palette (Cmd K): type to jump to a heading of the open
//! document, open any Markdown file of the folder, or run a command.
//!
//! The same list searches the text of every file (Shift Cmd F). What is found
//! is shown a part of a file at a time, the best first, under the file it is
//! in, with the page being read on top. Choosing one opens the file there,
//! with the words marked on the page.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui_kit::assets::IconName;
use gpui_kit::component::command::{Command, CommandGroup, CommandItem, CommandState};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, IndexPath, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::*;
use crate::search::{self, Rank};
use crate::theme::{ActivePalette as _, Palette, size};

/// What choosing a row does. Commands are plain actions, dispatched by the
/// palette itself; the other rows need the workspace.
enum Target {
    Heading(u32),
    File(PathBuf),
    /// A line of a file, found by searching the folder.
    Line(PathBuf, u32),
    Command,
    /// A row that only says something: the name of a file above what was
    /// found in it.
    Title,
}

/// One thing the palette offers.
enum Choice {
    /// An icon, words, and what it does when it is a command.
    Plain(IconName, SharedString, Option<Box<dyn Action>>),
    /// A part of a file that the search found.
    Found(Rc<Found>),
    /// The file the rows under it were found in: its name, and where it is.
    Title(SharedString, SharedString),
}

/// What is shown of a part of a file that was found.
struct Found {
    /// Its best line, or its heading.
    text: SharedString,
    /// The parts of `text` that were searched for.
    marks: Vec<std::ops::Range<usize>>,
    /// The heading it is under, when the line is not the heading.
    heading: SharedString,
    in_heading: bool,
}

/// The choices shown under one title.
type Group = (SharedString, Vec<Choice>);

pub(super) struct CommandPalette {
    state: Entity<CommandState>,
    /// One list of targets per group, in the order the groups are shown.
    targets: Rc<Vec<Vec<Target>>>,
    groups: Vec<Group>,
    /// Whether this palette searches the text of the folder's files, rather
    /// than offering headings, files and commands.
    searching: bool,
    /// The words the rows on show were found for.
    sought: SharedString,
    /// What is said under the rows: how much was found.
    told: SharedString,
}

impl CommandPalette {
    /// Put text in the palette's field as if it had been typed.
    pub(super) fn type_query(&self, text: &str, window: &mut Window, cx: &mut App) {
        let text = text.to_string();
        self.state
            .update(cx, |state, cx| state.set_query(text, window, cx));
    }
}

impl Workspace {
    pub(super) fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut targets = Vec::new();
        let mut groups = Vec::new();

        if let Some(doc) = self.active_doc().filter(|doc| !doc.headings.is_empty()) {
            let rows = doc
                .headings
                .iter()
                .map(|heading| Choice::Plain(IconName::Hash, heading.title.clone(), None))
                .collect();
            targets.push(
                doc.headings
                    .iter()
                    .map(|heading| Target::Heading(heading.line))
                    .collect(),
            );
            groups.push(("Headings".into(), rows));
        }

        let files = self.all_files();
        if !files.is_empty() {
            let rows = files
                .iter()
                .map(|path| Choice::Plain(IconName::FileText, self.place_of(path), None))
                .collect();
            targets.push(files.into_iter().map(Target::File).collect());
            groups.push(("Files".into(), rows));
        }

        let commands: Vec<(IconName, &str, Box<dyn Action>)> = vec![
            (
                IconName::BookOpen,
                "Read, or edit: switch between the page and the editor",
                Box::new(ToggleMode),
            ),
            (IconName::Search, "Search files", Box::new(FocusSearch)),
            (IconName::Pencil, "View: Editor", Box::new(ViewEditor)),
            (IconName::Columns2, "View: Split", Box::new(ViewSplit)),
            (IconName::Eye, "View: Preview", Box::new(ViewPreview)),
            (
                IconName::PanelLeft,
                "Show or hide the side panel",
                Box::new(ToggleSidebar),
            ),
            (
                IconName::Link2,
                "Turn scroll sync on or off",
                Box::new(ToggleScrollSync),
            ),
            (
                IconName::Moon,
                "Switch between light and dark",
                Box::new(ToggleTheme),
            ),
            (IconName::Plus, "New document", Box::new(NewFile)),
            (
                IconName::FolderOpen,
                "Open a file or folder…",
                Box::new(OpenFile),
            ),
            (
                IconName::TextSearch,
                "Find in all files…",
                Box::new(FindInFolder),
            ),
            (IconName::Share, "Export as HTML…", Box::new(ExportHtml)),
            (
                IconName::Type,
                "Font: JetBrains Mono",
                Box::new(UseJetBrainsMono),
            ),
            (IconName::Type, "Font: Fira Code", Box::new(UseFiraCode)),
            (IconName::Type, "Font: Menlo", Box::new(UseMenlo)),
            (IconName::AArrowUp, "Bigger text", Box::new(FontBigger)),
            (IconName::AArrowDown, "Smaller text", Box::new(FontSmaller)),
            (
                IconName::Contrast,
                match cx.global::<crate::settings::Settings>().contrast {
                    true => "Black and white: back to the usual colours",
                    false => "Black and white: sharper words, with no glare",
                },
                Box::new(ToggleContrast),
            ),
            (
                IconName::CircleCheck,
                "Check for updates",
                Box::new(CheckForUpdates),
            ),
            (
                IconName::CircleCheck,
                match cx.global::<crate::settings::Settings>().updates {
                    true => "Stop looking for updates by itself",
                    false => "Look for updates by itself",
                },
                Box::new(ToggleUpdates),
            ),
        ];
        targets.push(commands.iter().map(|_| Target::Command).collect());
        groups.push((
            "Commands".into(),
            commands
                .into_iter()
                .map(|(icon, label, action)| Choice::Plain(icon, label.into(), Some(action)))
                .collect(),
        ));

        let state = cx.new(|cx| CommandState::new(window, cx));
        state.update(cx, |state, cx| state.focus(window, cx));
        self.palette = Some(CommandPalette {
            state,
            targets: Rc::new(targets),
            groups,
            searching: false,
            sought: "".into(),
            told: "".into(),
        });
        cx.notify();
    }

    /// Open the palette to search the text of every file in the folder. Words
    /// being looked for on the page are looked for everywhere.
    pub(super) fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let words = self.words_being_found(cx);
        self.close_find(window, cx);
        let state = cx.new(|cx| CommandState::new(window, cx));
        state.update(cx, |state, cx| state.focus(window, cx));
        let palette = CommandPalette {
            state,
            targets: Rc::new(Vec::new()),
            groups: Vec::new(),
            searching: true,
            sought: "".into(),
            told: "".into(),
        };
        if let Some(words) = words {
            palette.type_query(&words, window, cx);
        }
        self.palette = Some(palette);
        cx.notify();
    }

    /// Search the files for what was typed, off the main thread, and show
    /// what was found.
    fn search_folder(&mut self, query: String, cx: &mut Context<Self>) {
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
                if !this
                    .palette
                    .as_ref()
                    .is_some_and(|palette| palette.searching)
                {
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
                let (mut targets, mut groups) = (Vec::new(), Vec::new());
                for (path, hits) in files {
                    // The first row of each file says which file it is.
                    let (name, way) = match Some(&path) == here.as_ref() {
                        true => ("On this page".into(), "".into()),
                        false => this.name_and_way(&path),
                    };
                    let lines = hits.iter().map(|hit| Target::Line(path.clone(), hit.line));
                    targets.push(std::iter::once(Target::Title).chain(lines).collect());
                    let rows = hits.into_iter().map(|hit| {
                        Choice::Found(Rc::new(Found {
                            text: hit.text.into(),
                            marks: hit.marks,
                            heading: hit.heading.into(),
                            in_heading: hit.in_heading,
                        }))
                    });
                    let title = Choice::Title(name, way);
                    groups.push(("".into(), std::iter::once(title).chain(rows).collect()));
                }
                if let Some(palette) = this.palette.as_mut() {
                    palette.targets = Rc::new(targets);
                    palette.groups = groups;
                    palette.sought = sought;
                    palette.told = match (close, places, count) {
                        (_, 0, _) => "".into(),
                        (true, ..) => "Nothing as typed. These are close.".into(),
                        (_, 1, _) => "1 place".into(),
                        (_, places, 1) => format!("{places} places in 1 file").into(),
                        (_, places, files) => format!("{places} places in {files} files").into(),
                    };
                }
                cx.notify();
            })
            .ok();
        });
    }

    /// Where a file is, said from the folder of the panel it is in: `docs/api/overview.md`.
    fn place_of(&self, path: &Path) -> SharedString {
        let place = match self.folder_of(path) {
            Some(folder) => {
                let folder = &self.folders[folder];
                let inside = path.strip_prefix(&folder.root).unwrap_or(path);
                format!("{}/{}", folder.name, inside.to_string_lossy())
            }
            None => path.to_string_lossy().into_owned(),
        };
        place.into()
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

    pub(super) fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(palette) = self.palette.take() {
            // It stays on screen a moment longer, fading, with nothing left to click.
            self.palette_leaving = Some((palette, std::time::Instant::now()));
            // Typing goes back to the document.
            self.focus_document(window, cx);
            cx.notify();
        }
    }

    pub(super) fn run_palette_target(
        &mut self,
        at: IndexPath,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(palette) = self.palette.as_ref() else {
            return;
        };
        let (targets, sought) = (palette.targets.clone(), palette.sought.clone());
        self.close_palette(window, cx);
        match targets.get(at.section).and_then(|group| group.get(at.row)) {
            Some(Target::Heading(line)) => self.go_to_line(*line, window, cx),
            Some(Target::File(path)) => self.open(path.clone(), window, cx),
            // What was searched for is marked on the page it was found in.
            Some(Target::Line(path, line)) => {
                self.open(path.clone(), window, cx);
                self.arrive_at(*line, Some(sought), window, cx);
            }
            Some(Target::Command | Target::Title) | None => {}
        }
    }

    /// The palette floating over the window, when it is open.
    pub(super) fn render_palette(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        // A palette that was just closed is drawn a little longer, fading away.
        let leaving = self.palette.is_none();
        let fade = match &self.palette_leaving {
            Some((_, since)) if leaving => {
                1. - crate::motion::progress(*since, crate::motion::HOVER, cx)
            }
            _ => 1.,
        };
        if leaving && fade <= 0. {
            self.palette_leaving = None;
        }
        let palette = match &self.palette {
            Some(palette) => palette,
            None => &self.palette_leaving.as_ref()?.0,
        };
        if leaving {
            window.request_animation_frame();
        }
        let p = *cx.palette();
        let (confirm, cancel, dismiss) = (cx.weak_entity(), cx.weak_entity(), cx.weak_entity());

        let searching = palette.searching;
        let query = cx.weak_entity();
        let mut command = Command::new(&palette.state)
            .placeholder(if searching {
                "Search the text of every file in your folders"
            } else {
                "Search headings, files and commands"
            })
            // When searching, the rows are already the answer: do not filter them again.
            .filterable(!searching)
            .on_query(move |text, _, cx| {
                if searching {
                    let text = text.to_string();
                    query
                        .update(cx, |this, cx| this.search_folder(text, cx))
                        .ok();
                }
            })
            // Rows that were found are given more room than commands are.
            .max_h(px(if searching { 480. } else { 420. }))
            .on_confirm(move |at, window, cx| {
                confirm
                    .update(cx, |this, cx| this.run_palette_target(at, window, cx))
                    .ok();
            })
            .on_cancel(move |window, cx| {
                cancel
                    .update(cx, |this, cx| this.close_palette(window, cx))
                    .ok();
            });
        for (title, rows) in &palette.groups {
            let items = rows.iter().map(|row| match row {
                Choice::Plain(icon, label, action) => {
                    let item = CommandItem::new().icon(*icon).label(label.clone());
                    match action {
                        Some(action) => item.action(action.boxed_clone()),
                        None => item,
                    }
                }
                Choice::Found(found) => found_row(found.clone(), p),
                Choice::Title(name, way) => title_row(name.clone(), way.clone(), p),
            });
            let group = match title.is_empty() {
                true => CommandGroup::new(),
                false => CommandGroup::new().label(title.clone()),
            };
            command = command.group(group.items(items));
        }
        if !palette.told.is_empty() {
            let told = palette.told.clone();
            command = command.footer(move |_, _, _| {
                div()
                    .px(px(18.))
                    .py(px(9.))
                    .border_t_1()
                    .border_color(p.border_raised)
                    .text_size(size::FONT_UI_SMALL)
                    .text_color(p.text_tertiary)
                    .child(told.clone())
            });
        }

        Some(
            div()
                .id("palette-backdrop")
                .opacity(fade)
                .absolute()
                .inset_0()
                .flex()
                .justify_center()
                .items_start()
                .pt(px(96.))
                .bg(gpui_kit::black().opacity(if cx.theme().is_dark() { 0.35 } else { 0.12 }))
                // A click beside the palette closes it; one inside it does not reach here.
                .on_click(move |_, window, cx| {
                    dismiss
                        .update(cx, |this, cx| this.close_palette(window, cx))
                        .ok();
                })
                .child(
                    div()
                        .id("palette")
                        .w(px(if searching { 660. } else { 600. }))
                        .max_w(relative(0.9))
                        .rounded(size::RADIUS_LG)
                        .border_1()
                        .border_color(p.border_strong)
                        .bg(p.surface_raised)
                        .shadow_lg()
                        .overflow_hidden()
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .child(command)
                        // It arrives: fades in while settling down the last few points.
                        // With reduced motion the animation is over as it starts.
                        .with_animation(
                            "palette-arrives",
                            Animation::new(if cx.reduce_motion() {
                                std::time::Duration::from_millis(1)
                            } else {
                                crate::motion::QUICK
                            })
                            .with_easing(crate::motion::ease),
                            |panel, progress| {
                                panel.opacity(progress).mt(px(-10.) * (1. - progress))
                            },
                        ),
                ),
        )
    }
}

/// The row of a part of a file that was found: its best line, with what was
/// searched for standing out, and at the end of the row the heading it is
/// under. A heading that was found is the row itself.
fn found_row(found: Rc<Found>, p: Palette) -> CommandItem {
    CommandItem::new()
        .label(found.text.clone())
        .child(move |_, _| {
            let stands_out = HighlightStyle {
                color: Some(p.accent_text),
                font_weight: Some(FontWeight::BOLD),
                ..Default::default()
            };
            let marks = found.marks.iter().map(|mark| (mark.clone(), stands_out));
            let icon = match found.in_heading {
                true => IconName::Hash,
                false => IconName::TextSearch,
            };
            h_flex()
                .flex_1()
                .min_w_0()
                // Room above and below, and at the sides: these are lines to
                // read, not names to pick from.
                .py(px(4.))
                .px(px(4.))
                .gap(px(10.))
                .items_center()
                .child(Icon::new(icon).size_4().text_color(p.text_tertiary))
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
                            .text_size(size::FONT_UI_SMALL)
                            .text_color(p.text_tertiary)
                            .child(found.heading.clone()),
                    )
                })
        })
}

/// The row that says which file the rows under it were found in: its name,
/// then, fainter, the folders on the way to it. It is said, not chosen, and
/// stands a little apart from the file above.
fn title_row(name: SharedString, way: SharedString, p: Palette) -> CommandItem {
    CommandItem::new().disabled(true).child(move |_, _| {
        h_flex()
            .flex_1()
            .min_w_0()
            .pt(px(10.))
            .px(px(4.))
            .gap(px(8.))
            .items_baseline()
            .child(
                div()
                    .flex_none()
                    .max_w(relative(0.6))
                    .truncate()
                    .text_size(size::FONT_UI_SMALL)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(p.text_secondary)
                    .child(name.clone()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(11.))
                    .text_color(p.text_tertiary)
                    .child(way.clone()),
            )
    })
}
