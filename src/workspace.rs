//! The main window: open documents, the layout around them, and the commands
//! that change either. Each strip of the window draws itself in a submodule.

mod backup;
mod disk;
mod fileops;
mod hover;
mod palette;
mod panes;
mod sidebar;
mod statusbar;
mod titlebar;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui_kit::base::input as edit;
use gpui_kit::base::transition;
use gpui_kit::component::input::{EditorState, InputEvent};
use gpui_kit::component::text::TextViewState;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::doc::{self, Heading, Stats};
use crate::files::{self, FileTree};
use crate::motion;
use crate::preview::BlockMap;
use crate::session::Session;
use crate::settings::{self, EditorFont, Settings};
use crate::startup;
use crate::sync::{ScrollSync, Side};
use crate::theme::{self, ActivePalette as _, size};
use crate::ui;

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
        FindInFolder,
        ExportHtml,
        NewFileHere,
        NewFolderHere,
        RenameFile,
        TrashFile,
        RevealFile,
        CopyFilePath,
        FontBigger,
        FontSmaller,
        FontReset,
        UseJetBrainsMono,
        UseFiraCode,
        UseMenlo,
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
        KeyBinding::new("cmd-shift-f", FindInFolder, None),
        KeyBinding::new("cmd-shift-e", ExportHtml, None),
        KeyBinding::new("cmd-=", FontBigger, None),
        KeyBinding::new("cmd-+", FontBigger, None),
        KeyBinding::new("cmd--", FontSmaller, None),
        KeyBinding::new("cmd-0", FontReset, None),
        KeyBinding::new("cmd-q", Quit, None),
    ]);
    // Reached only when no window is there to ask about unsaved changes.
    cx.on_action(|_: &Quit, cx| cx.quit());
}

/// Install the menu bar. Building it makes AppKit load extra frameworks (about
/// 11 ms), so this is called once the first frame is on screen.
pub fn install_menus(cx: &mut App) {
    cx.set_menus(vec![
        Menu::new("ZwinMD").items([MenuItem::action("Quit ZwinMD", Quit)]),
        Menu::new("File").items([
            MenuItem::action("New", NewFile),
            MenuItem::action("Open…", OpenFile),
            MenuItem::separator(),
            MenuItem::action("Save", Save),
            MenuItem::action("Save As…", SaveAs),
            MenuItem::action("Export as HTML…", ExportHtml),
            MenuItem::separator(),
            MenuItem::action("Rename…", RenameFile),
            MenuItem::action("Show in Finder", RevealFile),
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
            MenuItem::action("Find in All Files…", FindInFolder),
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
            MenuItem::separator(),
            MenuItem::action("Bigger Text", FontBigger),
            MenuItem::action("Smaller Text", FontSmaller),
            MenuItem::action("Default Text Size", FontReset),
            MenuItem::separator(),
            MenuItem::action("Font: JetBrains Mono", UseJetBrainsMono),
            MenuItem::action("Font: Fira Code", UseFiraCode),
            MenuItem::action("Font: Menlo", UseMenlo),
        ]),
    ]);
}

