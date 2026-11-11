//! The main window: open documents, the layout around them, and the commands
//! that change either. Each strip of the window draws itself in a submodule.

mod backup;
mod disk;
mod fileops;
mod find;
mod frame;
mod hover;
mod menu;
mod palette;
mod panel;
mod panes;
mod papers;
mod reader;
mod search_window;
mod statusbar;
mod switcher;
mod trail;
mod updates;
mod viewer;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui_kit::base::input as edit;
use gpui_kit::base::transition;
use gpui_kit::component::input::{EditorState, InputEvent};
use gpui_kit::component::text::TextViewState;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

// Scripted runs choose their own window size.
#[cfg_attr(feature = "shot", allow(unused_imports))]
pub use frame::{page_size, page_width};
pub use papers::{open_paper, open_requested, page_bounds};

use crate::doc::{self, Heading, Stats};
use crate::files::{self, Folder};
use crate::motion;
use crate::preview::BlockMap;
use crate::session::{FolderState, Session};
use crate::settings::{self, EditorFont, Settings};
use crate::startup;
use crate::sync::{ScrollSync, Side};
use crate::system::{self, command};
use crate::theme::{self, ActivePalette as _, size};

actions!(
    zinmd,
    [
        NewFile,
        NewWindow,
        SideBySide,
        OpenFile,
        Save,
        SaveAs,
        CloseFile,
        LastFile,
        ToggleMode,
        ViewEditor,
        ViewSplit,
        ViewPreview,
        ToggleSidebar,
        ToggleScrollSync,
        ToggleTheme,
        FocusSearch,
        OpenPalette,
        FindInFolder,
        ExportHtml,
        NewFileHere,
        RenameFile,
        TrashFile,
        RevealFile,
        CheckForUpdates,
        ToggleUpdates,
        LookCloser,
        ToggleContrast,
        FindOnPage,
        FindNext,
        FindBefore,
        CopyFilePath,
        CopyFileContent,
        PreviousSection,
        NextSection,
        GoBack,
        GoForward,
        PreviousFile,
        NextFile,
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
        KeyBinding::new(command!("n"), NewFile, None),
        KeyBinding::new(command!("shift-n"), NewWindow, None),
        // The papers in a row across the screen, and back where they were.
        KeyBinding::new(command!("alt-s"), SideBySide, None),
        KeyBinding::new(command!("o"), OpenFile, None),
        KeyBinding::new(command!("s"), Save, None),
        KeyBinding::new(command!("shift-s"), SaveAs, None),
        KeyBinding::new(command!("w"), CloseFile, None),
        KeyBinding::new("ctrl-tab", LastFile, None),
        // The same key goes to the editor and comes back to the page.
        KeyBinding::new(command!("e"), ToggleMode, None),
        // The command key with 1 to 9 belongs to the files open on their own, as it did to tabs.
        KeyBinding::new(command!("alt-1"), ViewEditor, None),
        KeyBinding::new(command!("alt-2"), ViewSplit, None),
        KeyBinding::new(command!("alt-3"), ViewPreview, None),
        KeyBinding::new(command!("\\"), ToggleSidebar, None),
        KeyBinding::new(command!("k"), FocusSearch, None),
        KeyBinding::new(command!("p"), OpenPalette, None),
        KeyBinding::new(command!("shift-c"), CopyFileContent, None),
        KeyBinding::new(command!("alt-c"), CopyFilePath, None),
        KeyBinding::new("alt-up", PreviousSection, None),
        KeyBinding::new("alt-down", NextSection, None),
        // Back to where you were and on again, as in a browser. In the
        // editor these keys keep what they mean there.
        KeyBinding::new(command!("["), GoBack, None),
        KeyBinding::new(command!("]"), GoForward, None),
        KeyBinding::new("alt-left", GoBack, None),
        KeyBinding::new("alt-right", GoForward, None),
        // The file before and after this one in its folder: the keys that
        // go from tab to tab elsewhere.
        KeyBinding::new(command!("shift-["), PreviousFile, None),
        KeyBinding::new(command!("shift-]"), NextFile, None),
        KeyBinding::new(command!("{"), PreviousFile, None),
        KeyBinding::new(command!("}"), NextFile, None),
        KeyBinding::new(command!("shift-f"), FindInFolder, None),
        KeyBinding::new(command!("shift-e"), ExportHtml, None),
        KeyBinding::new(command!("="), FontBigger, None),
        KeyBinding::new(command!("+"), FontBigger, None),
        KeyBinding::new(command!("-"), FontSmaller, None),
        KeyBinding::new(command!("0"), FontReset, None),
        KeyBinding::new(command!("q"), Quit, None),
        // On a page. In the editor these keys are the editor's own search.
        KeyBinding::new(command!("f"), FindOnPage, None),
        KeyBinding::new(command!("g"), FindNext, None),
        KeyBinding::new(command!("shift-g"), FindBefore, None),
    ]);
    // Reached only when no window is there to ask about unsaved changes.
    cx.on_action(|_: &Quit, cx| cx.quit());
}

/// Install the menu bar. Building it makes AppKit load extra frameworks (about
/// 11 ms), so this is called once the first frame is on screen.
pub fn install_menus(cx: &mut App) {
    cx.set_menus(vec![
        Menu::new("ZinMD").items([
            MenuItem::action("Check for Updates…", CheckForUpdates),
            MenuItem::separator(),
            MenuItem::action("Quit ZinMD", Quit),
        ]),
        Menu::new("File").items([
            MenuItem::action("New", NewFile),
            MenuItem::action("New Window", NewWindow),
            MenuItem::action("Open…", OpenFile),
            MenuItem::separator(),
            MenuItem::action("Save", Save),
            MenuItem::action("Save As…", SaveAs),
            MenuItem::action("Export as HTML…", ExportHtml),
            MenuItem::separator(),
            MenuItem::action("Rename…", RenameFile),
            MenuItem::action(system::REVEAL, RevealFile),
            MenuItem::separator(),
            MenuItem::action("Copy Content", CopyFileContent),
            MenuItem::action("Copy Path", CopyFilePath),
            MenuItem::separator(),
            MenuItem::action("Close File", CloseFile),
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
            MenuItem::action("Find on the Page…", FindOnPage),
            MenuItem::action("Find in All Files…", FindInFolder),
        ]),
        Menu::new("View").items([
            MenuItem::action("Read or Edit", ToggleMode),
            MenuItem::separator(),
            MenuItem::action("Editor", ViewEditor),
            MenuItem::action("Split", ViewSplit),
            MenuItem::action("Preview", ViewPreview),
            MenuItem::separator(),
            MenuItem::action("Search Files", FocusSearch),
            MenuItem::action("Command Palette…", OpenPalette),
            MenuItem::action("Show or Hide Side Panel", ToggleSidebar),
            MenuItem::action("Previous Section", PreviousSection),
            MenuItem::action("Next Section", NextSection),
            MenuItem::action("Go Back to the Last File", LastFile),
            MenuItem::action("Papers Side by Side, or Back", SideBySide),
            MenuItem::action("Back", GoBack),
            MenuItem::action("Forward", GoForward),
            MenuItem::action("Previous File in the Folder", PreviousFile),
            MenuItem::action("Next File in the Folder", NextFile),
            MenuItem::action("Turn Scroll Sync On or Off", ToggleScrollSync),
            MenuItem::action("Switch Light and Dark", ToggleTheme),
            MenuItem::separator(),
            MenuItem::action("Bigger Text", FontBigger),
            MenuItem::action("Smaller Text", FontSmaller),
            MenuItem::action("Default Text Size", FontReset),
            MenuItem::action("Black and White", ToggleContrast),
            MenuItem::separator(),
            MenuItem::action("Font: JetBrains Mono", UseJetBrainsMono),
            MenuItem::action("Font: Fira Code", UseFiraCode),
            MenuItem::action("Font: Menlo", UseMenlo),
        ]),
    ]);
}

