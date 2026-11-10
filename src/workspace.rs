//! The main window: open documents, the layout around them, and the commands
//! that change either. Each strip of the window draws itself in a submodule.

mod palette;
mod panes;
mod sidebar;
mod statusbar;
mod titlebar;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui_kit::base::input as edit;
use gpui_kit::component::input::{EditorState, InputEvent, Position};
use gpui_kit::component::resizable::{ResizableState, h_resizable, resizable_panel};
use gpui_kit::component::text::TextViewState;
use gpui_kit::component::v_flex;
use gpui_kit::*;

use crate::doc::{self, Heading, Stats};
use crate::files::{self, FileTree};
use crate::preview::BlockMap;
use crate::startup;
use crate::sync::{ScrollSync, Side};
use crate::theme::{self, ActivePalette as _, size};

actions!(
    zwinmd,
    [
        NewFile,
        OpenFile,
        Save,
        SaveAs,
        CloseTab,
        NextTab,
        PreviousTab,
        ViewEditor,
        ViewSplit,
        ViewPreview,
        ToggleSidebar,
        ToggleScrollSync,
        ToggleTheme,
        OpenPalette,
        ExportHtml,
        Quit
    ]
);

/// Register the app's keyboard shortcuts. Call once at startup.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-n", NewFile, None),
        KeyBinding::new("cmd-o", OpenFile, None),
        KeyBinding::new("cmd-s", Save, None),
        KeyBinding::new("cmd-shift-s", SaveAs, None),
        KeyBinding::new("cmd-w", CloseTab, None),
        KeyBinding::new("cmd-}", NextTab, None),
        KeyBinding::new("cmd-{", PreviousTab, None),
        KeyBinding::new("ctrl-tab", NextTab, None),
        KeyBinding::new("ctrl-shift-tab", PreviousTab, None),
        KeyBinding::new("cmd-1", ViewEditor, None),
        KeyBinding::new("cmd-2", ViewSplit, None),
        KeyBinding::new("cmd-3", ViewPreview, None),
        KeyBinding::new("cmd-\\", ToggleSidebar, None),
        KeyBinding::new("cmd-k", OpenPalette, None),
        KeyBinding::new("cmd-p", OpenPalette, None),
        KeyBinding::new("cmd-shift-e", ExportHtml, None),
        KeyBinding::new("cmd-q", Quit, None),
    ]);
    // Reached only when no window is there to ask about unsaved changes.
    cx.on_action(|_: &Quit, cx| cx.quit());
}

/// Install the menu bar. Building it makes AppKit load extra frameworks (about
/// 11 ms), so this is called once the first frame is on screen.
pub fn install_menus(cx: &mut App) {
    cx.set_menus(vec![
        Menu::new("zwinMD").items([MenuItem::action("Quit zwinMD", Quit)]),
        Menu::new("File").items([
            MenuItem::action("New", NewFile),
            MenuItem::action("Open…", OpenFile),
            MenuItem::separator(),
            MenuItem::action("Save", Save),
            MenuItem::action("Save As…", SaveAs),
            MenuItem::action("Export as HTML…", ExportHtml),
            MenuItem::separator(),
            MenuItem::action("Close Tab", CloseTab),
        ]),
        Menu::new("Edit").items([
            MenuItem::action("Undo", edit::Undo),
            MenuItem::action("Redo", edit::Redo),
            MenuItem::separator(),
            MenuItem::action("Cut", edit::Cut),
            MenuItem::action("Copy", edit::Copy),
            MenuItem::action("Paste", edit::Paste),
            MenuItem::action("Select All", edit::SelectAll),
            MenuItem::separator(),
            MenuItem::action("Find…", edit::Search),
        ]),
        Menu::new("View").items([
            MenuItem::action("Editor", ViewEditor),
            MenuItem::action("Split", ViewSplit),
            MenuItem::action("Preview", ViewPreview),
            MenuItem::separator(),
            MenuItem::action("Command Palette…", OpenPalette),
            MenuItem::action("Show or Hide Sidebar", ToggleSidebar),
            MenuItem::action("Turn Scroll Sync On or Off", ToggleScrollSync),
            MenuItem::action("Switch Light and Dark", ToggleTheme),
        ]),
    ]);
}