/// What dragging the edge of the sidebar carries.
struct DragSidebar;

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
    /// What its file looked like when we last read or wrote it, to tell when
    /// someone else changes it; and anything the user should be told about it.
    disk: Option<(std::time::SystemTime, u64)>,
    notice: Option<disk::Notice>,
    /// Names the file its unsaved edits are backed up to, and the pending write.
    backup_key: String,
    _backup: Task<()>,
    /// How wide its tab is, and when the tab was opened if it should grow in.
    tab_width: Pixels,
    born: Option<Instant>,
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
    tree_scroll: ScrollHandle,
    /// A folder of the file list sliding open or shut.
    tree_motion: Option<sidebar::TreeMotion>,
    /// Tabs of the last session still to bring back: their files, and which
    /// of them is the one already open.
    restore: Option<(Vec<PathBuf>, usize)>,
    /// The session as last written down, to notice when it changes.
    saved_session: Option<Session>,
    _session_save: Task<()>,
    /// Tells us when files change on disk; started after the first frame.
    watcher: Option<crate::watch::Watcher>,
    _watching: Task<()>,
    _tree_refresh: Task<()>,
    /// The row of the file list that was last right-clicked: what its menu acts on.
    context_row: Option<PathBuf>,
    /// The field for naming a new or renamed file, while one is being named.
    name_field: Option<fileops::NameField>,
    /// What the pointer is over, for hover that fades.
    hot: Option<hover::Hot>,
    /// Closed tabs still shrinking out of the tab strip.
    ghost_tabs: Vec<titlebar::GhostTab>,
    /// Counts changes of the document on show, so each one can fade in.
    switches: usize,
    /// Every Markdown file under the open folder, for the command palette.
    file_index: Vec<PathBuf>,
    palette: Option<palette::CommandPalette>,
    _search: Task<()>,
    /// The palette just closed, fading out, and when it closed.
    palette_leaving: Option<(palette::CommandPalette, Instant)>,
    /// How wide the sidebar is when open; dragging its edge changes this.
    sidebar_width: Pixels,
    /// The editor's share of the document area in split view.
    split: f32,
    /// The editor scrolling smoothly to where it was asked to jump.
    glide: Option<panes::Glide>,
    /// The widths the editor and preview text were last wrapped to at rest.
    settled: (Pixels, Pixels),
    focus: FocusHandle,
    /// Set once the user agreed to lose, or saved, every unsaved change.
    close_confirmed: bool,
    /// When the process started, to report how long opening took.
    started: Instant,
    opened_ms: Option<f64>,
    first_frame_requested: bool,
    /// When each frame was drawn, to judge smoothness in scripted runs.
    #[cfg(feature = "shot")]
    frames: Vec<Instant>,
}