/// What dragging the edge of the sidebar carries.
struct DragSidebar;

/// What the window is: a page to read, or the editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Read,
    Edit,
}

/// Which panes the editor shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    Editor,
    Split,
    Preview,
}

/// One open document.
pub struct Doc {
    /// Stable for as long as it is open, so per-document UI state (scroll, selection) never mixes.
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
    /// What the page shows, when that is not the source as it is: its wiki
    /// links are made links and its footnotes numbered. It says where its
    /// places are in the source, and it is what the page's blocks are
    /// counted in: two footnotes are two blocks of the source and one list
    /// of the page.
    links: Option<crate::links::Shown>,
    sync: ScrollSync,
    /// What its file looked like when we last read or wrote it, to tell when
    /// someone else changes it; and anything the user should be told about it.
    disk: Option<(std::time::SystemTime, u64)>,
    notice: Option<disk::Notice>,
    /// Names the file its unsaved edits are backed up to, and the pending write.
    backup_key: String,
    _backup: Task<()>,
    _changes: Subscription,
    _blocks: Task<()>,
}

impl Doc {
    fn dirty(&self) -> bool {
        self.text != self.saved
    }

    /// An empty document nobody named or typed in: only a place to start from.
    fn blank(&self) -> bool {
        self.path.is_none() && self.text.is_empty()
    }
}

pub struct Workspace {
    /// Whether this is the first paper of the run: the one that brings back
    /// what was open last time, and looks for updates.
    primary: bool,
    docs: Vec<Doc>,
    active: usize,
    next_doc_id: usize,
    mode: Mode,
    /// Counts changes of mode, so each one can fade in.
    mode_switches: usize,
    view: ViewMode,
    sidebar_open: bool,
    /// Whether the outline at the foot of the side panel shows its headings, or only its title.
    outline_open: bool,
    sync_enabled: bool,
    /// The folders of the side panel, in the order they are shown.
    folders: Vec<Folder>,
    /// Folders being read from disk, each off the main thread.
    _scans: HashMap<PathBuf, Task<()>>,
    /// The side panel: where it is scrolled, what is typed in its search, what the pointer is on.
    panel: panel::Panel,
    /// The page: its menu, its rail, how far each file has been read.
    reader: reader::Reader,
    trail: trail::Trail,
    /// The document that was on show before this one, to go back to.
    last_shown: Option<usize>,
    /// Files of the last session still to bring back.
    restore: Vec<PathBuf>,
    /// The session as last written down, to notice when it changes.
    saved_session: Option<Session>,
    session_checked: Instant,
    _session_save: Task<()>,
    _session_check: Task<()>,
    /// Where the window was while reading, and while editing: each shape is
    /// remembered while the window wears the other.
    page: Option<[f32; 4]>,
    desk: Option<[f32; 4]>,
    /// The window on its way from one shape to the other.
    reshaping: bool,
    _reshape: Task<()>,
    /// Frames left in which the source and the page are lined up after a change of mode.
    carry: u8,
    /// Tells us when files change on disk; started after the first frame.
    watcher: Option<crate::watch::Watcher>,
    _watching: Task<()>,
    /// The row of the file list that was last right-clicked: what its menu acts on.
    context_row: Option<PathBuf>,
    /// The row a menu is open on, shown marked until the menu goes.
    menu_row: Option<PathBuf>,
    /// The field for naming a new or renamed file, while one is being named.
    name_field: Option<fileops::NameField>,
    /// What the pointer is over, for hover that fades.
    hot: Option<hover::Hot>,
    /// Whether the menu of accent colours is down.
    colours_open: bool,
    /// Counts changes of the document on show, so each one can fade in.
    switches: usize,
    palette: Option<palette::CommandPalette>,
    _search: Task<()>,
    /// Set to give up the search of the files that is under way.
    search_stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// The palette just closed, fading out, and when it closed.
    palette_leaving: Option<(palette::CommandPalette, Instant)>,
    /// How wide the side panel is when open; dragging its edge changes this.
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
    /// How this copy was installed, and so how a new version takes its place.
    install: crate::update::Install,
    /// What there is to say about a new version, and whether the card that
    /// says it is up.
    update: updates::Update,
    update_shown: bool,
    /// A picture or a diagram opened large over the page, and whether it is
    /// up or on its way out.
    viewer: Option<viewer::Viewer>,
    viewer_open: bool,
    /// The field for finding words on the page, and whether it is up.
    find: Option<find::Find>,
    find_open: bool,
    /// The window that searches the text of every file, while it is open or
    /// on its way out.
    search: Option<search_window::SearchWindow>,
    search_open: bool,
    _viewer_save: Task<()>,
    _update_look: Task<()>,
    _update_work: Task<()>,
    _update_hide: Task<()>,
    /// When the process started, to report how long opening took.
    started: Instant,
    opened_ms: Option<f64>,
    first_frame_requested: bool,
    /// When each frame was drawn, to judge smoothness in scripted runs.
    #[cfg(feature = "shot")]
    frames: Vec<Instant>,
}

/// The window's place and size, as the session keeps it.
fn bounds_of(window: &Window) -> [f32; 4] {
    let bounds = match window.window_bounds() {
        WindowBounds::Windowed(bounds)
        | WindowBounds::Maximized(bounds)
        | WindowBounds::Fullscreen(bounds) => bounds,
    };
    [
        bounds.origin.x.into(),
        bounds.origin.y.into(),
        bounds.size.width.into(),
        bounds.size.height.into(),
    ]
}

