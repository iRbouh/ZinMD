//! The command palette (Cmd K): type to jump to a heading of the open
//! document, open any Markdown file of the folder, or run a command.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::command::{Command, CommandGroup, CommandItem, CommandState};
use gpui_kit::component::{ActiveTheme as _, IndexPath};
use gpui_kit::*;

use super::*;
use crate::theme::{ActivePalette as _, size};

/// What choosing a row does. Commands are plain actions, dispatched by the
/// palette itself; the other rows need the workspace.
enum Target {
    Heading(u32),
    File(PathBuf),
    /// A line of a file, found by searching the folder.
    Line(PathBuf, u32),
    Command,
}

/// One thing the palette offers: its icon, its words, and what it does when
/// it is a command.
type Choice = (IconName, SharedString, Option<Box<dyn Action>>);
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
}

impl CommandPalette {
    /// Put text in the palette's field as if it had been typed. For scripted runs.
    #[cfg(feature = "shot")]
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
                .map(|heading| (IconName::Hash, heading.title.clone(), None))
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
                .map(|path| (IconName::FileText, self.place_of(path), None))
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
                .map(|(icon, label, action)| (icon, label.into(), Some(action)))
                .collect(),
        ));

        let state = cx.new(|cx| CommandState::new(window, cx));
        state.update(cx, |state, cx| state.focus(window, cx));
        self.palette = Some(CommandPalette {
            state,
            targets: Rc::new(targets),
            groups,
            searching: false,
        });
        cx.notify();
    }

    /// Open the palette to search the text of every file in the folder.
    pub(super) fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = cx.new(|cx| CommandState::new(window, cx));
        state.update(cx, |state, cx| state.focus(window, cx));
        self.palette = Some(CommandPalette {
            state,
            targets: Rc::new(Vec::new()),
            groups: Vec::new(),
            searching: true,
        });
        cx.notify();
    }

    /// Search the folder for what was typed, off the main thread, and show the lines found.
    fn search_folder(&mut self, query: String, cx: &mut Context<Self>) {
        let files = self.all_files();
        self._search = cx.spawn(async move |this, cx| {
            // Let a burst of typing settle: only the last query is worth running.
            cx.background_executor()
                .timer(std::time::Duration::from_millis(80))
                .await;
            let hits = cx
                .background_executor()
                .spawn(async move { crate::search::run(&files, &query, 200) })
                .await;
            this.update(cx, |this, cx| {
                if !this
                    .palette
                    .as_ref()
                    .is_some_and(|palette| palette.searching)
                {
                    return;
                }
                // One group per file, in the order the files were searched.
                let (mut targets, mut groups): (Vec<Vec<Target>>, Vec<(SharedString, Vec<_>)>) =
                    (Vec::new(), Vec::new());
                let mut current: Option<&PathBuf> = None;
                for hit in &hits {
                    if current != Some(&hit.path) {
                        groups.push((this.place_of(&hit.path), Vec::new()));
                        targets.push(Vec::new());
                        current = Some(&hit.path);
                    }
                    let label = format!("{:>4}   {}", hit.line + 1, hit.text);
                    groups
                        .last_mut()
                        .unwrap()
                        .1
                        .push((IconName::TextSearch, label.into(), None));
                    targets
                        .last_mut()
                        .unwrap()
                        .push(Target::Line(hit.path.clone(), hit.line));
                }
                if let Some(palette) = this.palette.as_mut() {
                    palette.targets = Rc::new(targets);
                    palette.groups = groups;
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
        let targets = palette.targets.clone();
        self.close_palette(window, cx);
        match targets.get(at.section).and_then(|group| group.get(at.row)) {
            Some(Target::Heading(line)) => self.jump_to_line(*line, window, cx),
            Some(Target::File(path)) => self.open(path.clone(), window, cx),
            Some(Target::Line(path, line)) => {
                self.open(path.clone(), window, cx);
                self.jump_to_line(*line, window, cx);
            }
            Some(Target::Command) | None => {}
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
            .max_h(px(420.))
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
            let items = rows.iter().map(|(icon, label, action)| {
                let item = CommandItem::new().icon(*icon).label(label.clone());
                match action {
                    Some(action) => item.action(action.boxed_clone()),
                    None => item,
                }
            });
            command = command.group(CommandGroup::new().label(title.clone()).items(items));
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
                        .w(px(600.))
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