/// Which panes the window shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    Editor,
    Split,
    Preview,
}

/// One open document: a tab.
pub struct Doc {
    /// Stable for the life of the tab, so per-tab UI state (scroll, selection) never mixes.
    id: usize,
    path: Option<PathBuf>,
    name: SharedString,
    editor: Entity<EditorState>,
    preview: Entity<TextViewState>,
    /// The source as of the last edit.
    text: SharedString,
    /// The source as it is on disk.
    saved: SharedString,
    headings: Vec<Heading>,
    stats: Stats,
    /// Which source lines each preview row shows; rebuilt in the background after edits.
    blocks: BlockMap,
    sync: ScrollSync,
    _changes: Subscription,
    _blocks: Task<()>,
}

impl Doc {
    fn dirty(&self) -> bool {
        self.text != self.saved
    }
}

pub struct Workspace {
    docs: Vec<Doc>,
    active: usize,
    next_doc_id: usize,
    view: ViewMode,
    sidebar_open: bool,
    sync_enabled: bool,
    tree: FileTree,
    tree_scroll: UniformListScrollHandle,
    /// Every Markdown file under the open folder, for the command palette.
    file_index: Vec<PathBuf>,
    palette: Option<palette::CommandPalette>,
    /// Widths of the sidebar and the document area.
    sidebar_split: Entity<ResizableState>,
    /// Widths of the editor and the preview.
    pane_split: Entity<ResizableState>,
    focus: FocusHandle,
    /// Set once the user agreed to lose, or saved, every unsaved change.
    close_confirmed: bool,
    /// When the process started, to report how long opening took.
    started: Instant,
    opened_ms: Option<f64>,
    first_frame_requested: bool,
}