impl Workspace {
    /// `path` may be a Markdown file, a folder, or nothing.
    pub fn new(
        path: Option<PathBuf>,
        session: Session,
        started: Instant,
        primary: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let panel = panel::Panel::new(session.panel_scroll, window, cx);
        let mut this = Self {
            primary,
            docs: Vec::new(),
            active: 0,
            next_doc_id: 0,
            // A file is opened to be read: the page comes first, the editor is one key away.
            mode: frame::starting_mode(),
            mode_switches: 0,
            view: match session.view.as_str() {
                "editor" => ViewMode::Editor,
                "preview" => ViewMode::Preview,
                _ => ViewMode::Split,
            },
            sidebar_open: session.sidebar_open,
            outline_open: session.outline_open,
            sync_enabled: session.scroll_sync,
            folders: Vec::new(),
            _scans: HashMap::new(),
            panel,
            reader: reader::Reader::new(session.progress),
            trail: Default::default(),
            last_shown: None,
            restore: Vec::new(),
            saved_session: None,
            session_checked: started,
            _session_save: Task::ready(()),
            _session_check: Task::ready(()),
            page: session.page,
            desk: session.desk,
            reshaping: false,
            _reshape: Task::ready(()),
            carry: 0,
            watcher: None,
            _watching: Task::ready(()),
            context_row: None,
            menu_row: None,
            name_field: None,
            hot: None,
            colours_open: false,
            switches: 0,
            palette: None,
            _search: Task::ready(()),
            search_stop: Default::default(),
            palette_leaving: None,
            sidebar_width: px(session.sidebar_width),
            split: session.split,
            glide: None,
            settled: (px(0.), px(0.)),
            focus: cx.focus_handle(),
            close_confirmed: false,
            install: crate::update::Install::detect(),
            update: updates::Update::Quiet,
            update_shown: false,
            viewer: None,
            viewer_open: false,
            find: None,
            find_open: false,
            search: None,
            search_open: false,
            _viewer_save: Task::ready(()),
            _update_look: Task::ready(()),
            _update_work: Task::ready(()),
            _update_hide: Task::ready(()),
            started,
            opened_ms: None,
            first_frame_requested: false,
            #[cfg(feature = "shot")]
            frames: Vec::new(),
        };

        // The folders of last time come back as they were left.
        for state in session.folders {
            if state.path.is_dir() && !this.folders.iter().any(|f| f.root == state.path) {
                let mut folder = Folder::new(state.path);
                folder.pinned = state.pinned;
                folder.open = state.open;
                folder.open_dirs = state.open_dirs.into_iter().collect();
                this.folders.push(folder);
            }
        }

        let path = path.map(|path| path.canonicalize().unwrap_or(path));
        let file = match path {
            Some(dir) if dir.is_dir() => {
                let readme = ["README.md", "readme.md", "index.md"]
                    .iter()
                    .map(|name| dir.join(name))
                    .find(|p| p.is_file());
                if !this.folders.iter().any(|folder| folder.root == dir) {
                    this.folders.push(Folder::new(dir));
                }
                readme
            }
            Some(file) => Some(file),
            // Nothing asked for: pick up where the last session left off.
            None => {
                let mut files: Vec<PathBuf> = session
                    .files
                    .into_iter()
                    .filter(|file| file.is_file())
                    .collect();
                let shown = session
                    .active
                    .filter(|file| file.is_file())
                    .or_else(|| files.first().cloned());
                files.retain(|file| Some(file) != shown.as_ref());
                this.restore = files;
                shown
            }
        };
        match file {
            Some(file) => this.open_path(&file, window, cx),
            None => this.open_untitled(window, cx),
        }
        let roots: Vec<PathBuf> = this.folders.iter().map(|f| f.root.clone()).collect();
        for root in roots {
            this.scan_folder(root, cx);
        }

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

        // The paper that comes to the front is the one used last.
        cx.observe_window_activation(window, |_, window, cx| {
            if window.is_window_active() {
                papers::came_forward(window.window_handle(), cx);
            }
        })
        .detach();

        startup::mark("document loaded");
        this
    }

    fn active_doc(&self) -> Option<&Doc> {
        self.docs.get(self.active)
    }

    fn doc_ix(&self, id: usize) -> Option<usize> {
        self.docs.iter().position(|doc| doc.id == id)
    }

    /// The folder of the panel that `path` lies in: the innermost, if folders nest.
    fn folder_of(&self, path: &Path) -> Option<usize> {
        self.folders
            .iter()
            .enumerate()
            .filter(|(_, folder)| folder.contains(path))
            .max_by_key(|(_, folder)| folder.root.as_os_str().len())
            .map(|(ix, _)| ix)
    }

    /// The documents open on their own, outside every folder of the panel, in
    /// the order they were opened. A blank page is not one of them.
    fn singles(&self) -> Vec<usize> {
        (0..self.docs.len())
            .filter(|ix| {
                let doc = &self.docs[*ix];
                match &doc.path {
                    Some(path) => self.folder_of(path).is_none(),
                    None => !doc.blank(),
                }
            })
            .collect()
    }

    /// Typing goes to the document: into the source when editing. A page is
    /// only read, so there the window itself listens, for the keys that scroll.
    fn focus_document(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match (self.mode, self.active_doc()) {
            (Mode::Edit, Some(doc)) if self.view != ViewMode::Preview => {
                doc.editor.update(cx, |editor, cx| editor.focus(window, cx))
            }
            _ => self.focus.focus(window, cx),
        }
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

    /// The system asked for `path`. A folder joins the side panel. A file
    /// is a new paper, unless this one is an empty page waiting for one.
    pub(super) fn open_asked(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = path.canonicalize().unwrap_or(path);
        let waiting = matches!(self.docs.as_slice(), [only] if only.blank());
        match path.is_dir() || waiting {
            true => {
                self.open(path, window, cx);
                window.activate_window();
            }
            false => self.open_beside(Some(path), window, cx),
        }
    }

    /// Open `path` as a paper of its own: a new window beside this one, a
    /// little down and to the right, with the same folders in its panel.
    /// With no path it is an empty page. A file that already has a paper is
    /// brought to the front instead.
    pub(super) fn open_beside(
        &mut self,
        path: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = path.map(|path| path.canonicalize().unwrap_or(path));
        if let Some(there) = path.as_deref().and_then(|path| papers::showing(path, cx)) {
            return cx.defer(move |cx| {
                there
                    .update(cx, |_, window, _| window.activate_window())
                    .ok();
            });
        }
        let mut session = self.session(window, cx).for_another_paper();
        // A sheet the size of this one, or of the page this window was
        // before it became the editor's.
        let from = bounds_of(window);
        let step: f32 = papers::next_step(cx).into();
        let frame = match (self.mode, self.page) {
            (Mode::Read, _) => from,
            (Mode::Edit, Some(page)) => [from[0], from[1], page[2], page[3]],
            (Mode::Edit, None) => {
                let page = page_size(size(px(from[2]), px(from[3])));
                [from[0], from[1], page.width.into(), page.height.into()]
            }
        };
        let frame = [frame[0] + step, frame[1] + step, frame[2], frame[3]];
        session.page = Some(frame);
        let started = Instant::now();
        // Once this window has finished what it is doing.
        cx.defer(move |cx| {
            let bounds = page_bounds(Some(frame), cx);
            open_paper(path, session, bounds, started, false, cx);
        });
    }

    /// Show a file, or add a folder to the side panel.
    fn open(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let path = path.canonicalize().unwrap_or(path);
        if path.is_dir() {
            self.add_folder(path, cx);
            return;
        }
        self.leave_place(cx);
        // An untouched blank page was only a placeholder: the file takes its place.
        let placeholder = match self.docs.as_slice() {
            [only] if only.blank() => Some(only.id),
            _ => None,
        };
        self.open_path(&path, window, cx);
        if let Some(ix) = placeholder.and_then(|id| self.doc_ix(id)) {
            self.docs.remove(ix);
            self.active = self.active.saturating_sub(1);
            self.last_shown = None;
        }
    }

    /// Show `path`, reusing the document if it is already open.
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

    /// A new document is for writing: it opens in the editor.
    fn new_document(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.active_doc().is_some_and(Doc::blank) {
            self.open_untitled(window, cx);
        }
        self.set_mode(Mode::Edit, window, cx);
    }

    fn push_doc(
        &mut self,
        path: Option<PathBuf>,
        name: SharedString,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ix = self.docs.len();
        self.insert_doc(ix, path, name, text, window, cx);
        self.activate(ix, window, cx);
    }

    /// Add a document at place `at`, without bringing it on show.
    fn insert_doc(
        &mut self,
        at: usize,
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
        let links = self.page_text(path.as_deref(), &text);
        let preview = cx.new(|cx| match &links {
            Some(shown) => TextViewState::markdown(&shown.text, cx),
            None => TextViewState::markdown(&text, cx),
        });
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
            links,
            sync: ScrollSync::default(),
            disk,
            notice: None,
            // Unique among everything this app has ever backed up: start time and a counter.
            backup_key: format!(
                "{}-{id}",
                self.started.elapsed().as_nanos() ^ std::process::id() as u128
            ),
            _backup: Task::ready(()),
            _changes: changes,
            _blocks: Task::ready(()),
        };
        let at = at.min(self.docs.len());
        self.docs.insert(at, doc);
        // Documents after the new one moved along by one.
        if self.docs.len() > 1 && self.active >= at {
            self.active += 1;
        }
        self.map_blocks(at, Duration::ZERO, cx);
    }