impl Workspace {
    /// `path` may be a Markdown file, a folder, or nothing.
    pub fn new(
        path: Option<PathBuf>,
        session: Session,
        started: Instant,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self {
            docs: Vec::new(),
            active: 0,
            next_doc_id: 0,
            view: match session.view.as_str() {
                "editor" => ViewMode::Editor,
                "preview" => ViewMode::Preview,
                _ => ViewMode::Split,
            },
            sidebar_open: session.sidebar_open,
            sync_enabled: session.scroll_sync,
            restore: None,
            saved_session: None,
            _session_save: Task::ready(()),
            tree: FileTree::default(),
            tree_scroll: ScrollHandle::new(),
            tree_motion: None,
            watcher: None,
            _watching: Task::ready(()),
            _tree_refresh: Task::ready(()),
            context_row: None,
            name_field: None,
            hot: None,
            ghost_tabs: Vec::new(),
            switches: 0,
            file_index: Vec::new(),
            palette: None,
            _search: Task::ready(()),
            palette_leaving: None,
            sidebar_width: px(session.sidebar_width),
            split: session.split,
            glide: None,
            settled: (px(0.), px(0.)),
            focus: cx.focus_handle(),
            close_confirmed: false,
            started,
            opened_ms: None,
            first_frame_requested: false,
            #[cfg(feature = "shot")]
            frames: Vec::new(),
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
            // Nothing asked for: pick up where the last session left off.
            None => {
                let tabs: Vec<PathBuf> = session
                    .tabs
                    .into_iter()
                    .filter(|tab| tab.is_file())
                    .collect();
                let shown = session.active.min(tabs.len().saturating_sub(1));
                let file = tabs.get(shown).cloned();
                if tabs.len() > 1 {
                    this.restore = Some((tabs, shown));
                }
                (session.root.filter(|root| root.is_dir()), file)
            }
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
                            this.drop_backups();
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

    /// Open what the system, or a drop on the window, asked for, and come to the front.
    pub fn open_requested(
        &mut self,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if paths.is_empty() {
            return;
        }
        for path in paths {
            self.open(path, window, cx);
        }
        window.activate_window();
        cx.activate(true);
    }

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
        // The tab the window opens with is simply there; later ones grow in.
        let born = (!self.docs.is_empty()).then(Instant::now);
        let ix = self.docs.len();
        self.insert_doc(ix, path, name, text, born, window, cx);
        self.activate(ix, window, cx);
    }

    /// Add a document as tab number `at`, without bringing it on show.
    #[allow(clippy::too_many_arguments)]
    fn insert_doc(
        &mut self,
        at: usize,
        path: Option<PathBuf>,
        name: SharedString,
        text: String,
        born: Option<Instant>,
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
        let disk = path.as_deref().and_then(crate::watch::stamp);
        if let (Some(watcher), Some(path)) = (&mut self.watcher, &path) {
            watcher.include(path);
        }
        let tab_width = titlebar::tab_width(ui::text_width(
            &name,
            size::FONT_UI,
            FontWeight::MEDIUM,
            window,
        ));
        let doc = Doc {
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
            disk,
            notice: None,
            // Unique among everything this app has ever backed up: start time and a counter.
            backup_key: format!(
                "{}-{id}",
                self.started.elapsed().as_nanos() ^ std::process::id() as u128
            ),
            _backup: Task::ready(()),
            tab_width,
            born,
            _changes: changes,
            _blocks: Task::ready(()),
        };
        let at = at.min(self.docs.len());
        self.docs.insert(at, doc);
        // Tabs after the new one moved along by one.
        if self.docs.len() > 1 && self.active >= at {
            self.active += 1;
        }
        self.map_blocks(at, Duration::ZERO, cx);
    }

    /// Bring back the other tabs of the last session, around the one already
    /// open. Called after the first frame, so they do not hold up launch.
    fn restore_tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((tabs, shown)) = self.restore.take() else {
            return;
        };
        for (ix, path) in tabs.iter().enumerate() {
            if ix == shown
                || self
                    .docs
                    .iter()
                    .any(|doc| doc.path.as_deref() == Some(path.as_path()))
            {
                continue;
            }
            let Ok(bytes) = std::fs::read(path) else {
                continue;
            };
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            // Tabs saved before the shown one go before it, the rest after.
            let at = if ix < shown {
                self.active
            } else {
                self.docs.len()
            };
            let text = String::from_utf8_lossy(&bytes).into_owned();
            self.insert_doc(at, Some(path.clone()), name.into(), text, None, window, cx);
        }
        cx.notify();
    }

    /// The state worth remembering for next time.
    fn session(&self, window: &Window) -> Session {
        let bounds = match window.window_bounds() {
            WindowBounds::Windowed(bounds)
            | WindowBounds::Maximized(bounds)
            | WindowBounds::Fullscreen(bounds) => bounds,
        };
        let tabs: Vec<PathBuf> = self
            .docs
            .iter()
            .filter_map(|doc| doc.path.clone())
            .collect();
        let shown = self.active_doc().and_then(|doc| doc.path.as_ref());
        Session {
            window: Some([
                bounds.origin.x.into(),
                bounds.origin.y.into(),
                bounds.size.width.into(),
                bounds.size.height.into(),
            ]),
            root: self.tree.root.clone(),
            active: shown
                .and_then(|shown| tabs.iter().position(|tab| tab == shown))
                .unwrap_or(0),
            tabs,
            view: match self.view {
                ViewMode::Editor => "editor",
                ViewMode::Split => "split",
                ViewMode::Preview => "preview",
            }
            .into(),
            sidebar_open: self.sidebar_open,
            sidebar_width: self.sidebar_width.into(),
            split: self.split,
            scroll_sync: self.sync_enabled,
        }
    }