impl Workspace {
    /// `path` may be a Markdown file, a folder, or nothing.
    pub fn new(
        path: Option<PathBuf>,
        started: Instant,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self {
            docs: Vec::new(),
            active: 0,
            next_doc_id: 0,
            view: ViewMode::Split,
            sidebar_open: true,
            sync_enabled: true,
            tree: FileTree::default(),
            tree_scroll: UniformListScrollHandle::new(),
            file_index: Vec::new(),
            palette: None,
            sidebar_split: cx.new(|_| ResizableState::default()),
            pane_split: cx.new(|_| ResizableState::default()),
            focus: cx.focus_handle(),
            close_confirmed: false,
            started,
            opened_ms: None,
            first_frame_requested: false,
        };

        let path = path.map(|path| path.canonicalize().unwrap_or(path));
        let (root, file) = match path {
            Some(dir) if dir.is_dir() => {
                let readme = ["README.md", "readme.md", "index.md"]
                    .iter()
                    .map(|name| dir.join(name))
                    .find(|p| p.is_file());
                (Some(dir), readme)
            }
            Some(file) => (file.parent().map(Path::to_path_buf), Some(file)),
            None => (None, None),
        };
        match file {
            Some(file) => this.open_path(&file, window, cx),
            None => this.open_untitled(window, cx),
        }
        this.set_root(root, cx);

        // The red window button: ask about unsaved changes before closing.
        let weak = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            let Some(this) = weak.upgrade() else {
                return true;
            };
            this.update(cx, |this, cx| {
                if this.close_confirmed || !this.docs.iter().any(Doc::dirty) {
                    return true;
                }
                let confirmed = this.confirm_closing_all(window, cx);
                cx.spawn_in(window, async move |this, cx| {
                    if confirmed.await {
                        this.update_in(cx, |this, window, _| {
                            this.close_confirmed = true;
                            window.remove_window();
                        })
                        .ok();
                    }
                })
                .detach();
                false
            })
        });

        startup::mark("document loaded");
        this
    }

    fn active_doc(&self) -> Option<&Doc> {
        self.docs.get(self.active)
    }

    fn doc_ix(&self, id: usize) -> Option<usize> {
        self.docs.iter().position(|doc| doc.id == id)
    }

    // ------------------------------------------------------------ documents

    /// Open a file in a tab, or show a folder in the sidebar.
    fn open(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let path = path.canonicalize().unwrap_or(path);
        if path.is_dir() {
            self.set_root(Some(path), cx);
            return;
        }
        if self.tree.root.is_none() {
            self.set_root(path.parent().map(Path::to_path_buf), cx);
        }
        // An untouched empty tab was only a placeholder: the file takes its place.
        let placeholder = match self.docs.as_slice() {
            [only] if only.path.is_none() && only.text.is_empty() => Some(only.id),
            _ => None,
        };
        self.open_path(&path, window, cx);
        if let Some(ix) = placeholder.and_then(|id| self.doc_ix(id)) {
            self.docs.remove(ix);
            self.active = self.active.saturating_sub(1);
        }
    }

    /// Show `path` in a tab, reusing its tab if it is already open.
    fn open_path(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self
            .docs
            .iter()
            .position(|doc| doc.path.as_deref() == Some(path))
        {
            self.activate(ix, window, cx);
            return;
        }
        // Not valid UTF-8, or unreadable: open what we can rather than nothing.
        let text = std::fs::read(path)
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default();
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.push_doc(Some(path.to_path_buf()), name.into(), text, window, cx);
    }

    fn open_untitled(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.push_doc(None, "Untitled.md".into(), String::new(), window, cx);
    }

    fn push_doc(
        &mut self,
        path: Option<PathBuf>,
        name: SharedString,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = SharedString::from(text);
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language("markdown")
                .line_number(true)
                .soft_wrap(true)
                .default_value(text.clone())
        });
        let preview = cx.new(|cx| TextViewState::markdown(&text, cx));
        let changes = cx.subscribe(&editor, |this, editor, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.on_edit(&editor, cx);
            }
        });
        let id = self.next_doc_id;
        self.next_doc_id += 1;
        self.docs.push(Doc {
            id,
            path,
            name,
            editor,
            preview,
            headings: doc::outline(&text),
            stats: Stats::of(&text),
            saved: text.clone(),
            text,
            blocks: BlockMap::default(),
            sync: ScrollSync::default(),
            _changes: changes,
            _blocks: Task::ready(()),
        });
        let ix = self.docs.len() - 1;
        self.map_blocks(ix, Duration::ZERO, cx);
        self.activate(ix, window, cx);
    }

    fn on_edit(&mut self, editor: &Entity<EditorState>, cx: &mut Context<Self>) {
        let Some(ix) = self.docs.iter().position(|doc| doc.editor == *editor) else {
            return;
        };
        let text = SharedString::from(editor.read(cx).text().to_string());
        let doc = &mut self.docs[ix];
        doc.headings = doc::outline(&text);
        doc.stats = Stats::of(&text);
        doc.preview
            .update(cx, |preview, cx| preview.set_text(&text, cx));
        doc.text = text;
        // Wait for a pause in typing: the map is only needed to scroll.
        self.map_blocks(ix, Duration::from_millis(80), cx);
        cx.notify();
    }

    /// Rebuild the line map of a document off the main thread. A newer request
    /// replaces one that is still waiting.
    fn map_blocks(&mut self, ix: usize, delay: Duration, cx: &mut Context<Self>) {
        let doc = &mut self.docs[ix];
        let (id, text) = (doc.id, doc.text.clone());
        doc._blocks = cx.spawn(async move |this, cx| {
            if !delay.is_zero() {
                cx.background_executor().timer(delay).await;
            }
            let blocks = cx
                .background_executor()
                .spawn(async move { BlockMap::parse(&text) })
                .await;
            this.update(cx, |this, cx| {
                if let Some(ix) = this.doc_ix(id) {
                    this.docs[ix].blocks = blocks;
                    cx.notify();
                }
            })
            .ok();
        });
    }

    fn activate(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.docs.len() {
            return;
        }
        self.active = ix;
        self.docs[ix]
            .editor
            .update(cx, |editor, cx| editor.focus(window, cx));
        if let Some(path) = self.docs[ix].path.clone() {
            self.tree.reveal(&path);
        }
        cx.notify();
    }

    fn cycle_tab(&mut self, step: isize, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.docs.len() as isize;
        if count > 1 {
            self.activate(
                (self.active as isize + step).rem_euclid(count) as usize,
                window,
                cx,
            );
        }
    }

    /// Put the caret on a source line and bring it into view.
    fn jump_to_line(&mut self, line: u32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(doc) = self.active_doc() else { return };
        doc.editor.update(cx, |editor, cx| {
            editor.set_cursor_position(Position::new(line, 0), window, cx);
            editor.focus(window, cx);
        });
    }

    // --------------------------------------------------------------- saving

    /// Save a document, asking where if it has no file yet or `ask` is set.
    /// Resolves to whether it is now saved.
    fn save_doc(
        &mut self,
        id: usize,
        ask: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<bool> {
        let Some(ix) = self.doc_ix(id) else {
            return Task::ready(false);
        };
        let doc = &self.docs[ix];
        if doc.path.is_some() && !ask {
            return Task::ready(self.write_doc(ix, window, cx));
        }
        let folder = doc
            .path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .or_else(|| self.tree.root.clone())
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_default();
        let chosen = cx.prompt_for_new_path(&folder, Some(&doc.name));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(mut path))) = chosen.await else {
                return false;
            };
            if path.extension().is_none() {
                path.set_extension("md");
            }
            this.update_in(cx, |this, window, cx| {
                let Some(ix) = this.doc_ix(id) else {
                    return false;
                };
                let doc = &mut this.docs[ix];
                doc.name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default()
                    .into();
                doc.path = Some(path.clone());
                let saved = this.write_doc(ix, window, cx);
                // The new file may belong in the sidebar.
                match this.tree.root {
                    Some(_) => this.tree.refresh(),
                    None => this.set_root(path.parent().map(Path::to_path_buf), cx),
                }
                this.tree.reveal(&path);
                saved
            })
            .unwrap_or(false)
        })
    }

    /// Write a document to its file. Tells the user if that fails.
    fn write_doc(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let doc = &mut self.docs[ix];
        let Some(path) = &doc.path else { return false };
        match std::fs::write(path, doc.text.as_bytes()) {
            Ok(()) => {
                doc.saved = doc.text.clone();
                cx.notify();
                true
            }
            Err(error) => {
                let message = format!("“{}” could not be saved.", doc.name);
                drop(window.prompt(
                    PromptLevel::Critical,
                    &message,
                    Some(&error.to_string()),
                    &["OK"],
                    cx,
                ));
                false
            }
        }
    }

    /// Close a tab, first offering to save it if it has unsaved changes.
    fn request_close(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(doc) = self.docs.get(ix) else { return };
        let id = doc.id;
        if !doc.dirty() {
            self.close_doc(id, window, cx);
            return;
        }
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!(
                "Do you want to save the changes you made to “{}”?",
                doc.name
            ),
            Some("Your changes will be lost if you don’t save them."),
            &["Save", "Don’t Save", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let close = match answer.await {
                Ok(0) => match this
                    .update_in(cx, |this, window, cx| this.save_doc(id, false, window, cx))
                {
                    Ok(saving) => saving.await,
                    Err(_) => false,
                },
                Ok(1) => true,
                _ => false,
            };
            if close {
                this.update_in(cx, |this, window, cx| this.close_doc(id, window, cx))
                    .ok();
            }
        })
        .detach();
    }

    fn close_doc(&mut self, id: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.doc_ix(id) else { return };
        self.docs.remove(ix);
        if self.docs.is_empty() {
            self.close_confirmed = true;
            window.remove_window();
            return;
        }
        let next = if ix < self.active {
            self.active - 1
        } else {
            self.active.min(self.docs.len() - 1)
        };
        self.activate(next, window, cx);
    }

    /// Before closing everything: offer to save what is unsaved. Resolves to
    /// whether closing may go ahead.
    fn confirm_closing_all(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Task<bool> {
        let unsaved: Vec<usize> = self
            .docs
            .iter()
            .filter(|doc| doc.dirty())
            .map(|doc| doc.id)
            .collect();
        let (message, save) = match unsaved.as_slice() {
            [] => return Task::ready(true),
            [one] => {
                let name = self
                    .doc_ix(*one)
                    .map(|ix| self.docs[ix].name.clone())
                    .unwrap_or_default();
                (
                    format!("Do you want to save the changes you made to “{name}”?"),
                    "Save",
                )
            }
            many => (
                format!(
                    "You have {} documents with unsaved changes. Do you want to save them?",
                    many.len()
                ),
                "Save All",
            ),
        };
        let answer = window.prompt(
            PromptLevel::Warning,
            &message,
            Some("Your changes will be lost if you don’t save them."),
            &[save, "Don’t Save", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| match answer.await {
            Ok(0) => {
                for id in unsaved {
                    let Ok(saving) =
                        this.update_in(cx, |this, window, cx| this.save_doc(id, false, window, cx))
                    else {
                        return false;
                    };
                    if !saving.await {
                        return false;
                    }
                }
                true
            }
            Ok(1) => true,
            _ => false,
        })
    }

    fn quit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let confirmed = self.confirm_closing_all(window, cx);
        cx.spawn(async move |_, cx| {
            if confirmed.await {
                cx.update(|cx| cx.quit());
            }
        })
        .detach();
    }

    /// Write the open document as a standalone HTML page, where the user chooses.
    pub(super) fn export_html(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(doc) = self.active_doc() else { return };
        let (text, name) = (doc.text.clone(), doc.name.clone());
        let folder = doc
            .path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_default();
        let suggested = Path::new(name.as_ref()).with_extension("html");
        let chosen = cx.prompt_for_new_path(&folder, suggested.to_str());
        cx.spawn_in(window, async move |_, cx| {
            let Ok(Ok(Some(path))) = chosen.await else {
                return;
            };
            // Diagrams are drawn during export, which can take a moment.
            let page = cx
                .background_executor()
                .spawn(async move { crate::export::html(&text, &name) })
                .await;
            let written = std::fs::write(&path, page);
            cx.update(|window, cx| match written {
                Ok(()) => cx.reveal_path(&path),
                Err(error) => {
                    let detail = error.to_string();
                    drop(window.prompt(
                        PromptLevel::Critical,
                        "The page could not be exported.",
                        Some(&detail),
                        &["OK"],
                        cx,
                    ));
                }
            })
            .ok();
        })
        .detach();
    }

    fn open_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: true,
            prompt: None,
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = chosen.await else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                for path in paths {
                    this.open(path, window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    // ------------------------------------------------------------- sidebar

    /// Point the file tree at a folder. The folder is read off the main thread.
    fn set_root(&mut self, root: Option<PathBuf>, cx: &mut Context<Self>) {
        self.tree = FileTree {
            root: root.clone(),
            rows: Vec::new(),
        };
        self.file_index.clear();
        cx.notify();
        let Some(root) = root else { return };
        let reveal = self.active_doc().and_then(|doc| doc.path.clone());
        cx.spawn(async move |this, cx| {
            let (tree, index) = cx
                .background_executor()
                .spawn(async move {
                    let mut tree = FileTree {
                        rows: files::read_dir(&root, 0),
                        root: Some(root),
                    };
                    if let Some(path) = reveal {
                        tree.reveal(&path);
                    }
                    let index = tree
                        .root
                        .as_deref()
                        .map(|root| palette::index_files(root, 5000));
                    (tree, index.unwrap_or_default())
                })
                .await;
            this.update(cx, |this, cx| {
                // Unless another folder was chosen while this one was being read.
                if this.tree.root == tree.root {
                    this.tree = tree;
                    this.file_index = index;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn on_tree_click(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(row) = self.tree.rows.get(ix) else {
            return;
        };
        if row.is_dir {
            self.tree.toggle(ix);
            cx.notify();
        } else {
            let path = row.path.clone();
            self.open_path(&path, window, cx);
        }
    }

    // ------------------------------------------------------------- commands

    fn set_view(&mut self, view: ViewMode, cx: &mut Context<Self>) {
        // A pane that was hidden catches up with the one that stayed on screen.
        let shown = match self.view {
            ViewMode::Preview => Side::Preview,
            _ => Side::Editor,
        };
        if let Some(doc) = self.docs.get_mut(self.active) {
            doc.sync.follow(shown);
        }
        self.view = view;
        cx.notify();
    }

    fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_open = !self.sidebar_open;
        cx.notify();
    }

    fn toggle_scroll_sync(&mut self, cx: &mut Context<Self>) {
        self.sync_enabled = !self.sync_enabled;
        if let Some(doc) = self.docs.get_mut(self.active) {
            doc.sync.follow(Side::Editor);
        }
        cx.notify();
    }

    /// One step of a `ZWINMD_SCRIPT`, standing in for the user in scripted checks:
    /// `editor:<px>` and `preview:<px>` scroll a pane, `type:<text>` types at the
    /// caret, `line:<n>` moves the caret, `view:editor|split|preview`, `sidebar`,
    /// `sync`, `new`, `open:<path>`, `save` and `close` run those commands,
    /// `link:<url>` follows a link and `check:<n>` clicks the n-th task checkbox.
    #[cfg(feature = "shot")]
    pub fn debug_step(&mut self, step: &str, window: &mut Window, cx: &mut Context<Self>) {
        let (command, argument) = step.split_once(':').unwrap_or((step, ""));
        let Some(doc) = self.active_doc() else { return };
        let id = doc.id;
        match command {
            "editor" => {
                let y = -px(argument.parse().unwrap_or(0.));
                doc.editor.update(cx, |editor, cx| {
                    editor.set_scroll_offset(point(px(0.), y), cx)
                });
            }
            "preview" => doc
                .preview
                .read(cx)
                .list_state()
                .scroll_by(px(argument.parse().unwrap_or(0.))),
            "type" => {
                let text = argument.replace("\\n", "\n");
                // Report what one edit costs: until the editor repaints, and
                // until the preview shows the new text.
                let edited = Instant::now();
                doc.editor
                    .update(cx, |editor, cx| editor.insert(text, window, cx));
                window.on_next_frame(move |_, _| {
                    eprintln!(
                        "edit -> frame   {:>6.1} ms",
                        edited.elapsed().as_secs_f64() * 1000.
                    )
                });
                let mut seen = false;
                cx.observe(&doc.preview, move |_, _, _| {
                    if !std::mem::replace(&mut seen, true) {
                        eprintln!(
                            "edit -> preview {:>6.1} ms",
                            edited.elapsed().as_secs_f64() * 1000.
                        );
                    }
                })
                .detach();
            }
            "line" => self.jump_to_line(argument.parse().unwrap_or(0), window, cx),
            "view" => self.set_view(
                match argument {
                    "editor" => ViewMode::Editor,
                    "preview" => ViewMode::Preview,
                    _ => ViewMode::Split,
                },
                cx,
            ),
            "sidebar" => self.toggle_sidebar(cx),
            "sync" => self.toggle_scroll_sync(cx),
            "new" => self.open_untitled(window, cx),
            "open" => self.open(PathBuf::from(argument), window, cx),
            "save" => self.save_doc(id, false, window, cx).detach(),
            "close" => self.request_close(self.active, window, cx),
            "palette" => self.open_palette(window, cx),
            "export" => {
                let doc = &self.docs[self.active];
                if let Err(error) =
                    std::fs::write(argument, crate::export::html(&doc.text, &doc.name))
                {
                    eprintln!("export failed: {error}");
                }
            }
            "link" => self.follow_link(argument, window, cx),
            "check" => {
                // Click the n-th task checkbox, as the preview would report it.
                let text = self.docs[self.active].text.clone();
                let nth = argument.parse().unwrap_or(0);
                let marker = text
                    .match_indices("[")
                    .filter(|(at, _)| matches!(text.get(*at..*at + 3), Some("[ ]" | "[x]")))
                    .nth(nth);
                if let Some((at, _)) = marker {
                    let checked = &text[at..at + 3] == "[ ]";
                    self.toggle_task(id, at..at + 3, checked, window, cx);
                }
            }
            other => eprintln!("unknown script step: {other}"),
        }
        cx.notify();
    }

    // ---------------------------------------------------------------- panes

    /// Record how long it took from process start to the first frame on screen.
    fn time_first_frame(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.first_frame_requested {
            return;
        }
        self.first_frame_requested = true;
        startup::mark("first render");
        let started = self.started;
        let this = cx.weak_entity();
        window.on_next_frame(move |_, cx| {
            let ms = started.elapsed().as_secs_f64() * 1000.;
            if startup::enabled() {
                startup::mark("first frame on screen");
                startup::report();
                cx.quit();
                return;
            }
            install_menus(cx);
            this.update(cx, |this, cx| {
                this.opened_ms = Some(ms);
                cx.notify();
            })
            .ok();
        });
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.time_first_frame(window, cx);
        self.sync_scroll(window, cx);
        let p = *cx.palette();
        let titlebar = self.render_titlebar(window, cx);
        let sidebar = self.render_sidebar(cx);
        let panes = self.render_panes(window, cx);
        let statusbar = self.render_statusbar(cx);
        let palette = self.render_palette(cx);

        v_flex()
            .id("workspace")
            .relative()
            .size_full()
            .bg(p.surface_app)
            .text_color(p.text_primary)
            .text_size(size::FONT_UI)
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &NewFile, window, cx| this.open_untitled(window, cx)))
            .on_action(cx.listener(|this, _: &OpenFile, window, cx| this.open_dialog(window, cx)))
            .on_action(cx.listener(|this, _: &Save, window, cx| {
                if let Some(id) = this.active_doc().map(|doc| doc.id) {
                    this.save_doc(id, false, window, cx).detach();
                }
            }))
            .on_action(cx.listener(|this, _: &SaveAs, window, cx| {
                if let Some(id) = this.active_doc().map(|doc| doc.id) {
                    this.save_doc(id, true, window, cx).detach();
                }
            }))
            .on_action(cx.listener(|this, _: &CloseTab, window, cx| {
                this.request_close(this.active, window, cx)
            }))
            .on_action(cx.listener(|this, _: &NextTab, window, cx| this.cycle_tab(1, window, cx)))
            .on_action(
                cx.listener(|this, _: &PreviousTab, window, cx| this.cycle_tab(-1, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ViewEditor, _, cx| this.set_view(ViewMode::Editor, cx)),
            )
            .on_action(cx.listener(|this, _: &ViewSplit, _, cx| this.set_view(ViewMode::Split, cx)))
            .on_action(
                cx.listener(|this, _: &ViewPreview, _, cx| this.set_view(ViewMode::Preview, cx)),
            )
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| this.toggle_sidebar(cx)))
            .on_action(cx.listener(|this, _: &ToggleScrollSync, _, cx| this.toggle_scroll_sync(cx)))
            .on_action(cx.listener(|_, _: &ToggleTheme, _, cx| theme::toggle(cx)))
            .on_action(
                cx.listener(|this, _: &OpenPalette, window, cx| match this.palette {
                    Some(_) => this.close_palette(window, cx),
                    None => this.open_palette(window, cx),
                }),
            )
            .on_action(cx.listener(|this, _: &ExportHtml, window, cx| this.export_html(window, cx)))
            .on_action(cx.listener(|this, _: &Quit, window, cx| this.quit(window, cx)))
            .child(titlebar)
            .child(
                div().flex_1().min_h_0().w_full().child(
                    h_resizable("workspace")
                        .with_state(&self.sidebar_split)
                        .child(
                            resizable_panel()
                                .visible(self.sidebar_open)
                                .size(size::SIDEBAR)
                                .size_range(px(180.)..px(480.))
                                .child(sidebar),
                        )
                        .child(
                            resizable_panel()
                                .size_range(px(320.)..Pixels::MAX)
                                .child(panes),
                        ),
                ),
            )
            .child(statusbar)
            .children(palette)
    }
}