    /// Bring back the other files of the last session, behind the one already
    /// open. Called after the first frame, so they do not hold up launch.
    fn restore_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for path in std::mem::take(&mut self.restore) {
            if self
                .docs
                .iter()
                .any(|doc| doc.path.as_deref() == Some(path.as_path()))
            {
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let text = String::from_utf8_lossy(&bytes).into_owned();
            let at = self.docs.len();
            self.insert_doc(at, Some(path), name.into(), text, window, cx);
        }
        cx.notify();
    }

    /// The state worth remembering for next time.
    fn session(&self, window: &Window, cx: &App) -> Session {
        // While the window is on its way between its two shapes, its size is neither's.
        let here = (!self.reshaping).then(|| bounds_of(window));
        let (page, desk) = match self.mode {
            Mode::Read => (here.or(self.page), self.desk),
            Mode::Edit => (self.page, here.or(self.desk)),
        };
        let files: Vec<PathBuf> = self
            .singles()
            .into_iter()
            .filter_map(|ix| self.docs[ix].path.clone())
            .collect();
        Session {
            page,
            desk,
            folders: self
                .folders
                .iter()
                .map(|folder| {
                    let mut open_dirs: Vec<PathBuf> = folder.open_dirs.iter().cloned().collect();
                    open_dirs.sort();
                    FolderState {
                        path: folder.root.clone(),
                        pinned: folder.pinned,
                        open: folder.open,
                        open_dirs,
                    }
                })
                .collect(),
            files,
            active: self.active_doc().and_then(|doc| doc.path.clone()),
            view: match self.view {
                ViewMode::Editor => "editor",
                ViewMode::Split => "split",
                ViewMode::Preview => "preview",
            }
            .into(),
            sidebar_open: self.sidebar_open,
            sidebar_width: self.sidebar_width.into(),
            outline_open: self.outline_open,
            split: self.split,
            scroll_sync: self.sync_enabled,
            panel_scroll: self.panel.scrolled(),
            progress: self.reader.progress.clone(),
            papers: papers::others(window.window_handle(), cx),
        }
    }

    /// Write the session down if it changed. Looked at as frames are drawn,
    /// which is simpler and safer than remembering to at every place that
    /// changes it, but no more than a few times a second; the write itself
    /// waits for things to settle and happens off the main thread.
    fn remember_session(&mut self, window: &Window, cx: &mut Context<Self>) {
        const EVERY: Duration = Duration::from_millis(300);
        if self.session_checked.elapsed() < EVERY {
            // Nothing may be drawn again for a long while: look once more, shortly.
            self._session_check = cx.spawn(async move |this, cx| {
                cx.background_executor().timer(EVERY).await;
                this.update(cx, |_, cx| cx.notify()).ok();
            });
            return;
        }
        self.session_checked = Instant::now();
        self._session_check = Task::ready(());
        // Each paper says where it is. The one in front writes the session
        // down: it is what was looked at last.
        let shows = self.active_doc().and_then(|doc| doc.path.clone());
        papers::note(window.window_handle(), shows, bounds_of(window), cx);
        if !window.is_window_active() && papers::count(cx) > 1 {
            return;
        }
        let session = self.session(window, cx);
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
        let links = self.page_text(self.docs[ix].path.clone().as_deref(), &text);
        let doc = &mut self.docs[ix];
        doc.headings = doc::outline(&text);
        doc.stats = Stats::of(&text);
        doc.preview.update(cx, |preview, cx| match &links {
            Some(shown) => preview.set_text(&shown.text, cx),
            None => preview.set_text(&text, cx),
        });
        doc.links = links;
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
        // The blocks are the page's: counted in what it shows.
        let text: SharedString = match &doc.links {
            Some(shown) => shown.text.clone().into(),
            None => doc.text.clone(),
        };
        let id = doc.id;
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
            self.last_shown = self.docs.get(self.active).map(|doc| doc.id);
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
        self.focus_document(window, cx);
        self.reveal_active();
        // A file was just chosen: the panel that floats over the page has done its job.
        self.reader.panel_open = false;
        self.reader.wake(cx);
        cx.notify();
    }

    /// Open whatever is shut on the way to the document on show, and bring
    /// its row into view in the side panel.
    fn reveal_active(&mut self) {
        let Some(path) = self.active_doc().and_then(|doc| doc.path.clone()) else {
            return;
        };
        if let Some(folder) = self.folder_of(&path) {
            self.folders[folder].reveal(&path);
        }
        self.panel.show(path);
    }