    /// Write the session down if it changed. Checked every frame, which is
    /// simpler and safer than remembering to at every place that changes it;
    /// the write itself waits for things to settle and happens off the main thread.
    fn remember_session(&mut self, window: &Window, cx: &mut Context<Self>) {
        let session = self.session(window);
        if self.saved_session.as_ref() == Some(&session) {
            return;
        }
        self.saved_session = Some(session.clone());
        self._session_save = cx.spawn(async move |_, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(500))
                .await;
            cx.background_executor()
                .spawn(async move { session.save() })
                .await;
        });
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
        self.back_up(ix, cx);
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
        if self.active != ix && self.docs.len() > 1 {
            self.switches += 1;
        }
        self.active = ix;
        // Not drawn in our own title bar, but it names the window in Mission
        // Control, the Window menu and to assistive tools.
        window.set_window_title(&self.docs[ix].name);
        startup::trace(|| {
            format!(
                "showing {:?}",
                self.docs[ix]
                    .path
                    .as_deref()
                    .unwrap_or(Path::new("untitled"))
            )
        });
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
                doc.tab_width = titlebar::tab_width(ui::text_width(
                    &doc.name,
                    size::FONT_UI,
                    FontWeight::MEDIUM,
                    window,
                ));
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
                // Our own write must not look like someone else's change.
                doc.disk = crate::watch::stamp(path);
                doc.notice = None;
                if let Some(watcher) = &mut self.watcher {
                    watcher.include(path);
                }
                self.back_up(ix, cx);
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
        let closed = self.docs.remove(ix);
        Self::drop_backup(&closed.backup_key);
        // Ghosts are placed by where they were among the open tabs; keep that true.
        for ghost in &mut self.ghost_tabs {
            if ghost.at > ix {
                ghost.at -= 1;
            }
        }
        self.ghost_tabs.push(titlebar::GhostTab {
            at: ix,
            name: closed.name.clone(),
            width: closed.tab_width,
            closed: Instant::now(),
        });
        self.switches += 1;
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
        cx.spawn(async move |this, cx| {
            if confirmed.await {
                this.update(cx, |this, _| this.drop_backups()).ok();
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
        if let Some(watcher) = &mut self.watcher {
            watcher.set_root(root.as_deref());
        }
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
            self.toggle_folder(ix, cx);
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

    /// Change the editor font or its size, and remember the choice.
    fn update_settings(&mut self, change: impl FnOnce(&mut Settings), cx: &mut Context<Self>) {
        let mut settings = *cx.global::<Settings>();
        change(&mut settings);
        settings.font_size = settings::clamp_size(settings.font_size);
        if settings == *cx.global::<Settings>() {
            return;
        }
        theme::set_editor_font(&settings, cx);
        cx.set_global(settings);
        cx.background_executor()
            .spawn(async move { settings.save() })
            .detach();
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

    /// Say how smoothly the last script step was drawn. An animation shows up
    /// as an unbroken run of frames right after the step: this reports how
    /// long that run lasted and the longest wait inside it. A run much shorter
    /// than the animation means it stalled partway.
    #[cfg(feature = "shot")]
    pub fn report_frames(&mut self) {
        let frames = std::mem::take(&mut self.frames);
        let gaps: Vec<f64> = frames
            .windows(2)
            .map(|pair| pair[1].duration_since(pair[0]).as_secs_f64() * 1000.)
            .collect();
        // The run ends at the first pause too long to be part of an animation.
        let run: Vec<f64> = gaps.iter().copied().take_while(|gap| *gap < 30.).collect();
        if run.len() >= 3 {
            let longest = run.iter().copied().fold(0., f64::max);
            let span: f64 = run.iter().sum();
            eprintln!(
                "  ran {span:.0} ms in {} frames, longest wait {longest:.1} ms",
                run.len() + 1
            );
        }
    }

    /// One step of a `ZWINMD_SCRIPT`, standing in for the user in scripted checks:
    /// `editor:<px>` and `preview:<px>` scroll a pane, `type:<text>` types at the
    /// caret, `line:<n>` moves the caret, `view:editor|split|preview`, `sidebar`,
    /// `sync`, `new`, `open:<path>`, `save` and `close` run those commands,
    /// `link:<url>` follows a link, `check:<n>` clicks the n-th task checkbox,
    /// `tree:<n>` clicks the n-th row of the file list, `palette` and `theme`
    /// toggle those, `font:<name>` and `size:<n>` set the editor font.
    #[cfg(feature = "shot")]
    pub fn debug_step(&mut self, step: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.report_frames();
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
            "wait" => {}
            "sync" => self.toggle_scroll_sync(cx),
            "new" => self.open_untitled(window, cx),
            "open" => self.open(PathBuf::from(argument), window, cx),
            "save" => self.save_doc(id, false, window, cx).detach(),
            "close" => self.request_close(self.active, window, cx),
            "palette" => match self.palette {
                Some(_) => self.close_palette(window, cx),
                None => self.open_palette(window, cx),
            },
            "find" => window.dispatch_action(Box::new(edit::Search), cx),
            // File actions on a row of the file list: `context:<n>` picks the row
            // as a right-click would, then `newfile`, `newfolder`, `rename`, `trash`
            // act on it; `name:<text>` types a name and presses Return.
            "context" => {
                let row = argument
                    .parse()
                    .ok()
                    .and_then(|ix: usize| self.tree.rows.get(ix));
                self.context_row = row.map(|row| row.path.clone());
            }
            "newfile" => self.new_file_here(window, cx),
            "newfolder" => self.new_folder_here(window, cx),
            "rename" => self.rename_here(window, cx),
            "trash" => self.trash_here(window, cx),
            "name" => self.debug_name(argument, window, cx),
            "search" => self.open_search(window, cx),
            // Type into the palette, whichever kind is open.
            "query" => {
                if let Some(palette) = &self.palette {
                    palette.type_query(argument, window, cx);
                }
            }
            // Choose a row of the palette: `pick:<group>.<row>`.
            "pick" => {
                let (group, row) = argument.split_once('.').unwrap_or((argument, "0"));
                let at = gpui_kit::component::IndexPath::new(row.parse().unwrap_or(0))
                    .section(group.parse().unwrap_or(0));
                self.run_palette_target(at, window, cx);
            }
            "tree" => self.on_tree_click(argument.parse().unwrap_or(0), window, cx),
            "theme" => theme::toggle(cx),
            "font" => {
                let font = EditorFont::ALL
                    .into_iter()
                    .find(|font| font.label() == argument);
                self.update_settings(|s| s.font = font.unwrap_or(s.font), cx);
            }
            "size" => self.update_settings(
                |s| s.font_size = argument.parse().unwrap_or(s.font_size),
                cx,
            ),
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
        window.on_next_frame(move |window, cx| {
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
                this.restore_tabs(window, cx);
                this.restore_backups(window, cx);
                this.start_watching(window, cx);
                cx.notify();
            })
            .ok();
        });
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.time_first_frame(window, cx);
        self.remember_session(window, cx);
        theme::tick(window, cx);
        #[cfg(feature = "shot")]
        self.frames.push(Instant::now());
        self.sync_scroll(window, cx);
        let p = *cx.palette();
        // The sidebar slides and the panes glide between views; both are just
        // widths on their way to a target.
        let sidebar_target = if self.sidebar_open {
            self.sidebar_width
        } else {
            px(0.)
        };
        let sidebar_width = transition(
            "sidebar-width",
            sidebar_target,
            motion::glide(motion::PANEL, cx),
            window,
            cx,
        );
        let share_target = match self.view {
            ViewMode::Editor => 1.,
            ViewMode::Split => self.split,
            ViewMode::Preview => 0.,
        };
        let share = transition(
            "editor-share",
            share_target,
            motion::glide(motion::PANEL, cx),
            window,
            cx,
        );
        let panes_width = window.viewport_size().width - sidebar_width;

        let titlebar = self.render_titlebar(window, cx);
        // The sidebar is kept drawn even when shut, so opening it has nothing to build.
        let sidebar = self.render_sidebar(window, cx);
        let panes_target = (window.viewport_size().width - sidebar_target, share_target);
        let panes = self.render_panes(panes_width, share, panes_target, cx);
        let statusbar = self.render_statusbar(cx);
        let palette = self.render_palette(window, cx);
        let full_sidebar = self.sidebar_width;
        let sidebar_handle = (self.sidebar_open && sidebar_width == full_sidebar)
            .then(|| motion::resize_handle("sidebar-handle", sidebar_width, DragSidebar, &p));

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
            .on_action(
                cx.listener(|this, _: &NewFileHere, window, cx| this.new_file_here(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &NewFolderHere, window, cx| this.new_folder_here(window, cx)),
            )
            .on_action(cx.listener(|this, _: &RenameFile, window, cx| this.rename_here(window, cx)))
            .on_action(cx.listener(|this, _: &TrashFile, window, cx| this.trash_here(window, cx)))
            .on_action(cx.listener(|this, _: &RevealFile, _, cx| this.reveal_here(cx)))
            .on_action(cx.listener(|this, _: &CopyFilePath, _, cx| this.copy_path_here(cx)))
            .on_action(
                cx.listener(|this, _: &FindInFolder, window, cx| this.open_search(window, cx)),
            )
            .on_action(cx.listener(|this, _: &ExportHtml, window, cx| this.export_html(window, cx)))
            .on_action(cx.listener(|this, _: &FontBigger, _, cx| {
                this.update_settings(|s| s.font_size += 1., cx)
            }))
            .on_action(cx.listener(|this, _: &FontSmaller, _, cx| {
                this.update_settings(|s| s.font_size -= 1., cx)
            }))
            .on_action(cx.listener(|this, _: &FontReset, _, cx| {
                this.update_settings(|s| s.font_size = settings::DEFAULT_FONT_SIZE, cx)
            }))
            .on_action(cx.listener(|this, _: &UseJetBrainsMono, _, cx| {
                this.update_settings(|s| s.font = EditorFont::JetBrainsMono, cx)
            }))
            .on_action(cx.listener(|this, _: &UseFiraCode, _, cx| {
                this.update_settings(|s| s.font = EditorFont::FiraCode, cx)
            }))
            .on_action(cx.listener(|this, _: &UseMenlo, _, cx| {
                this.update_settings(|s| s.font = EditorFont::Menlo, cx)
            }))
            .on_action(cx.listener(|this, _: &Quit, window, cx| this.quit(window, cx)))
            .on_drop(cx.listener(|this, dropped: &ExternalPaths, window, cx| {
                this.open_requested(dropped.paths().to_vec(), window, cx);
            }))
            .on_drag_move::<DragSidebar>(cx.listener(
                |this, drag: &DragMoveEvent<DragSidebar>, _, cx| {
                    this.sidebar_width = drag.event.position.x.clamp(px(180.), px(480.));
                    cx.notify();
                },
            ))
            .on_drag_move::<panes::DragSplit>(cx.listener(
                |this, drag: &DragMoveEvent<panes::DragSplit>, window, cx| {
                    let left = if this.sidebar_open {
                        this.sidebar_width
                    } else {
                        px(0.)
                    };
                    let width = window.viewport_size().width - left;
                    this.split = ((drag.event.position.x - left) / width).clamp(0.2, 0.8);
                    cx.notify();
                },
            ))
            .child(titlebar)
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    // The sidebar keeps its full width and slides out to the left
                    // from under a window that narrows, so its text never re-wraps.
                    .child(
                        div()
                            .relative()
                            .flex_none()
                            .h_full()
                            .w(sidebar_width)
                            .overflow_hidden()
                            .when(sidebar_width > px(0.5), |slot| {
                                slot.border_r_1().border_color(p.border_subtle)
                            })
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .bottom_0()
                                    .right_0()
                                    .w(full_sidebar)
                                    .child(sidebar),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .child(match self.switches {
                                0 => panes,
                                _ if cx.reduce_motion() => panes,
                                // A document coming on show fades up, rather than cutting in.
                                switches => div()
                                    .size_full()
                                    .child(panes)
                                    .with_animation(
                                        ("document-arrives", switches),
                                        Animation::new(motion::QUICK).with_easing(motion::ease),
                                        |pane, progress| pane.opacity(0.3 + 0.7 * progress),
                                    )
                                    .into_any_element(),
                            }),
                    )
                    .children(sidebar_handle),
            )
            .child(statusbar)
            .children(palette)
    }
}
