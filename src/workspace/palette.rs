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
    Command,
}

pub(super) struct CommandPalette {
    state: Entity<CommandState>,
    /// One list of targets per group, in the order the groups are shown.
    targets: Rc<Vec<Vec<Target>>>,
    groups: Vec<(
        &'static str,
        Vec<(IconName, SharedString, Option<Box<dyn Action>>)>,
    )>,
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
            groups.push(("Headings", rows));
        }

        if !self.file_index.is_empty() {
            let root = self.tree.root.clone().unwrap_or_default();
            let label = |path: &PathBuf| {
                SharedString::from(
                    path.strip_prefix(&root)
                        .unwrap_or(path)
                        .to_string_lossy()
                        .into_owned(),
                )
            };
            let rows = self
                .file_index
                .iter()
                .map(|path| (IconName::FileText, label(path), None))
                .collect();
            targets.push(self.file_index.iter().cloned().map(Target::File).collect());
            groups.push(("Files", rows));
        }

        let commands: Vec<(IconName, &str, Box<dyn Action>)> = vec![
            (IconName::Pencil, "View: Editor", Box::new(ViewEditor)),
            (IconName::Columns2, "View: Split", Box::new(ViewSplit)),
            (IconName::Eye, "View: Preview", Box::new(ViewPreview)),
            (
                IconName::PanelLeft,
                "Show or hide the sidebar",
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
        ];
        targets.push(commands.iter().map(|_| Target::Command).collect());
        groups.push((
            "Commands",
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
        });
        cx.notify();
    }

    pub(super) fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.take().is_some() {
            // Typing goes back to the document.
            if let Some(doc) = self.active_doc() {
                doc.editor.update(cx, |editor, cx| editor.focus(window, cx));
            }
            cx.notify();
        }
    }

    fn run_palette_target(&mut self, at: IndexPath, window: &mut Window, cx: &mut Context<Self>) {
        let Some(palette) = self.palette.as_ref() else {
            return;
        };
        let targets = palette.targets.clone();
        self.close_palette(window, cx);
        match targets.get(at.section).and_then(|group| group.get(at.row)) {
            Some(Target::Heading(line)) => self.jump_to_line(*line, window, cx),
            Some(Target::File(path)) => self.open(path.clone(), window, cx),
            Some(Target::Command) | None => {}
        }
    }

    /// The palette floating over the window, when it is open.
    pub(super) fn render_palette(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let palette = self.palette.as_ref()?;
        let p = *cx.palette();
        let (confirm, cancel, dismiss) = (cx.weak_entity(), cx.weak_entity(), cx.weak_entity());

        let mut command = Command::new(&palette.state)
            .placeholder("Search headings, files and commands")
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
            command = command.group(CommandGroup::new().label(*title).items(items));
        }

        Some(
            div()
                .id("palette-backdrop")
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
                        .with_animation(
                            "palette-arrives",
                            Animation::new(crate::motion::QUICK).with_easing(crate::motion::ease),
                            |panel, progress| {
                                panel.opacity(progress).mt(px(-10.) * (1. - progress))
                            },
                        ),
                ),
        )
    }
}

/// Every Markdown file under `root`, for the palette. Stops at `limit` files,
/// so a huge folder cannot stall the background thread that runs this.
pub(super) fn index_files(root: &Path, limit: usize) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for row in crate::files::read_dir(&folder, 0) {
            if row.is_dir {
                folders.push(row.path);
            } else {
                found.push(row.path);
                if found.len() >= limit {
                    return found;
                }
            }
        }
    }
    found.sort();
    found
}