    /// Go back to the document that was on show before this one.
    fn last_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.last_shown.and_then(|id| self.doc_ix(id)) {
            self.leave_place(cx);
            self.activate(ix, window, cx);
        }
    }

    /// Show the n-th of the files open on their own, counting from 0.
    fn jump_to_single(&mut self, nth: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.singles().get(nth).copied() {
            self.leave_place(cx);
            self.activate(ix, window, cx);
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
            .or_else(|| self.folders.first().map(|folder| folder.root.clone()))
            .or_else(system::home)
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
                // The new file may belong to one of the folders in the side panel.
                if let Some(folder) = this.folder_of(&path) {
                    let root = this.folders[folder].root.clone();
                    this.scan_folder(root, cx);
                }
                if this.active == ix {
                    window.set_window_title(&this.docs[ix].name);
                    this.reveal_active();
                }
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

    /// Close a document, first offering to save it if it has unsaved changes.
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
        self.switches += 1;
        // The window stays: with nothing left to show, it shows a blank page to start from.
        if self.docs.is_empty() {
            self.active = 0;
            self.last_shown = None;
            self.open_untitled(window, cx);
            return;
        }
        // Back to the document that was on show before, if it is still open.
        let before = self
            .last_shown
            .filter(|last| *last != id)
            .and_then(|last| self.doc_ix(last));
        let next = before.unwrap_or(if ix < self.active {
            self.active - 1
        } else {
            self.active.min(self.docs.len() - 1)
        });
        // Whatever was the active index now points elsewhere: the one to show is chosen afresh.
        self.active = usize::MAX;
        self.last_shown = None;
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
        let (text, name, path) = (doc.text.clone(), doc.name.clone(), doc.path.clone());
        // The page goes out as it is shown: wiki links as links.
        let text = match self.page_text(path.as_deref(), &text) {
            Some(shown) => shown.text.into(),
            None => text,
        };
        let folder = path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .or_else(system::home)
            .unwrap_or_default();
        let suggested = Path::new(name.as_ref()).with_extension("html");
        let accent = theme::accent(cx);
        let chosen = cx.prompt_for_new_path(&folder, suggested.to_str());
        cx.spawn_in(window, async move |_, cx| {
            let Ok(Ok(Some(path))) = chosen.await else {
                return;
            };
            // Diagrams are drawn during export, which can take a moment.
            let page = cx
                .background_executor()
                .spawn(async move { crate::export::html(&text, &name, accent) })
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

    // ------------------------------------------------------------- folders

    /// Add a folder to the side panel, or show it if it is already there.
    /// The folder is read off the main thread.
    fn add_folder(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        match self.folders.iter().position(|folder| folder.root == root) {
            Some(ix) => self.folders[ix].open = true,
            None => {
                self.folders.push(Folder::new(root.clone()));
                self.scan_folder(root.clone(), cx);
            }
        }
        self.watch_folders();
        self.panel.show(root);
        // A folder was asked for: its files are what there is to look at now.
        match self.mode {
            Mode::Edit => self.sidebar_open = true,
            Mode::Read => self.reader.panel_open = true,
        }
        cx.notify();
    }

    /// Take a folder off the side panel. Its files stay where they are on disk.
    fn remove_folder(&mut self, root: &Path, cx: &mut Context<Self>) {
        self.folders.retain(|folder| folder.root != root);
        self._scans.remove(root);
        self.watch_folders();
        cx.notify();
    }

    /// Pin a folder to the top of the panel, or let it go.
    fn toggle_pin(&mut self, root: &Path, cx: &mut Context<Self>) {
        if let Some(folder) = self.folders.iter_mut().find(|folder| folder.root == root) {
            folder.pinned = !folder.pinned;
        }
        cx.notify();
    }

    /// Read a folder's Markdown files again. A newer request replaces one still running.
    fn scan_folder(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        let task = cx.spawn({
            let root = root.clone();
            async move |this, cx| {
                let walked = root.clone();
                let found = cx
                    .background_executor()
                    .spawn(async move { files::scan(&walked, 5000) })
                    .await;
                this.update(cx, |this, cx| {
                    let Some(ix) = this.folders.iter().position(|folder| folder.root == root)
                    else {
                        return;
                    };
                    startup::trace(|| format!("read {root:?}: {} files", found.len()));
                    this.folders[ix].set_files(found);
                    this.relink_pages(cx);
                    // The file on show may be in a subfolder that is still shut.
                    let shown = this.active_doc().and_then(|doc| doc.path.clone());
                    if let Some(path) = shown.filter(|path| this.folder_of(path) == Some(ix)) {
                        this.folders[ix].reveal(&path);
                    }
                    // Reading the folder again dropped the row a new name is being typed in.
                    this.place_draft();
                    cx.notify();
                })
                .ok();
            }
        });
        self._scans.insert(root, task);
    }

    /// Read a folder again shortly. Several changes in a row cause one read.
    fn scan_folder_soon(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        let task = cx.spawn({
            let root = root.clone();
            async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                this.update(cx, |this, cx| this.scan_folder(root, cx)).ok();
            }
        });
        self._scans.insert(root, task);
    }

    /// Tell the watcher which folders are in the panel now.
    fn watch_folders(&mut self) {
        if let Some(watcher) = &mut self.watcher {
            let roots: Vec<PathBuf> = self.folders.iter().map(|f| f.root.clone()).collect();
            watcher.set_roots(&roots);
        }
    }

    /// Every Markdown file of every folder in the panel.
    fn all_files(&self) -> Vec<PathBuf> {
        self.folders
            .iter()
            .flat_map(|folder| folder.files.iter().cloned())
            .collect()
    }

    /// The text of a document as its page shows it, when that is not the
    /// text itself: each wiki link that names a file the app knows is made
    /// an ordinary link to it, and footnotes are numbered. The files a wiki
    /// link can name are those of the panel's folders, those that are open,
    /// and those beside the document.
    fn page_text(&mut self, path: Option<&Path>, text: &str) -> Option<crate::links::Shown> {
        if !crate::links::may_link(text) {
            return None;
        }
        // Footnotes need no file to be known.
        if !text.contains("[[") {
            return crate::links::linked(text, |_| None);
        }
        let folder = path.and_then(Path::parent);
        let beside = match path {
            Some(path) if self.folder_of(path).is_none() => self.beside(path),
            _ => Vec::new(),
        };
        let known = crate::links::Known::new(
            self.folders
                .iter()
                .flat_map(|folder| &folder.files)
                .chain(self.docs.iter().filter_map(|doc| doc.path.as_ref()))
                .chain(&beside)
                .map(PathBuf::as_path),
        );
        crate::links::linked(text, |name| {
            let file = known.find(name, folder)?;
            Some(crate::links::way(folder, file))
        })
    }

    /// Make the wiki links of the open pages again: the files they can name
    /// have changed.
    fn relink_pages(&mut self, cx: &mut Context<Self>) {
        for ix in 0..self.docs.len() {
            if !self.docs[ix].text.contains("[[") {
                continue;
            }
            let (path, text) = (self.docs[ix].path.clone(), self.docs[ix].text.clone());
            let links = self.page_text(path.as_deref(), &text);
            let doc = &mut self.docs[ix];
            // Only a page whose links changed is drawn again.
            let was = doc.preview.read(cx).rendered_text().source().len();
            let now = links.as_ref().map_or(text.len(), |shown| shown.text.len());
            if was != now {
                doc.preview.update(cx, |preview, cx| match &links {
                    Some(shown) => preview.set_text(&shown.text, cx),
                    None => preview.set_text(&text, cx),
                });
            }
            doc.links = links;
            self.map_blocks(ix, Duration::ZERO, cx);
        }
    }

    /// Every file a search of the text looks in: the one on show first, so
    /// that it comes first among equals, then the others that are open, then
    /// those of the folders.
    fn files_to_search(&self) -> Vec<PathBuf> {
        let shown = self.active_doc().and_then(|doc| doc.path.clone());
        let open = self.docs.iter().filter_map(|doc| doc.path.clone());
        let mut seen = std::collections::HashSet::new();
        shown
            .into_iter()
            .chain(open)
            .chain(self.all_files())
            .filter(|path| seen.insert(path.clone()))
            .collect()
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

    /// Change the accent colour, the editor font or its size, and remember the choice.
    fn update_settings(&mut self, change: impl FnOnce(&mut Settings), cx: &mut Context<Self>) {
        let before = *cx.global::<Settings>();
        let mut settings = before;
        change(&mut settings);
        settings.font_size = settings::clamp_size(settings.font_size);
        settings.reading_scale = settings::clamp_scale(settings.reading_scale);
        if settings == before {
            return;
        }
        if (settings.accent, settings.contrast) != (before.accent, before.contrast) {
            theme::set_colours(&settings, cx);
        }
        if (settings.font, settings.font_size) != (before.font, before.font_size) {
            theme::set_editor_font(&settings, cx);
        }
        cx.set_global(settings);
        cx.background_executor()
            .spawn(async move { settings.save() })
            .detach();
        cx.notify();
    }

    /// Show or hide the side panel. Beside the editor it takes its place in
    /// the window; over a page it floats, and takes the keyboard while it is there.
    fn toggle_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.mode {
            Mode::Edit => self.sidebar_open = !self.sidebar_open,
            Mode::Read => {
                self.reader.panel_open = !self.reader.panel_open;
                match self.reader.panel_open {
                    true => self.panel.focus(window, cx),
                    false => self.focus_document(window, cx),
                }
            }
        }
        cx.notify();
    }

    /// Whether the side panel is on show, in either mode.
    fn panel_shown(&self) -> bool {
        match self.mode {
            Mode::Edit => self.sidebar_open,
            Mode::Read => self.reader.panel_open,
        }
    }

    /// Put the caret in the side panel's search field, opening the panel if it is shut.
    fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.mode {
            Mode::Edit => self.sidebar_open = true,
            Mode::Read => self.reader.panel_open = true,
        }
        self.panel.focus_search(window, cx);
        cx.notify();
    }

    /// Switch between the page and the editor. The window takes the shape of
    /// each, and the document stays where it was being read.
    fn set_mode(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode == mode {
            return;
        }
        if !self.reshaping {
            let here = bounds_of(window);
            match self.mode {
                Mode::Read => self.page = Some(here),
                Mode::Edit => self.desk = Some(here),
            }
        }
        // The page is the preview of the editor, so it is already in place when
        // the editor shows one. The source has to be lined up with it; and the
        // page with the source when the editor was showing the source alone.
        let lead = match (mode, self.view) {
            (Mode::Edit, _) => Some(Side::Preview),
            (Mode::Read, ViewMode::Editor) => Some(Side::Editor),
            (Mode::Read, _) => None,
        };
        if let (Some(lead), Some(doc)) = (lead, self.docs.get_mut(self.active)) {
            doc.sync.follow(lead);
            self.carry = 12;
        }
        self.mode = mode;
        self.mode_switches += 1;
        self.reader.panel_open = false;
        self.reader.wake(cx);
        self.reshape_window(window, cx);
        self.focus_document(window, cx);
        cx.notify();
    }

    fn toggle_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let other = match self.mode {
            Mode::Read => Mode::Edit,
            Mode::Edit => Mode::Read,
        };
        self.set_mode(other, window, cx);
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

    /// One step of a `ZINMD_SCRIPT`, standing in for the user in scripted checks:
    /// `editor:<px>` and `preview:<px>` scroll a pane, `type:<text>` types at the
    /// caret, `line:<n>` moves the caret, `mode:read|edit`, `view:editor|split|preview`,
    /// `sidebar`, `outline`, `sync`, `new`, `open:<path>`, `save` and `close` run those commands,
    /// `link:<url>` follows a link, `check:<n>` clicks the n-th task checkbox,
    /// `file:<name>` opens the file of that name from the side panel, `fold:<name>`
    /// opens or shuts a folder, `pin:<name>` pins one, `filter:<text>` types in the
    /// panel's search, `panelscroll:<px>` scrolls its list, `rail:<n>` points at the n-th mark of the page's rail,
    /// `section:<n>` goes to the n-th heading, `palette` and `theme` toggle those,
    /// `font:<name>` and `size:<n>` set the editor font.
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
            "line" => self.go_to_line(argument.parse().unwrap_or(0), window, cx),
            // `go:back` and `go:forward` move along the trail of where the
            // reader has been; `turn:next` and `turn:previous` open the file
            // beside this one.
            "go" => self.go_along(argument == "forward", window, cx),
            "turn" => self.turn_to_file(argument != "previous", window, cx),
            "mode" => self.set_mode(
                match argument {
                    "edit" => Mode::Edit,
                    _ => Mode::Read,
                },
                window,
                cx,
            ),
            "view" => self.set_view(
                match argument {
                    "editor" => ViewMode::Editor,
                    "preview" => ViewMode::Preview,
                    _ => ViewMode::Split,
                },
                cx,
            ),
            "sidebar" => self.toggle_sidebar(window, cx),
            "outline" => self.outline_open = !self.outline_open,
            "wait" => {}
            "sync" => self.toggle_scroll_sync(cx),
            "new" => self.new_document(window, cx),
            "open" => self.open(PathBuf::from(argument), window, cx),
            "save" => self.save_doc(id, false, window, cx).detach(),
            "close" => self.request_close(self.active, window, cx),
            "back" => self.last_file(window, cx),
            "palette" => match self.palette {
                Some(_) => self.close_palette(window, cx),
                None => self.open_palette(window, cx),
            },
            "find" => window.dispatch_action(Box::new(edit::Search), cx),
            // `update:ready`, `update:out`, `update:done` and the rest put the
            // card that tells of a new version up, as if one had been found.
            "update" => self.debug_update(argument, cx),
            // What restarting into a new version does before it leaves.
            "backup" => self.back_up_now(),
            // `look:diagram` opens a diagram large over the page, `look:<file>`
            // a picture, as a click on either does.
            "look" => self.debug_look(argument, window, cx),
            // `seek:<words>` looks for words on the page, as typing them
            // into the field for finding does.
            "seek" => self.debug_find(argument, window, cx),
            // File actions on a row of the side panel: `context:<name>` picks the
            // row as a right-click would, then `newfile`, `rename`, `trash` act on
            // it; `name:<text>` types a name, and `key:enter` confirms it.
            "context" => self.context_row = self.debug_path(argument),
            "newfile" => self.new_file_here(window, cx),
            "rename" => self.rename_here(window, cx),
            "trash" => self.trash_here(window, cx),
            "name" => self.debug_name(argument, window, cx),
            "search" => self.open_search(window, cx),
            // Type into the palette, whichever kind is open.
            "query" => match (self.searching(), &self.palette) {
                (true, _) => self.type_in_search(argument, window, cx),
                (false, Some(palette)) => palette.type_query(argument, window, cx),
                (false, None) => {}
            },
            // Choose a row of the palette: `pick:<group>.<row>`.
            // Or, in the window that searches every file, the n-th place
            // that was found: `pick:<n>`.
            "pick" if self.searching() => {
                self.debug_pick_found(argument.parse().unwrap_or(0), window, cx)
            }
            "pick" => {
                let (group, row) = argument.split_once('.').unwrap_or((argument, "0"));
                let at = gpui_kit::component::IndexPath::new(row.parse().unwrap_or(0))
                    .section(group.parse().unwrap_or(0));
                self.run_palette_target(at, window, cx);
            }
            "file" => {
                if let Some(path) = self.debug_path(argument) {
                    self.open(path, window, cx);
                }
            }
            "fold" => self.debug_fold(argument, cx),
            "pin" => {
                if let Some(root) = self.debug_folder(argument) {
                    self.toggle_pin(&root, cx);
                }
            }
            "unlist" => {
                if let Some(root) = self.debug_folder(argument) {
                    self.remove_folder(&root, cx);
                }
            }
            "filter" => self.debug_filter(argument, window, cx),
            "panelscroll" => self.debug_scroll(argument.parse().unwrap_or(0.)),
            "hover" => self.debug_hover(argument, cx),
            "peek" => {
                let path = self.debug_path(argument);
                self.peek_at(path, cx);
            }
            "rail" => self.reader.point_at(argument.parse().ok()),
            "section" => self.go_to_section(argument.parse().unwrap_or(0), window, cx),
            "menu" => self.reader.wake(cx),
            "theme" => theme::toggle(cx),
            // `accent:Sky` picks a colour, `colours` lets the menu of colours down or up.
            "accent" => {
                let accent = theme::Accent::ALL
                    .into_iter()
                    .find(|accent| accent.label() == argument);
                self.update_settings(|s| s.accent = accent.unwrap_or(s.accent), cx);
            }
            "colours" => self.colours_open = !self.colours_open,
            // Put the page in black and white, or back.
            "contrast" => self.update_settings(|s| s.contrast = !s.contrast, cx),
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
                let page = crate::export::html(&doc.text, &doc.name, theme::accent(cx));
                if let Err(error) = std::fs::write(argument, page) {
                    eprintln!("export failed: {error}");
                }
            }
            "link" => self.follow_link(argument, false, window, cx),
            // `beside:<path>` opens a file as a paper of its own; with no
            // path, an empty one.
            // Bring up the switcher between the papers.
            "switch" => self.switch_papers(window, cx),
            // Lay the papers side by side, or put them back.
            "row" => cx.defer(papers::lay_side_by_side),
            "beside" => {
                let path = (!argument.is_empty()).then(|| PathBuf::from(argument));
                self.open_beside(path, window, cx)
            }
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

    /// The file or folder a script step names: by the name the panel shows, or by its path.
    #[cfg(feature = "shot")]
    fn debug_path(&self, name: &str) -> Option<PathBuf> {
        if name.contains('/') {
            return Some(PathBuf::from(name));
        }
        let listed = self.folders.iter().flat_map(|folder| folder.files.iter());
        let open = self.docs.iter().filter_map(|doc| doc.path.as_ref());
        listed
            .chain(open)
            .find(|path| files::display_name(path) == name)
            .cloned()
    }

    #[cfg(feature = "shot")]
    fn debug_folder(&self, name: &str) -> Option<PathBuf> {
        self.folders
            .iter()
            .find(|folder| folder.name.as_ref() == name)
            .map(|folder| folder.root.clone())
    }

    /// Open or shut the folder, or subfolder, of this name.
    #[cfg(feature = "shot")]
    fn debug_fold(&mut self, name: &str, cx: &mut Context<Self>) {
        if let Some(root) = self.debug_folder(name) {
            return self.toggle_folder(&root, cx);
        }
        let dir = self.folders.iter().enumerate().find_map(|(ix, folder)| {
            let row = folder
                .rows
                .iter()
                .find(|row| row.is_dir && row.name.as_ref() == name)?;
            Some((ix, row.path.clone()))
        });
        if let Some((ix, dir)) = dir {
            self.toggle_dir(ix, &dir, cx);
        }
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
                // What was open and unsaved last time comes back in the
                // first paper, which is also the one that looks for updates.
                if this.primary {
                    this.restore_files(window, cx);
                    this.restore_backups(window, cx);
                    this.start_updates(window, cx);
                }
                this.start_watching(window, cx);
                this.shape_window(window);
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
        self.trail.settle();
        self.arrive(window, cx);
        let p = *cx.palette();

        let body = match self.mode {
            Mode::Read => self.render_reader(window, cx),
            Mode::Edit => self.render_desk(window, cx),
        };
        // A change of mode fades up, while the window takes its new shape.
        let body = match self.mode_switches {
            0 => body,
            _ if cx.reduce_motion() => body,
            switches => div()
                .size_full()
                .child(body)
                .with_animation(
                    ("mode-arrives", switches),
                    Animation::new(motion::PANEL).with_easing(motion::ease),
                    |body, progress| body.opacity(0.2 + 0.8 * progress),
                )
                .into_any_element(),
        };
        let palette = self.render_palette(window, cx);
        let searching = self.render_search_window(window, cx);
        let peek = self.render_peek(window, cx);
        let update = self.render_update(window, cx);
        let viewer = self.render_viewer(window, cx);
        let find = self.render_find(window, cx);
        // Pictures named relative to the document are looked for beside it.
        let beside = self
            .active_doc()
            .and_then(|doc| doc.path.as_deref()?.parent().map(Path::to_path_buf));
        crate::preview::Shown::set(window, beside, cx);

        v_flex()
            .id("workspace")
            .relative()
            .size_full()
            .bg(p.surface_app)
            .text_color(p.text_primary)
            .text_size(size::FONT_UI)
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &NewFile, window, cx| this.new_document(window, cx)))
            .on_action(
                cx.listener(|this, _: &NewWindow, window, cx| this.open_beside(None, window, cx)),
            )
            .on_action(cx.listener(|_, _: &SideBySide, _, cx| {
                // Once this paper has finished what it is doing.
                cx.defer(papers::lay_side_by_side)
            }))
            // The switcher is held up by Control: letting go chooses.
            .on_modifiers_changed(cx.listener(|this, event: &ModifiersChangedEvent, _, cx| {
                this.switcher_let_go(event.modifiers, cx)
            }))
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
            .on_action(cx.listener(|this, _: &CloseFile, window, cx| {
                this.request_close(this.active, window, cx)
            }))
            // With more than one paper open, the switcher; with one, back
            // to the file read before.
            .on_action(cx.listener(|this, _: &LastFile, window, cx| this.switch_papers(window, cx)))
            .on_action(cx.listener(|this, _: &GoBack, window, cx| this.go_along(false, window, cx)))
            .on_action(
                cx.listener(|this, _: &GoForward, window, cx| this.go_along(true, window, cx)),
            )
            .on_action(cx.listener(|this, _: &PreviousFile, window, cx| {
                this.turn_to_file(false, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &NextFile, window, cx| this.turn_to_file(true, window, cx)),
            )
            // The buttons at the side of a mouse.
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Back),
                cx.listener(|this, _, window, cx| this.go_along(false, window, cx)),
            )
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Forward),
                cx.listener(|this, _, window, cx| this.go_along(true, window, cx)),
            )
            .on_action(cx.listener(|this, _: &ToggleMode, window, cx| this.toggle_mode(window, cx)))
            .on_action(
                cx.listener(|this, _: &ViewEditor, _, cx| this.set_view(ViewMode::Editor, cx)),
            )
            .on_action(cx.listener(|this, _: &ViewSplit, _, cx| this.set_view(ViewMode::Split, cx)))
            .on_action(
                cx.listener(|this, _: &ViewPreview, _, cx| this.set_view(ViewMode::Preview, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ToggleSidebar, window, cx| this.toggle_sidebar(window, cx)),
            )
            .on_action(cx.listener(|this, _: &ToggleScrollSync, _, cx| this.toggle_scroll_sync(cx)))
            .on_action(cx.listener(|_, _: &ToggleTheme, _, cx| theme::toggle(cx)))
            .on_action(
                cx.listener(|this, _: &FocusSearch, window, cx| this.focus_search(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &OpenPalette, window, cx| match this.palette {
                    Some(_) => this.close_palette(window, cx),
                    None => this.open_palette(window, cx),
                }),
            )
            .on_action(cx.listener(|this, _: &NewFileHere, window, cx| {
                this.context_row = None;
                this.new_file_here(window, cx)
            }))
            .on_action(cx.listener(|this, _: &RenameFile, window, cx| {
                this.context_row = None;
                this.rename_here(window, cx)
            }))
            .on_action(cx.listener(|this, _: &TrashFile, window, cx| this.trash_here(window, cx)))
            .on_action(cx.listener(|this, _: &RevealFile, _, cx| {
                this.context_row = None;
                this.reveal_here(cx)
            }))
            .on_action(cx.listener(|this, _: &CopyFilePath, _, cx| {
                this.context_row = None;
                this.copy_path_here(cx)
            }))
            .on_action(cx.listener(|this, _: &CopyFileContent, _, cx| {
                this.context_row = None;
                this.copy_content_here(cx)
            }))
            .on_action(cx.listener(|this, _: &PreviousSection, window, cx| {
                this.step_section(-1, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &NextSection, window, cx| this.step_section(1, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &FindInFolder, window, cx| this.open_search(window, cx)),
            )
            .on_action(cx.listener(|this, _: &ExportHtml, window, cx| this.export_html(window, cx)))
            // Bigger and smaller act on what is being looked at: the words
            // of the page while reading, the editor's text while editing.
            .on_action(cx.listener(|this, _: &FontBigger, _, cx| {
                let reading = this.mode == Mode::Read;
                this.update_settings(
                    |s| match reading {
                        true => s.reading_scale += settings::READING_STEP,
                        false => s.font_size += 1.,
                    },
                    cx,
                )
            }))
            .on_action(cx.listener(|this, _: &FontSmaller, _, cx| {
                let reading = this.mode == Mode::Read;
                this.update_settings(
                    |s| match reading {
                        true => s.reading_scale -= settings::READING_STEP,
                        false => s.font_size -= 1.,
                    },
                    cx,
                )
            }))
            .on_action(cx.listener(|this, _: &FontReset, _, cx| {
                let reading = this.mode == Mode::Read;
                this.update_settings(
                    |s| match reading {
                        true => s.reading_scale = 1.,
                        false => s.font_size = settings::DEFAULT_FONT_SIZE,
                    },
                    cx,
                )
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
            .on_action(cx.listener(|this, _: &CheckForUpdates, window, cx| {
                this.look_for_update(true, window, cx)
            }))
            .on_action(cx.listener(|this, _: &LookCloser, _, cx| this.look_closer(cx)))
            .on_action(cx.listener(|this, _: &ToggleContrast, _, cx| {
                this.update_settings(|settings| settings.contrast = !settings.contrast, cx)
            }))
            .on_action(
                cx.listener(|this, _: &FindOnPage, window, cx| this.find_on_page(window, cx)),
            )
            .on_action(cx.listener(|this, _: &FindNext, _, cx| this.find_step(1, cx)))
            .on_action(cx.listener(|this, _: &FindBefore, _, cx| this.find_step(-1, cx)))
            .on_action(cx.listener(|this, _: &ToggleUpdates, _, cx| {
                this.update_settings(|settings| settings.updates = !settings.updates, cx)
            }))
            // Escape shuts the menu of colours before it means anything else:
            // as the editor's own action when the editor has the keyboard, as
            // a plain key otherwise.
            .capture_action(cx.listener(|this, _: &edit::Escape, _, cx| {
                if this.colours_open {
                    this.colours_open = false;
                    cx.notify();
                    cx.stop_propagation();
                }
            }))
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                // The switcher between the papers has the keyboard, even
                // when the system left it with this paper.
                if this.switcher_key(&event.keystroke, cx) {
                    return cx.stop_propagation();
                }
                // A picture open over the page has the keyboard to itself.
                if this.viewer_key(&event.keystroke, window, cx) {
                    return cx.stop_propagation();
                }
                // And so does the window that searches every file.
                if this.search_key(&event.keystroke, window, cx) {
                    return cx.stop_propagation();
                }
                // Escape puts the field for finding words away.
                if this.finding() && event.keystroke.key == "escape" {
                    this.close_find(window, cx);
                    return cx.stop_propagation();
                }
                if this.colours_open && event.keystroke.key == "escape" {
                    this.colours_open = false;
                    cx.notify();
                    cx.stop_propagation();
                }
            }))
            // The command key with 1 to 9 shows the files open on their own,
            // the way it showed tabs.
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let key = &event.keystroke;
                let only_cmd = system::only_command(&key.modifiers);
                let digit = key
                    .key
                    .parse::<usize>()
                    .ok()
                    .filter(|n| (1..=9).contains(n));
                if let (true, Some(digit)) = (only_cmd, digit) {
                    this.jump_to_single(digit - 1, window, cx);
                    return cx.stop_propagation();
                }
                // A page is read with the keyboard as much as with the wheel.
                let reading = this.mode == Mode::Read && this.focus.is_focused(window);
                let plain = !(key.modifiers.platform || key.modifiers.alt || key.modifiers.control);
                if !(reading && plain) {
                    return;
                }
                let screen = window.viewport_size().height * 0.85;
                let step = px(72.);
                match (key.key.as_str(), key.modifiers.shift) {
                    ("down", _) | ("j", false) => this.scroll_page(step, cx),
                    ("up", _) | ("k", false) => this.scroll_page(-step, cx),
                    ("space", false) | ("pagedown", _) => this.scroll_page(screen, cx),
                    ("space", true) | ("pageup", _) => this.scroll_page(-screen, cx),
                    ("home", _) => this.move_page_to(
                        ListOffset {
                            item_ix: 0,
                            offset_in_item: px(0.),
                        },
                        cx,
                    ),
                    _ => return,
                }
                cx.stop_propagation();
            }))
            .on_drop(cx.listener(|this, dropped: &ExternalPaths, window, cx| {
                this.panel.dropping = None;
                this.open_requested(dropped.paths().to_vec(), window, cx);
            }))
            .on_drag_move::<DragSidebar>(cx.listener(
                |this, drag: &DragMoveEvent<DragSidebar>, _, cx| {
                    this.sidebar_width = drag.event.position.x.clamp(px(200.), px(480.));
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
            .child(body)
            .children(peek)
            .children(palette)
            .children(searching)
            .children(find)
            .children(viewer)
            .children(update)
    }
}

impl Workspace {
    /// The editor's window: the bar across the top, the side panel, the
    /// source and the preview, and the status bar.
    fn render_desk(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = *cx.palette();
        // The side panel slides and the panes glide between views; both are just
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
        // The panel is kept drawn even when shut, so opening it has nothing to build.
        let sidebar = self.render_panel(false, window, cx);
        let panes_target = (window.viewport_size().width - sidebar_target, share_target);
        let panes = self.render_panes(panes_width, share, panes_target, cx);
        let statusbar = self.render_statusbar(cx);
        let full_sidebar = self.sidebar_width;
        let sidebar_handle = (self.sidebar_open && sidebar_width == full_sidebar)
            .then(|| motion::resize_handle("sidebar-handle", sidebar_width, DragSidebar, &p));

        v_flex()
            .size_full()
            .child(titlebar)
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    // The panel keeps its full width and slides out to the left
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
            .into_any_element()
    }
}
