//! The side panel: what is open, in one quiet list instead of tabs.
//!
//! Files opened on their own come first, then the folders that were added,
//! each with the Markdown files inside it. At rest it shows names and nothing
//! else; the rest appears when the pointer comes close: arrows beside the
//! folders, and on the row under the pointer the two things most often wanted
//! from a file, its content and its path.
//!
//! Every row is placed by the panel itself, at a height that glides to where
//! the row belongs. That is all the motion there is, and it covers everything
//! that can move a row: a folder opening, a folder pinned or dragged
//! elsewhere, a file arriving. Rows that are new fade in where they stand.

use std::collections::{HashMap, HashSet};
use std::f32::consts::FRAC_PI_2;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use gpui_kit::assets::IconName;
use gpui_kit::base::input::{Escape, MoveDown, MoveUp};
use gpui_kit::base::{Transition, transition};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::hover::Hot;
use super::menu::key_cap;
use super::{CopyFileContent, CopyFilePath, Workspace};
use crate::doc::{self, Stats};
use crate::files;
use crate::motion;
use crate::system::{self, shortcut};
use crate::theme::{ActivePalette as _, Palette, size};
use crate::ui::{icon_button, section_label};

/// Every row is this tall, whatever it holds, so nothing moves under the
/// pointer when a row changes what it shows.
pub(super) const ROW: Pixels = px(32.);
/// Between one row and the next. The same everywhere, so a list of folders
/// that are all shut is as evenly spaced as a list of files.
const ROW_GAP: Pixels = px(2.);
/// The title of a section: "Files", "Folders".
const HEADER: Pixels = px(36.);
/// Air between the two sections.
const SECTION_GAP: Pixels = px(12.);
/// Air after what an open folder lists, to set it apart from the next folder.
const FOLDER_GAP: Pixels = px(10.);
/// Space on each side of the rows.
const GUTTER: Pixels = px(12.);
/// How far in each level starts. Past two levels nothing is indented further.
const INDENT: Pixels = px(14.);
/// The inside edge of a row, where its name starts.
const INSET: Pixels = px(12.);
/// A button on a row, and the spot at its right end the buttons share with
/// the dot that says a file has unsaved changes.
const SPOT: Pixels = px(22.);
const OUTLINE_ROW_HEIGHT: Pixels = px(26.);
/// How long the pointer rests on a file before the panel shows what is in it.
const PEEK_AFTER: Duration = Duration::from_millis(650);
/// How long a button says that it did what it was pressed for.
const DONE_FOR: Duration = Duration::from_millis(1300);
const PEEK_WIDTH: Pixels = px(272.);
/// The most files a search lists.
const HITS: usize = 200;

/// What a button on a row does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    CopyContent,
    CopyPath,
    Close,
    Pin,
    Unlist,
}

/// Something listed in the panel.
#[derive(Clone)]
enum Item {
    Header(&'static str),
    /// A document open on its own, by its place among the open documents.
    Single(usize),
    /// A folder's name.
    Folder(usize),
    /// A row under a folder's name: the folder, and which of its rows.
    Row(usize, usize),
    /// A file found by the search.
    Hit(Hit),
}

#[derive(Clone)]
struct Hit {
    path: PathBuf,
    name: SharedString,
    /// Where in the name what was typed is: the start and the length, in bytes.
    found: (usize, usize),
    /// Where the file is, said briefly.
    place: SharedString,
}

/// An item, and where it goes.
struct Placed {
    item: Item,
    /// Stable for as long as the item is listed, whatever is listed around it.
    key: u64,
    top: Pixels,
    height: Pixels,
    /// How many levels in it starts.
    depth: usize,
}

fn key_of(kind: &str, what: impl Hash) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    kind.hash(&mut hasher);
    what.hash(&mut hasher);
    hasher.finish()
}

/// What the panel knows about a file it was asked to show the inside of.
pub(super) struct Peek {
    key: u64,
    title: SharedString,
    /// The first sentences of the file.
    about: SharedString,
    /// How long it takes to read, and how far it has been read.
    place: SharedString,
    read: Option<f32>,
    edited: SharedString,
}

/// A folder being dragged over the panel from outside the app.
pub(super) struct Dropping {
    path: PathBuf,
    name: SharedString,
    /// How many Markdown files are in it, once counted.
    found: Option<usize>,
}

/// What dragging a folder's name carries, and what is shown under the pointer meanwhile.
#[derive(Clone)]
pub(super) struct DragFolder {
    root: PathBuf,
    name: SharedString,
}

impl Render for DragFolder {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = *cx.palette();
        h_flex()
            .h(ROW)
            .w(px(220.))
            .px(INSET)
            .rounded(size::RADIUS_SM)
            .border_1()
            .border_color(p.border_strong)
            .bg(p.surface_raised)
            .shadow_lg()
            .text_size(size::FONT_UI)
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(p.text_primary)
            .child(div().truncate().child(self.name.clone()))
    }
}

pub(super) struct Panel {
    scroll: ScrollHandle,
    /// How far down it was scrolled last time, until the list is there to scroll.
    restore: Option<f32>,
    focus: FocusHandle,
    search: Entity<InputState>,
    /// What is typed in the search: the panel lists only what matches it.
    query: SharedString,
    /// The row the keyboard is on.
    cursor: Option<u64>,
    /// Whether the pointer is inside the panel: the arrows show while it is.
    pointer_in: bool,
    /// A file or folder to bring into view.
    showing: Option<PathBuf>,
    /// The list scrolling smoothly to bring something into view: from, to, since.
    glide: Option<(Pixels, Pixels, Instant)>,
    /// What was listed on the last frame, to tell which rows are new.
    known: HashSet<u64>,
    born: HashMap<u64, Instant>,
    drawn: bool,
    /// What the pointer was over on the last frame.
    was_hot: Option<Hot>,
    peek: Option<Peek>,
    peeking: bool,
    _peek: Task<()>,
    /// The button that was just pressed, to show that it worked.
    done: Option<(u64, Act)>,
    _done: Task<()>,
    /// How tall the whole list is.
    extent: Pixels,
    /// Where the tab that marks the open file last was: its top, and its left edge.
    tab_at: (Pixels, Pixels),
    /// Where the line to the open file last ran from, and the subfolder it stepped in at.
    way_at: (Pixels, Option<Pixels>),
    /// Where each folder starts, in the order shown, to tell where a dragged one would land.
    blocks: Vec<(Pixels, PathBuf)>,
    /// The folder being dragged, and the place among the others it would land in.
    dragging: Option<PathBuf>,
    landing: Option<usize>,
    pub(super) dropping: Option<Dropping>,
    _count: Task<()>,
    _events: Subscription,
}

impl Panel {
    pub(super) fn new(scrolled: f32, window: &mut Window, cx: &mut Context<Workspace>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search or jump to"));
        let events = cx.subscribe_in(
            &search,
            window,
            |this, input, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    this.panel.query = input.read(cx).value();
                    // The first match is the one Return opens.
                    this.panel.cursor = None;
                    this.panel.scroll.set_offset(point(px(0.), px(0.)));
                    this.move_cursor(1, cx);
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.open_cursor(window, cx),
                _ => {}
            },
        );
        Self {
            scroll: ScrollHandle::new(),
            restore: (scrolled > 0.).then_some(scrolled),
            focus: cx.focus_handle(),
            search,
            query: SharedString::default(),
            cursor: None,
            pointer_in: false,
            showing: None,
            glide: None,
            known: HashSet::new(),
            born: HashMap::new(),
            drawn: false,
            was_hot: None,
            peek: None,
            peeking: false,
            _peek: Task::ready(()),
            done: None,
            _done: Task::ready(()),
            extent: px(0.),
            tab_at: (px(0.), GUTTER),
            way_at: (px(0.), None),
            blocks: Vec::new(),
            dragging: None,
            landing: None,
            dropping: None,
            _count: Task::ready(()),
            _events: events,
        }
    }

    /// How far down the list is scrolled.
    pub(super) fn scrolled(&self) -> f32 {
        f32::from(-self.scroll.offset().y).max(0.)
    }

    /// Bring the row of this file or folder into view, once it is listed.
    pub(super) fn show(&mut self, path: PathBuf) {
        self.showing = Some(path);
    }

    /// Give the panel the keyboard: the arrows then move through its rows.
    pub(super) fn focus(&mut self, window: &mut Window, cx: &mut App) {
        self.focus.focus(window, cx);
    }

    pub(super) fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Workspace>) {
        self.search.update(cx, |search, cx| {
            search.focus(window, cx);
            search.select_all(window, cx);
        });
    }

    fn search_focused(&self, window: &Window, cx: &App) -> bool {
        self.search.read(cx).focus_handle(cx).is_focused(window)
    }
}

/// How long ago something happened, in words.
fn ago(then: SystemTime) -> String {
    let seconds = SystemTime::now()
        .duration_since(then)
        .map_or(0, |passed| passed.as_secs());
    let count = |n: u64, unit: &str| match n {
        1 => format!("1 {unit} ago"),
        n => format!("{n} {unit}s ago"),
    };
    match seconds {
        0..60 => "just now".to_string(),
        60..3600 => count(seconds / 60, "minute"),
        3600..86_400 => count(seconds / 3600, "hour"),
        86_400..172_800 => "yesterday".to_string(),
        172_800..1_209_600 => count(seconds / 86_400, "day"),
        1_209_600..5_184_000 => count(seconds / 604_800, "week"),
        5_184_000..63_072_000 => count(seconds / 2_592_000, "month"),
        _ => count(seconds / 31_536_000, "year"),
    }
}

impl Workspace {
    // ------------------------------------------------------------- the list

    /// The folders in the order they are shown: the pinned ones first.
    fn folder_order(&self) -> Vec<usize> {
        let (pinned, loose): (Vec<usize>, Vec<usize>) =
            (0..self.folders.len()).partition(|ix| self.folders[*ix].pinned);
        pinned.into_iter().chain(loose).collect()
    }

    /// Everything the panel lists, each thing with where it goes, and how tall it all is.
    fn lay_out(&self) -> (Vec<Placed>, Pixels) {
        let mut placed = Vec::new();
        let mut y = px(0.);
        let mut put = |item: Item, key: u64, height: Pixels, depth: usize, y: &mut Pixels| {
            placed.push(Placed {
                item,
                key,
                top: *y,
                height,
                depth,
            });
            *y += height;
        };

        if !self.panel.query.is_empty() {
            put(
                Item::Header("RESULTS"),
                key_of("header", "results"),
                HEADER,
                0,
                &mut y,
            );
            for hit in self.hits() {
                let key = key_of("hit", &hit.path);
                put(Item::Hit(hit), key, ROW, 0, &mut y);
                y += ROW_GAP;
            }
            return (placed, y);
        }

        // A section with nothing in it is not there, title and all.
        let singles = self.singles();
        if !singles.is_empty() {
            put(
                Item::Header("FILES"),
                key_of("header", "files"),
                HEADER,
                0,
                &mut y,
            );
            for ix in singles {
                let key = key_of("single", self.docs[ix].id);
                put(Item::Single(ix), key, ROW, 0, &mut y);
                y += ROW_GAP;
            }
            y += SECTION_GAP;
        }
        if !self.folders.is_empty() {
            put(
                Item::Header("FOLDERS"),
                key_of("header", "folders"),
                HEADER,
                0,
                &mut y,
            );
            for ix in self.folder_order() {
                let folder = &self.folders[ix];
                put(
                    Item::Folder(ix),
                    key_of("folder", &folder.root),
                    ROW,
                    0,
                    &mut y,
                );
                y += ROW_GAP;
                if !folder.open || folder.rows.is_empty() {
                    continue;
                }
                for (row_ix, row) in folder.rows.iter().enumerate() {
                    let key = key_of("row", &row.path);
                    put(Item::Row(ix, row_ix), key, ROW, row.depth + 1, &mut y);
                    y += ROW_GAP;
                }
                y += FOLDER_GAP;
            }
        }
        (placed, y)
    }

    /// The files whose names hold what is typed in the search: those that
    /// start with it first, then the rest, each by name.
    fn hits(&self) -> Vec<Hit> {
        let wanted = self.panel.query.to_lowercase();
        let mut hits: Vec<Hit> = Vec::new();
        let mut consider = |path: &Path, place: String| {
            let name = files::display_name(path);
            if let Some(at) = name.to_lowercase().find(&wanted) {
                // Lowering the case can change how long a letter is: only
                // mark the name where the two still line up.
                let found = match name.is_char_boundary(at)
                    && name.is_char_boundary((at + wanted.len()).min(name.len()))
                {
                    true => (at, wanted.len().min(name.len() - at)),
                    false => (0, 0),
                };
                hits.push(Hit {
                    path: path.to_path_buf(),
                    name: name.into(),
                    found,
                    place: place.into(),
                });
            }
        };
        for ix in self.singles() {
            if let Some(path) = &self.docs[ix].path {
                let place = path
                    .parent()
                    .and_then(Path::file_name)
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                consider(path, place);
            }
        }
        for folder in &self.folders {
            for file in &folder.files {
                let inside = file
                    .parent()
                    .and_then(|parent| parent.strip_prefix(&folder.root).ok())
                    .map(|way| way.to_string_lossy().replace('/', " / "))
                    .filter(|way| !way.is_empty());
                let place = match inside {
                    Some(way) => format!("{} / {way}", folder.name),
                    None => folder.name.to_string(),
                };
                consider(file, place);
            }
        }
        hits.sort_by_cached_key(|hit| {
            let name = hit.name.to_lowercase();
            (!name.starts_with(&wanted), name)
        });
        hits.truncate(HITS);
        hits
    }

    /// The row of the document on show, if it is listed.
    fn active_key(&self) -> Option<u64> {
        let doc = self.active_doc()?;
        match &doc.path {
            Some(path) if !self.panel.query.is_empty() => Some(key_of("hit", path)),
            Some(path) if self.folder_of(path).is_some() => Some(key_of("row", path)),
            _ => Some(key_of("single", doc.id)),
        }
    }

    // --------------------------------------------------------------- folders

    /// Open or shut a folder of the panel.
    pub(super) fn toggle_folder(&mut self, root: &Path, cx: &mut Context<Self>) {
        if let Some(folder) = self.folders.iter_mut().find(|folder| folder.root == root) {
            folder.open = !folder.open;
        }
        cx.notify();
    }

    /// Open or shut a subfolder.
    pub(super) fn toggle_dir(&mut self, folder: usize, dir: &Path, cx: &mut Context<Self>) {
        if let Some(folder) = self.folders.get_mut(folder) {
            folder.toggle_dir(dir);
        }
        cx.notify();
    }

    /// Put a dragged folder down at place `at` among the others. Where it
    /// lands says whether it is pinned: among the pinned ones it is.
    fn land_folder(&mut self, root: &Path, at: usize, cx: &mut Context<Self>) {
        let order = self.folder_order();
        let Some(from) = order.iter().position(|ix| self.folders[*ix].root == root) else {
            return;
        };
        let pinned_count = order.iter().filter(|ix| self.folders[**ix].pinned).count();
        let was_pinned = self.folders[order[from]].pinned;
        // Dropped on the line between the two groups, it stays in its own.
        let pinned = match at {
            at if at < pinned_count => true,
            at if at > pinned_count => false,
            _ => was_pinned,
        };
        let mut roots: Vec<PathBuf> = order
            .iter()
            .map(|ix| self.folders[*ix].root.clone())
            .collect();
        let moved = roots.remove(from);
        let at = if at > from { at - 1 } else { at }.min(roots.len());
        roots.insert(at, moved.clone());
        let mut folders = std::mem::take(&mut self.folders);
        folders.sort_by_key(|folder| roots.iter().position(|root| *root == folder.root));
        if let Some(folder) = folders.iter_mut().find(|folder| folder.root == moved) {
            folder.pinned = pinned;
        }
        self.folders = folders;
        cx.notify();
    }

    // ---------------------------------------------------------------- acting

    /// Do what a button on a row is for.
    fn act(
        &mut self,
        key: u64,
        act: Act,
        path: Option<PathBuf>,
        doc: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let open = path.as_ref().and_then(|path| {
            self.docs
                .iter()
                .position(|doc| doc.path.as_ref() == Some(path))
        });
        match act {
            Act::CopyContent => {
                // What is in the editor, unsaved edits and all; otherwise what is on disk.
                let text = match doc.or(open) {
                    Some(ix) => Some(self.docs[ix].text.to_string()),
                    None => path
                        .as_ref()
                        .and_then(|path| std::fs::read(path).ok())
                        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned()),
                };
                if let Some(text) = text {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
            }
            Act::CopyPath => {
                if let Some(path) = &path {
                    cx.write_to_clipboard(ClipboardItem::new_string(
                        path.to_string_lossy().into_owned(),
                    ));
                }
            }
            Act::Close => {
                if let Some(ix) = doc.or(open) {
                    self.request_close(ix, window, cx);
                }
                return;
            }
            Act::Pin => {
                if let Some(root) = &path {
                    self.toggle_pin(root, cx);
                }
                return;
            }
            Act::Unlist => {
                if let Some(root) = &path {
                    self.remove_folder(root, cx);
                }
                return;
            }
        }
        // The button says it worked, for a moment.
        self.panel.done = Some((key, act));
        self.panel._done = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(DONE_FOR).await;
            this.update(cx, |this, cx| {
                this.panel.done = None;
                cx.notify();
            })
            .ok();
        });
        cx.notify();
    }

    /// Copy the content of the file a menu was opened on, or of the one on show.
    pub(super) fn copy_content_here(&mut self, cx: &mut Context<Self>) {
        let path = self.context_row.take();
        let text = match &path {
            Some(path) => match self.docs.iter().find(|doc| doc.path.as_ref() == Some(path)) {
                Some(doc) => Some(doc.text.to_string()),
                None => std::fs::read(path)
                    .ok()
                    .map(|bytes| String::from_utf8_lossy(&bytes).into_owned()),
            },
            None => self.active_doc().map(|doc| doc.text.to_string()),
        };
        if let Some(text) = text {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    // -------------------------------------------------------------- keyboard

    /// The rows the keyboard can be on, in order.
    fn stops(&self) -> Vec<Placed> {
        let (placed, _) = self.lay_out();
        placed
            .into_iter()
            .filter(|placed| !matches!(placed.item, Item::Header(_)))
            .collect()
    }

    /// Move the keyboard's row up or down, and keep it in view.
    fn move_cursor(&mut self, step: isize, cx: &mut Context<Self>) {
        let stops = self.stops();
        if stops.is_empty() {
            self.panel.cursor = None;
            return;
        }
        // With no row yet, start from the document on show: the keyboard
        // picks up where the eye is.
        let here = self
            .panel
            .cursor
            .or_else(|| self.active_key())
            .and_then(|key| stops.iter().position(|stop| stop.key == key));
        let next = match here {
            Some(here) if self.panel.cursor.is_some() => {
                (here as isize + step).clamp(0, stops.len() as isize - 1) as usize
            }
            Some(here) => here,
            None if step < 0 => stops.len() - 1,
            None => 0,
        };
        let stop = &stops[next];
        self.panel.cursor = Some(stop.key);
        self.scroll_panel_to(stop.top, stop.height, cx);
        // What was being peeked at is no longer where the keyboard is.
        self.panel.peeking = false;
        cx.notify();
    }

    /// Open, or open and shut, the row the keyboard is on.
    fn open_cursor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(key) = self.panel.cursor else { return };
        let Some(stop) = self.stops().into_iter().find(|stop| stop.key == key) else {
            return;
        };
        match stop.item {
            Item::Single(ix) => self.activate(ix, window, cx),
            Item::Folder(ix) => {
                let root = self.folders[ix].root.clone();
                self.toggle_folder(&root, cx);
            }
            Item::Row(folder, row) => {
                let row = self.folders[folder].rows[row].clone();
                match row.is_dir {
                    true => self.toggle_dir(folder, &row.path, cx),
                    false => self.open(row.path, window, cx),
                }
            }
            Item::Hit(hit) => {
                self.clear_search(window, cx);
                self.open(hit.path, window, cx);
            }
            Item::Header(_) => {}
        }
    }

    /// Open or shut the folder the keyboard is on: right opens, left shuts.
    fn fold_cursor(&mut self, open: bool, cx: &mut Context<Self>) {
        let Some(key) = self.panel.cursor else { return };
        let Some(stop) = self.stops().into_iter().find(|stop| stop.key == key) else {
            return;
        };
        match stop.item {
            Item::Folder(ix) if self.folders[ix].open != open => {
                let root = self.folders[ix].root.clone();
                self.toggle_folder(&root, cx);
            }
            Item::Row(folder, row) => {
                let row = self.folders[folder].rows[row].clone();
                if row.is_dir && row.open != open {
                    self.toggle_dir(folder, &row.path, cx);
                }
            }
            _ => {}
        }
    }

    /// Empty the search, so the panel lists everything again.
    fn clear_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.panel
            .search
            .update(cx, |search, cx| search.set_value("", window, cx));
        self.panel.query = SharedString::default();
        self.panel.cursor = None;
    }

    /// A key was pressed with the keyboard in the panel.
    fn panel_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        if key.modifiers.platform || key.modifiers.control || key.modifiers.alt {
            return;
        }
        let searching = self.panel.search_focused(window, cx);
        match key.key.as_str() {
            "up" => self.move_cursor(-1, cx),
            "down" => self.move_cursor(1, cx),
            "enter" if !searching => self.open_cursor(window, cx),
            "right" if !searching => self.fold_cursor(true, cx),
            "left" if !searching => self.fold_cursor(false, cx),
            "space" if !searching => {
                // A second press takes the card away again.
                let path = self.cursor_file().filter(|_| !self.panel.peeking);
                self.peek_at(path, cx);
            }
            "escape" => self.leave_panel(window, cx),
            // Typing anything else starts a search with it: no need to click the field.
            _ if !searching => {
                let typed = key.key_char.clone().filter(|typed| {
                    typed.chars().all(|c| !c.is_control()) && !typed.trim().is_empty()
                });
                let Some(typed) = typed else { return };
                self.panel.search.update(cx, |search, cx| {
                    search.set_value(typed, window, cx);
                    search.focus(window, cx);
                });
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    /// Escape: out of the search first, then out of the panel.
    fn leave_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.panel.peeking = false;
        if !self.panel.query.is_empty() {
            self.clear_search(window, cx);
            self.panel.focus(window, cx);
        } else {
            self.panel.cursor = None;
            self.reader.panel_open = false;
            self.focus_document(window, cx);
        }
        cx.notify();
    }

    /// The file on the row the keyboard is on.
    fn cursor_file(&self) -> Option<PathBuf> {
        let key = self.panel.cursor?;
        let stop = self.stops().into_iter().find(|stop| stop.key == key)?;
        match stop.item {
            Item::Single(ix) => self.docs[ix].path.clone(),
            Item::Row(folder, row) => {
                let row = &self.folders[folder].rows[row];
                (!row.is_dir).then(|| row.path.clone())
            }
            Item::Hit(hit) => Some(hit.path),
            _ => None,
        }
    }

    // --------------------------------------------------------------- scrolling

    /// Scroll the list, smoothly, just enough to show the row at `top`.
    fn scroll_panel_to(&mut self, top: Pixels, height: Pixels, cx: &mut Context<Self>) {
        let view = self.panel.scroll.bounds().size.height;
        if view <= px(0.) {
            return;
        }
        let scrolled = -self.panel.scroll.offset().y;
        // A row is not brought up against the very edge: one more shows beyond it.
        let margin = ROW + ROW_GAP;
        let to = if top - margin < scrolled {
            (top - margin).max(px(0.))
        } else if top + height + margin > scrolled + view {
            top + height + margin - view
        } else {
            return;
        };
        if cx.reduce_motion() {
            self.panel.scroll.set_offset(point(px(0.), -to));
        } else {
            self.panel.glide = Some((scrolled, to, Instant::now()));
        }
        cx.notify();
    }

    /// Move a scroll in progress one frame on.
    fn advance_panel_scroll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((from, to, started)) = self.panel.glide else {
            return;
        };
        let progress = motion::progress(started, motion::PANEL, cx);
        let y = from + (to - from) * progress;
        self.panel.scroll.set_offset(point(px(0.), -y));
        if progress >= 1. {
            self.panel.glide = None;
        } else {
            window.request_animation_frame();
        }
    }

    // ---------------------------------------------------------------- peeking

    /// Show what is inside `path` in a card beside the panel, or take the card away.
    pub(super) fn peek_at(&mut self, path: Option<PathBuf>, cx: &mut Context<Self>) {
        self.panel._peek = Task::ready(());
        let Some(path) = path else {
            self.panel.peeking = false;
            return cx.notify();
        };
        let key = match self.folder_of(&path) {
            Some(_) if self.panel.query.is_empty() => key_of("row", &path),
            _ if !self.panel.query.is_empty() => key_of("hit", &path),
            _ => self
                .docs
                .iter()
                .find(|doc| doc.path.as_ref() == Some(&path))
                .map_or(0, |doc| key_of("single", doc.id)),
        };
        let read = self.reader.read(&path);
        let open = self
            .docs
            .iter()
            .find(|doc| doc.path.as_ref() == Some(&path))
            .map(|doc| doc.text.clone());
        let describe = move |text: &str, path: PathBuf| {
            let title = doc::outline(text)
                .into_iter()
                .next()
                .map(|heading| heading.title)
                .unwrap_or_else(|| files::display_name(&path).into());
            let minutes = Stats::of(text).minutes();
            let place = match read {
                Some(far) if far >= 0.02 => format!(
                    "{minutes} MIN READ   ·   {}% READ",
                    (far * 100.).round() as usize
                ),
                _ => format!("{minutes} MIN READ"),
            };
            let edited = std::fs::metadata(&path)
                .and_then(|file| file.modified())
                .map_or(String::new(), |time| format!("Edited {}", ago(time)));
            Peek {
                key,
                title,
                about: doc::excerpt(text, 0, None, 170).into(),
                place: place.into(),
                read,
                edited: edited.into(),
            }
        };
        match open {
            Some(text) => {
                self.panel.peek = Some(describe(&text, path));
                self.panel.peeking = true;
                cx.notify();
            }
            // A file that is not open is read off the main thread.
            None => {
                self.panel._peek = cx.spawn(async move |this, cx| {
                    let peek = cx
                        .background_executor()
                        .spawn(async move {
                            let text = std::fs::read(&path)
                                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                                .unwrap_or_default();
                            describe(&text, path)
                        })
                        .await;
                    this.update(cx, |this, cx| {
                        this.panel.peek = Some(peek);
                        this.panel.peeking = true;
                        cx.notify();
                    })
                    .ok();
                });
            }
        }
    }

    /// Type into the panel's search, for scripted runs.
    #[cfg(feature = "shot")]
    pub(super) fn debug_filter(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let typed = text.to_string();
        self.panel
            .search
            .update(cx, |search, cx| search.set_value(typed, window, cx));
        self.panel.query = text.to_string().into();
        self.panel.cursor = None;
        self.move_cursor(1, cx);
    }

    /// Scroll the list this far down, for scripted runs.
    #[cfg(feature = "shot")]
    pub(super) fn debug_scroll(&mut self, down: f32) {
        self.panel.scroll.set_offset(point(px(0.), -px(down)));
    }

    /// Put the pointer on the row of this name, for scripted runs: a file, or a folder.
    #[cfg(feature = "shot")]
    pub(super) fn debug_hover(&mut self, name: &str, cx: &mut Context<Self>) {
        let folder = self
            .folders
            .iter()
            .find(|folder| folder.name.as_ref() == name)
            .map(|folder| Hot::Folder(key_of("folder", &folder.root)));
        let file = self
            .debug_path(name)
            .map(|path| match self.folder_of(&path) {
                Some(_) => Hot::File(key_of("row", &path)),
                None => {
                    let open = self
                        .docs
                        .iter()
                        .find(|doc| doc.path.as_ref() == Some(&path));
                    Hot::File(open.map_or(0, |doc| key_of("single", doc.id)))
                }
            });
        self.hot = folder.or(file);
        self.panel.pointer_in = self.hot.is_some();
        cx.notify();
    }

    /// The pointer came to rest on a file, or left one: peek after a moment, or stop.
    fn follow_pointer(&mut self, placed: &[Placed], cx: &mut Context<Self>) {
        if self.panel.was_hot == self.hot {
            return;
        }
        self.panel.was_hot = self.hot;
        let over = match self.hot {
            Some(Hot::File(key)) => placed.iter().find(|placed| placed.key == key),
            _ => None,
        };
        let path = over.and_then(|placed| match &placed.item {
            Item::Single(ix) => self.docs[*ix].path.clone(),
            Item::Row(folder, row) => Some(self.folders[*folder].rows[*row].path.clone()),
            Item::Hit(hit) => Some(hit.path.clone()),
            _ => None,
        });
        // The card for one file does not linger over another.
        self.panel.peeking = false;
        self.panel._peek = match path {
            Some(path) => cx.spawn(async move |this, cx| {
                cx.background_executor().timer(PEEK_AFTER).await;
                this.update(cx, |this, cx| this.peek_at(Some(path), cx))
                    .ok();
            }),
            None => Task::ready(()),
        };
    }

    /// The card that says what is inside the file being peeked at.
    pub(super) fn render_peek(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let showing = self.panel.peeking && self.panel_shown();
        let presence = transition(
            "peek",
            if showing { 1. } else { 0. },
            Transition::new(motion::QUICK).ease(motion::ease),
            window,
            cx,
        );
        let peek = self.panel.peek.as_ref()?;
        if presence <= 0.001 {
            return None;
        }
        let p = *cx.palette();
        let viewport = window.viewport_size();
        // Beside the panel, level with the row it is about.
        let list = self.panel.scroll.bounds();
        let (placed, _) = self.lay_out();
        let row = placed
            .iter()
            .find(|placed| placed.key == peek.key)
            .map_or(px(0.), |placed| placed.top);
        let y = (list.top() + row + self.panel.scroll.offset().y - px(40.))
            .max(px(12.))
            .min(viewport.height - px(210.));
        // On a window too narrow for the card beside the panel, it sits over the page's edge.
        let x = (list.right() + px(12.)).min(viewport.width - PEEK_WIDTH - px(12.));
        let y = transition("peek-y", y, motion::glide(motion::QUICK, cx), window, cx);

        Some(
            v_flex()
                .absolute()
                .left(x - px(6.) * (1. - presence))
                .top(y)
                .w(PEEK_WIDTH)
                .px(px(16.))
                .pt(px(14.))
                .pb(px(12.))
                .gap(px(6.))
                .rounded(size::RADIUS_LG)
                .border_1()
                .border_color(p.border_strong)
                .bg(p.surface_raised)
                .shadow_lg()
                .opacity(presence)
                .text_size(size::FONT_UI)
                .child(
                    div()
                        .text_size(px(10.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(p.text_tertiary)
                        .child(peek.place.clone()),
                )
                .child(
                    div()
                        .text_size(px(15.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(peek.title.clone()),
                )
                .when(!peek.about.is_empty(), |card| {
                    card.child(
                        div()
                            .text_size(px(12.5))
                            .line_height(px(19.))
                            .text_color(p.text_secondary)
                            .child(peek.about.clone()),
                    )
                })
                // How far it has been read, as a line.
                .children(peek.read.filter(|far| *far >= 0.02).map(|far| {
                    div()
                        .mt(px(2.))
                        .h(px(3.))
                        .w_full()
                        .rounded_full()
                        .bg(p.border_subtle)
                        .child(
                            div()
                                .h_full()
                                .w(relative(far.min(1.)))
                                .rounded_full()
                                .bg(p.accent),
                        )
                }))
                .child(
                    h_flex()
                        .mt(px(2.))
                        .gap(px(6.))
                        .text_size(px(11.))
                        .text_color(p.text_tertiary)
                        .child(div().flex_1().truncate().child(peek.edited.clone()))
                        .child(key_cap("Space", p.text_tertiary, p.surface_sunken))
                        .child("to peek"),
                ),
        )
    }
}

/// What a row says, in a space that may be too short for it. Rather than being
/// cut off, or shortened with dots, it fades out where its room ends, into
/// `backdrop`: the colour the row is drawn on.
pub(super) fn fading(words: impl IntoElement, backdrop: Hsla) -> Div {
    div()
        .relative()
        .flex()
        .flex_1()
        .items_center()
        .min_w_0()
        .h_full()
        .overflow_hidden()
        .whitespace_nowrap()
        .child(words)
        .child(
            // Kept clear of the row's top and bottom edges, where the open
            // file's tab has its outline.
            div()
                .absolute()
                .top(px(3.))
                .bottom(px(3.))
                .right_0()
                .w(px(26.))
                .bg(linear_gradient(
                    90.,
                    linear_color_stop(backdrop.opacity(0.), 0.),
                    linear_color_stop(backdrop, 1.),
                )),
        )
}

/// What a row for a file needs to know.
struct FileRow {
    key: u64,
    path: Option<PathBuf>,
    /// The open document it stands for, if it is one of those open on their own.
    doc: Option<usize>,
    name: SharedString,
    /// The way down to a file deeper than the panel indents, said after its name.
    way: SharedString,
    /// Where the file is, said at the right end of its row.
    place: Option<SharedString>,
    /// The part of the name that matches the search: its start and length.
    found: (usize, usize),
    /// Whether it is the document on show.
    lit: bool,
    single: bool,
}

impl Workspace {
    // ---------------------------------------------------------------- dragging

    /// A folder of the panel is being dragged: work out where it would land.
    fn drag_folder_over(
        &mut self,
        root: PathBuf,
        position: Point<Pixels>,
        inside: bool,
        cx: &mut Context<Self>,
    ) {
        let list = self.panel.scroll.bounds();
        let y = position.y - list.top() - self.panel.scroll.offset().y;
        // Before the first folder whose middle is below the pointer.
        let blocks = &self.panel.blocks;
        let landing = inside.then(|| {
            (0..blocks.len())
                .find(|ix| {
                    let bottom = blocks.get(ix + 1).map_or(self.panel.extent, |next| next.0);
                    y < (blocks[*ix].0 + bottom) / 2.
                })
                .unwrap_or(blocks.len())
        });
        if self.panel.landing != landing || self.panel.dragging.as_ref() != Some(&root) {
            self.panel.landing = landing;
            self.panel.dragging = Some(root);
            cx.notify();
        }
    }

    /// Something is being dragged over the panel from outside the app. A
    /// folder is counted, so the panel can say what dropping it will add.
    fn drag_paths_over(&mut self, paths: &[PathBuf], inside: bool, cx: &mut Context<Self>) {
        let over = paths.first().filter(|_| inside);
        if over == self.panel.dropping.as_ref().map(|dropping| &dropping.path) {
            return;
        }
        self.panel.dropping = None;
        self.panel._count = Task::ready(());
        if let Some(path) = over.filter(|path| path.is_dir()).cloned() {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            self.panel.dropping = Some(Dropping {
                path: path.clone(),
                name: name.into(),
                found: None,
            });
            self.panel._count = cx.spawn(async move |this, cx| {
                let counted = path.clone();
                let found = cx
                    .background_executor()
                    .spawn(async move { files::scan(&counted, 5000).len() })
                    .await;
                this.update(cx, |this, cx| {
                    if let Some(dropping) = &mut this.panel.dropping
                        && dropping.path == path
                    {
                        dropping.found = Some(found);
                        cx.notify();
                    }
                })
                .ok();
            });
        }
        cx.notify();
    }

    // ----------------------------------------------------------------- drawing

    /// The side panel. `floating` is the one that slides over a page: it
    /// starts below the window's buttons and has no outline, which the page's
    /// rail is there for.
    pub(super) fn render_panel(
        &mut self,
        floating: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = *cx.palette();
        self.advance_panel_scroll(window, cx);
        if !cx.has_active_drag() {
            self.panel.dragging = None;
            self.panel.landing = None;
            self.panel.dropping = None;
        }
        let (placed, extent) = self.lay_out();
        self.panel.extent = extent;
        self.follow_pointer(&placed, cx);

        // Rows that were not listed on the last frame are new: they fade in.
        let keys: HashSet<u64> = placed.iter().map(|placed| placed.key).collect();
        if self.panel.drawn {
            let now = Instant::now();
            for key in keys.difference(&self.panel.known) {
                self.panel.born.insert(*key, now);
            }
        }
        self.panel
            .born
            .retain(|key, born| keys.contains(key) && born.elapsed() < motion::PANEL);
        self.panel.known = keys;
        self.panel.drawn = true;
        self.panel.blocks = placed
            .iter()
            .filter_map(|placed| match placed.item {
                Item::Folder(ix) => Some((placed.top, self.folders[ix].root.clone())),
                _ => None,
            })
            .collect();

        // Once the list is there to scroll: back to where it was left, and to
        // the row that was asked to be shown.
        let view = self.panel.scroll.bounds().size.height;
        if view > px(0.) {
            if let Some(scrolled) = self.panel.restore.take() {
                let furthest = (extent - view).max(px(0.));
                self.panel
                    .scroll
                    .set_offset(point(px(0.), -px(scrolled).min(furthest)));
            }
            if let Some(path) = self.panel.showing.take() {
                let keys = [
                    key_of("row", &path),
                    key_of("folder", &path),
                    key_of("hit", &path),
                ];
                let found = placed.iter().find(|placed| match placed.item {
                    Item::Single(ix) => self.docs[ix].path.as_ref() == Some(&path),
                    _ => keys.contains(&placed.key),
                });
                match found.map(|found| (found.top, found.height)) {
                    Some((top, height)) => self.scroll_panel_to(top, height, cx),
                    // Its folder is still being read: wait for the row to be there.
                    None => {
                        let pending = self
                            .folder_of(&path)
                            .is_some_and(|ix| self.folders[ix].files.is_empty());
                        self.panel.showing = pending.then_some(path);
                    }
                }
            }
        }

        let searching = !self.panel.query.is_empty();
        let nothing = placed.is_empty() && !searching;
        let body = match nothing {
            true => self.render_nothing_open(&p, cx),
            false => self.render_list(&placed, extent, &p, window, cx),
        };
        let outline = (!floating && !searching).then(|| self.render_outline(&p, window, cx));
        let hints = searching.then(|| {
            h_flex()
                .flex_none()
                .justify_center()
                .gap(px(14.))
                .h(px(30.))
                .text_size(px(11.5))
                .text_color(p.text_tertiary)
                .child("↑ ↓  move")
                .child("⏎  open")
                .child("esc  clear")
        });
        let dropping = self.render_dropping(&p);

        v_flex()
            .id("panel")
            .relative()
            .size_full()
            .track_focus(&self.panel.focus)
            // The window's buttons are over the top of a floating panel.
            .when(floating, |panel| panel.pt(px(34.)))
            .on_hover(cx.listener(|this, over: &bool, _, cx| {
                this.panel.pointer_in = *over;
                cx.notify();
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                this.panel_key(event, window, cx)
            }))
            // In the search field the arrows belong to the list, not to the caret.
            .capture_action(cx.listener(|this, _: &MoveUp, window, cx| {
                if this.panel.search_focused(window, cx) {
                    this.move_cursor(-1, cx);
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &MoveDown, window, cx| {
                if this.panel.search_focused(window, cx) {
                    this.move_cursor(1, cx);
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &Escape, window, cx| {
                if this.panel.search_focused(window, cx) {
                    this.leave_panel(window, cx);
                    cx.stop_propagation();
                }
            }))
            .on_drag_move::<DragFolder>(cx.listener(
                |this, drag: &DragMoveEvent<DragFolder>, _, cx| {
                    let root = drag.drag(cx).root.clone();
                    let inside = drag.bounds.contains(&drag.event.position);
                    this.drag_folder_over(root, drag.event.position, inside, cx);
                },
            ))
            .on_drop(cx.listener(|this, drag: &DragFolder, _, cx| {
                if let Some(at) = this.panel.landing.take() {
                    this.land_folder(&drag.root, at, cx);
                }
                this.panel.dragging = None;
                cx.notify();
            }))
            .on_drag_move::<ExternalPaths>(cx.listener(
                |this, drag: &DragMoveEvent<ExternalPaths>, _, cx| {
                    let inside = drag.bounds.contains(&drag.event.position);
                    let paths = drag.drag(cx).paths().to_vec();
                    this.drag_paths_over(&paths, inside, cx);
                },
            ))
            .child(self.render_search(&p, window, cx))
            .child(body)
            .children(hints)
            .children(outline)
            .children(dropping)
            .into_any_element()
    }

    /// The search field at the top of the panel. What is typed in it is what the panel lists.
    fn render_search(
        &self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let focused = self.panel.search_focused(window, cx);
        let searching = !self.panel.query.is_empty();
        let field = Input::new(&self.panel.search)
            .appearance(false)
            .bordered(false)
            .small()
            .px_0()
            .text_size(size::FONT_UI);
        h_flex()
            .flex_none()
            .h(px(32.))
            .mx(GUTTER)
            .mt(px(12.))
            .pl(px(9.))
            .pr(px(6.))
            .gap(px(7.))
            .rounded(px(7.))
            .border_1()
            .border_color(if focused { p.accent } else { p.border_subtle })
            .bg(p.surface_raised)
            .child(
                Icon::new(IconName::Search)
                    .size(px(14.))
                    .text_color(p.text_tertiary),
            )
            .child(div().flex_1().min_w_0().child(field))
            .child(key_cap(
                if searching { "esc" } else { shortcut!("K") },
                p.text_tertiary,
                p.surface_sunken,
            ))
    }

    fn render_list(
        &mut self,
        placed: &[Placed],
        extent: Pixels,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let scrolled = -self.panel.scroll.offset().y;
        // Only rows in view are built. Before the first layout the size of the
        // view is unknown; a screenful is a safe guess.
        let view = match self.panel.scroll.bounds().size.height {
            height if height > px(0.) => height,
            _ => px(1200.),
        };
        // The arrows beside the folders are there while the pointer is in the panel.
        let arrows = transition(
            "panel-arrows",
            if self.panel.pointer_in { 1. } else { 0. },
            Transition::new(motion::QUICK).ease(motion::ease),
            window,
            cx,
        );
        let active = self.active_key();
        let shown_path = self.active_doc().and_then(|doc| doc.path.clone());

        let mut rows = Vec::new();
        let mut fading = false;
        for placed in placed {
            let near = placed.top + placed.height > scrolled - px(160.)
                && placed.top < scrolled + view + px(160.);
            if !near {
                continue;
            }
            // Each row glides to where it belongs.
            let y = transition(
                ElementId::from(("panel-y", placed.key as usize)),
                placed.top,
                motion::glide(motion::QUICK, cx),
                window,
                cx,
            );
            let arrived = self.panel.born.get(&placed.key).map_or(1., |born| {
                fading = true;
                motion::progress(*born, motion::PANEL, cx)
            });
            let lit = active == Some(placed.key);
            let (row, of_folder) = match &placed.item {
                Item::Header(label) => (self.render_header(label, p, window, cx), None),
                Item::Single(ix) => {
                    let doc = &self.docs[*ix];
                    // Where it is, to tell two files of the same name apart.
                    let place = doc
                        .path
                        .as_deref()
                        .and_then(Path::parent)
                        .and_then(Path::file_name)
                        .map(|name| SharedString::from(name.to_string_lossy().into_owned()));
                    let name = match &doc.path {
                        Some(path) => files::display_name(path),
                        None => "Untitled".to_string(),
                    };
                    let row = FileRow {
                        key: placed.key,
                        path: doc.path.clone(),
                        doc: Some(*ix),
                        name: name.into(),
                        way: SharedString::default(),
                        place,
                        found: (0, 0),
                        lit,
                        single: true,
                    };
                    (self.render_file(row, p, window, cx), None)
                }
                Item::Folder(ix) => (
                    self.render_folder(*ix, placed.key, arrows, p, window, cx),
                    Some(*ix),
                ),
                Item::Row(folder, row) => {
                    let listed = &self.folders[*folder].rows[*row];
                    let drawn = match listed.is_dir {
                        true => self.render_dir(*folder, *row, placed.key, arrows, p, window, cx),
                        false => {
                            let row = FileRow {
                                key: placed.key,
                                path: Some(listed.path.clone()),
                                doc: None,
                                name: listed.name.clone(),
                                way: listed.way.clone(),
                                place: None,
                                found: (0, 0),
                                lit,
                                single: false,
                            };
                            self.render_file(row, p, window, cx)
                        }
                    };
                    (drawn, Some(*folder))
                }
                Item::Hit(hit) => {
                    let row = FileRow {
                        key: placed.key,
                        path: Some(hit.path.clone()),
                        doc: None,
                        name: hit.name.clone(),
                        way: SharedString::default(),
                        place: Some(hit.place.clone()),
                        found: hit.found,
                        lit: shown_path.as_ref() == Some(&hit.path),
                        single: false,
                    };
                    (self.render_file(row, p, window, cx), None)
                }
            };
            // A folder being dragged steps back, with everything in it.
            let lifted = of_folder
                .is_some_and(|ix| self.panel.dragging.as_ref() == Some(&self.folders[ix].root));
            rows.push(
                div()
                    .absolute()
                    .top(y)
                    .left(GUTTER + INDENT * placed.depth as f32)
                    .right(GUTTER)
                    .h(placed.height)
                    .opacity(arrived * if lifted { 0.35 } else { 1. })
                    .child(row),
            );
        }
        if fading {
            window.request_animation_frame();
        }

        // The open file is marked by one white tab that glides from row to row.
        let lit = active.and_then(|key| placed.iter().find(|placed| placed.key == key));
        if let Some(lit) = lit {
            self.panel.tab_at = (lit.top, GUTTER + INDENT * lit.depth as f32);
        }
        // While something is dragged, things keep up with the pointer rather than glide.
        let pace = match cx.has_active_drag() {
            true => Duration::ZERO,
            false => motion::QUICK,
        };
        let quick = || Transition::new(pace).ease(motion::ease);
        let tab_shown = transition(
            "panel-tab",
            if lit.is_some() { 1. } else { 0. },
            quick(),
            window,
            cx,
        );
        let tab_y = transition("panel-tab-y", self.panel.tab_at.0, quick(), window, cx);
        let tab_x = transition("panel-tab-x", self.panel.tab_at.1, quick(), window, cx);
        let tab = (tab_shown > 0.001).then(|| {
            div()
                .absolute()
                .top(tab_y)
                .left(tab_x)
                .right(GUTTER)
                .h(ROW)
                .rounded(px(7.))
                .border_1()
                .border_color(p.border_subtle)
                .bg(p.surface_raised)
                .shadow_sm()
                .opacity(tab_shown)
        });

        // One line leads to it: out of its folder's name, in at each subfolder
        // on the way, and into the tab.
        let way = lit.and_then(|lit| {
            let Item::Row(folder, row) = lit.item else {
                return None;
            };
            let name = placed
                .iter()
                .find(|placed| matches!(placed.item, Item::Folder(ix) if ix == folder))?;
            let through = (lit.depth > 1)
                .then(|| {
                    placed.iter().rfind(|placed| {
                        matches!(placed.item, Item::Row(of, at)
                            if of == folder && at < row && self.folders[of].rows[at].is_dir)
                    })
                })
                .flatten();
            Some((name.top, through.map(|through| through.top)))
        });
        if let Some(way) = way {
            self.panel.way_at = way;
        }
        let way_shown = transition(
            "panel-way",
            if way.is_some() { 1. } else { 0. },
            quick(),
            window,
            cx,
        );
        let (from, through) = self.panel.way_at;
        let from = transition("panel-way-from", from + ROW / 2., quick(), window, cx);
        let through = through.map(|through| {
            transition("panel-way-through", through + ROW / 2., quick(), window, cx)
        });
        let line = (way_shown > 0.001).then(|| {
            let (to, edge, color) = (tab_y + ROW / 2., tab_x, p.accent.opacity(way_shown));
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let at = |x: Pixels, y: Pixels| point(bounds.left() + x, bounds.top() + y);
                    let bend = px(6.);
                    let mut x = GUTTER + px(2.);
                    let mut path = PathBuilder::stroke(px(1.5));
                    path.move_to(at(x + bend, from));
                    path.curve_to(at(x, from + bend), at(x, from));
                    if let Some(through) = through.filter(|through| *through > from + bend * 2.) {
                        path.line_to(at(x, through - bend));
                        path.curve_to(at(x + INDENT / 2., through), at(x, through));
                        path.curve_to(at(x + INDENT, through + bend), at(x + INDENT, through));
                        x += INDENT;
                    }
                    if to - bend > from + bend {
                        path.line_to(at(x, to - bend));
                    }
                    path.curve_to(at(x + bend, to), at(x, to));
                    if edge > x + bend {
                        path.line_to(at(edge, to));
                    }
                    if let Ok(path) = path.build() {
                        window.paint_path(path, color);
                    }
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full()
        });

        // Where a dragged folder would land.
        let landing = self.panel.landing.map(|at| {
            let y = match self.panel.blocks.get(at) {
                // In the gap just above the folder it would go before.
                Some((top, _)) => *top - ROW_GAP,
                None => extent,
            };
            let y = transition("panel-landing", y.max(px(0.)), quick(), window, cx);
            div()
                .absolute()
                .top(y)
                .left(GUTTER)
                .right(GUTTER)
                .h(px(2.))
                .rounded_full()
                .bg(p.accent)
        });

        // A folder whose name has scrolled off the top, while its files are
        // still in view, keeps its name there.
        let stuck = placed.iter().enumerate().find_map(|(at, block)| {
            let Item::Folder(ix) = block.item else {
                return None;
            };
            let end = placed[at + 1..]
                .iter()
                .find(|placed| matches!(placed.item, Item::Folder(_)))
                .map_or(extent, |next| next.top)
                - FOLDER_GAP;
            (scrolled > block.top + px(1.) && scrolled < end - ROW).then(|| {
                // The next folder's name pushes it off as it arrives.
                let pushed = (end - ROW - ROW_GAP - scrolled).min(px(0.));
                (ix, block.key, pushed)
            })
        });
        let sticky = stuck.map(|(ix, key, pushed)| {
            div()
                .absolute()
                .top(pushed)
                .left_0()
                .right_0()
                .px(GUTTER)
                .pb(px(3.))
                .bg(p.surface_app)
                .border_b_1()
                .border_color(p.border_subtle)
                .shadow_sm()
                .child(self.render_folder(ix, key, arrows, p, window, cx))
        });

        div()
            .relative()
            .flex_1()
            .min_h_0()
            .mt(px(2.))
            .child(
                div()
                    .id("panel-list")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.panel.scroll)
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .h(extent + px(12.))
                            .children(tab)
                            .children(line)
                            .children(rows)
                            .children(landing)
                            .children(self.menu_row.is_some().then(|| unmark_menu_row(cx))),
                    ),
            )
            .children(sticky)
            .into_any_element()
    }

    /// The title of a section. "Files" offers to close them all; "Folders" to add one.
    fn render_header(
        &self,
        label: &'static str,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let title = h_flex()
            .id(label)
            .h(HEADER)
            .pt(px(12.))
            .pl(INSET)
            .pr(px(6.))
            .justify_between()
            .child(section_label(label, p));
        match label {
            "FOLDERS" => title
                .child(
                    self.hoverable(
                        icon_button("add-folder", IconName::Plus, p),
                        Hot::Button("add-folder"),
                        p.surface_hover,
                        window,
                        cx,
                    )
                    .size(SPOT)
                    .tooltip(|window, cx| Tooltip::new("Add a folder").build(window, cx))
                    .on_click(cx.listener(|this, _, window, cx| this.open_dialog(window, cx))),
                )
                .into_any_element(),
            "FILES" => {
                // Shown while the pointer is in the panel, like the arrows.
                let (hover, strong) = (p.surface_hover, p.text_primary);
                title
                    .child(
                        div()
                            .id("close-all")
                            .px(px(6.))
                            .py(px(2.))
                            .rounded(size::RADIUS_XS)
                            .text_size(size::FONT_UI_SMALL)
                            .text_color(p.text_tertiary)
                            .when(!self.panel.pointer_in, |all| all.invisible())
                            .hover(move |style| style.bg(hover).text_color(strong))
                            .child("Close all")
                            .on_click(cx.listener(|this, _, window, cx| {
                                // Those with unsaved changes ask first, one at a time.
                                let ids: Vec<usize> = this
                                    .singles()
                                    .into_iter()
                                    .map(|ix| this.docs[ix].id)
                                    .collect();
                                for id in ids {
                                    if let Some(ix) = this.doc_ix(id) {
                                        this.request_close(ix, window, cx);
                                    }
                                }
                            })),
                    )
                    .into_any_element()
            }
            _ => title.into_any_element(),
        }
    }

    /// One of the small buttons that show on a row under the pointer.
    #[allow(clippy::too_many_arguments)]
    fn render_button(
        &self,
        key: u64,
        act: Act,
        path: Option<PathBuf>,
        doc: Option<usize>,
        pinned: bool,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let done = self.panel.done == Some((key, act));
        let (id, icon, says) = match act {
            Act::CopyContent => ("copy-content", IconName::Copy, "Copy content"),
            Act::CopyPath => ("copy-path", IconName::Code, "Copy path"),
            Act::Close => ("close-file", IconName::X, "Close"),
            Act::Pin if pinned => ("pin-folder", IconName::PinOff, "Unpin"),
            Act::Pin => ("pin-folder", IconName::Pin, "Pin to the top"),
            Act::Unlist => ("unlist-folder", IconName::X, "Take off the list"),
        };
        let (hover, strong) = (p.surface_raised, p.text_primary);
        div()
            .id((id, key as usize))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(SPOT)
            .rounded(size::RADIUS_SM)
            .map(|button| match done {
                // It turns into a tick for a moment: that worked.
                true => button
                    .bg(p.status_success_soft)
                    .text_color(p.status_success),
                false => button
                    .text_color(p.text_secondary)
                    .hover(move |style| style.bg(hover).text_color(strong)),
            })
            .child(Icon::new(if done { IconName::Check } else { icon }).size(px(14.)))
            .tooltip(move |window, cx| {
                let tip = Tooltip::new(if done { "Copied" } else { says });
                match act {
                    Act::CopyContent => tip.action(&CopyFileContent, None),
                    Act::CopyPath => tip.action(&CopyFilePath, None),
                    _ => tip,
                }
                .build(window, cx)
            })
            // A press on a button is not the start of dragging the row it is on.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.act(key, act, path.clone(), doc, window, cx);
            }))
    }

    /// A row that is being named: it holds the field for its name, where the name goes.
    fn render_naming(
        &self,
        input: &Entity<InputState>,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let field = Input::new(input)
            .appearance(false)
            .bordered(false)
            .small()
            .px_0()
            .text_size(size::FONT_UI);
        h_flex()
            .h(ROW)
            // One less than a plain row, making up for the border.
            .pl(INSET - px(1.))
            .pr(px(7.))
            .rounded(px(7.))
            .border_1()
            .border_color(p.accent_border)
            .bg(p.surface_raised)
            // Escape gives up.
            .on_action(cx.listener(|this, _: &Escape, window, cx| this.cancel_naming(window, cx)))
            // A click anywhere else is the end of naming, even where it moves no focus.
            .on_mouse_down_out(cx.listener(|this, _, window, cx| this.finish_naming(window, cx)))
            .child(div().flex_1().min_w_0().child(field))
            .into_any_element()
    }

    /// The row of a file: its name, and at its right end one spot that holds
    /// the dot for unsaved changes at rest and the buttons under the pointer.
    fn render_file(
        &self,
        row: FileRow,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Some(input) = row.path.as_deref().and_then(|path| self.naming_at(path)) {
            return self.render_naming(&input, p, cx);
        }
        let hot = Hot::File(row.key);
        let warmth = self.warmth(hot, window, cx);
        let pointed = self.hot == Some(hot);
        let chosen = self.panel.cursor == Some(row.key);
        let open = row.doc.or_else(|| {
            let path = row.path.as_ref()?;
            self.docs
                .iter()
                .position(|doc| doc.path.as_ref() == Some(path))
        });
        let dirty = open.is_some_and(|ix| self.docs[ix].dirty());
        let acts: &[Act] = match (row.single, row.path.is_some()) {
            (true, true) => &[Act::CopyContent, Act::CopyPath, Act::Close],
            (true, false) => &[Act::CopyContent, Act::Close],
            (false, _) => &[Act::CopyContent, Act::CopyPath],
        };
        let acting = pointed || chosen;

        // The part of the name that was searched for stands out.
        let name = match row.found {
            (_, 0) => div().flex_none().child(row.name.clone()),
            (at, len) => {
                let (before, rest) = row.name.split_at(at);
                let (found, after) = rest.split_at(len);
                h_flex()
                    .flex_none()
                    .child(before.to_string())
                    .child(
                        div()
                            .font_weight(FontWeight::BOLD)
                            .text_color(p.accent_text)
                            .child(found.to_string()),
                    )
                    .child(after.to_string())
            }
        };
        let spot = div()
            .absolute()
            .right(px(4.))
            .top((ROW - SPOT) / 2.)
            .h(SPOT)
            .flex()
            .items_center();
        let spot = if acting || warmth > 0.01 {
            spot.gap(px(1.))
                .opacity(if chosen { 1. } else { warmth })
                .children(acts.iter().map(|act| {
                    self.render_button(row.key, *act, row.path.clone(), row.doc, false, p, cx)
                }))
        } else if dirty {
            spot.w(SPOT)
                .justify_center()
                .child(div().size(px(6.)).rounded_full().bg(p.accent))
        } else {
            spot
        };

        // What the row is drawn on: the panel, the tab under the open file as it
        // arrives, and the fill that fades in under the pointer.
        let fill = match row.lit && !chosen {
            true => p.surface_hover.opacity(0.),
            false => p.surface_hover,
        };
        let tabbed = transition(
            ElementId::from(("panel-lit", row.key as usize)),
            if row.lit { 1. } else { 0. },
            motion::glide(motion::QUICK, cx),
            window,
            cx,
        );
        let backdrop = p
            .surface_app
            .blend(p.surface_raised.opacity(tabbed))
            .blend(fill.opacity(warmth));
        let (target, doc) = (row.path.clone(), row.doc.map(|ix| self.docs[ix].id));
        let searching = !self.panel.query.is_empty();
        let item = h_flex()
            .id(("file", row.key as usize))
            .relative()
            .h(ROW)
            .pl(INSET)
            .pr(INSET)
            .gap(px(6.))
            .rounded(px(7.))
            // One weight for every file: the tab marks the open one, so no name ever shifts.
            .text_color(if row.lit {
                p.text_primary
            } else {
                p.text_secondary
            })
            .when(acting, |item| {
                item.pr(px(8.) + (SPOT + px(1.)) * acts.len() as f32)
            })
            .when(dirty && !acting, |item| item.pr(SPOT + px(8.)))
            // A name too long for its row fades out: before the buttons, when they are there.
            .child(fading(
                h_flex()
                    .flex_none()
                    .gap(px(8.))
                    .child(name)
                    .when(!row.way.is_empty(), |words| {
                        words.child(
                            div()
                                .flex_none()
                                .text_color(p.text_disabled)
                                .child(row.way.clone()),
                        )
                    }),
                backdrop,
            ))
            .children(row.place.clone().filter(|_| !acting).map(|place| {
                div()
                    .flex_none()
                    .max_w(relative(0.5))
                    .truncate()
                    .text_size(px(11.5))
                    .text_color(p.text_tertiary)
                    .child(place)
            }))
            .child(spot)
            .when(chosen, |item| {
                item.child(
                    div()
                        .absolute()
                        .inset_0()
                        .rounded(px(7.))
                        .border_1()
                        .border_color(p.accent),
                )
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                match (&target, doc.and_then(|id| this.doc_ix(id))) {
                    (Some(path), _) => {
                        if searching {
                            this.clear_search(window, cx);
                        }
                        this.open(path.clone(), window, cx);
                    }
                    (None, Some(ix)) => this.activate(ix, window, cx),
                    _ => {}
                }
            }));
        // The open file has its tab; the others answer the pointer.
        let item = self.hoverable(item, hot, fill, window, cx);
        let Some(path) = row.path else {
            return item.into_any_element();
        };
        self.with_menu(
            item,
            path,
            if row.single { Menu::Single } else { Menu::File },
            p,
            cx,
        )
    }

    /// A folder's name: the title its files are listed under.
    fn render_folder(
        &self,
        ix: usize,
        key: u64,
        arrows: f32,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let folder = &self.folders[ix];
        let hot = Hot::Folder(key);
        let warmth = self.warmth(hot, window, cx);
        let pointed = self.hot == Some(hot);
        let chosen = self.panel.cursor == Some(key);
        let acting = pointed || chosen;
        // The arrow turns to point down as the folder opens.
        let turn = transition(
            ElementId::from(("chevron", key as usize)),
            if folder.open { 1. } else { 0. },
            motion::glide(motion::QUICK, cx),
            window,
            cx,
        );
        let root = folder.root.clone();
        let spot = div()
            .absolute()
            .right(px(4.))
            .top((ROW - SPOT) / 2.)
            .h(SPOT)
            .flex()
            .items_center()
            .gap(px(1.));
        let spot = if acting || warmth > 0.01 {
            spot.opacity(if chosen { 1. } else { warmth })
                .child(self.render_button(
                    key,
                    Act::Pin,
                    Some(root.clone()),
                    None,
                    folder.pinned,
                    p,
                    cx,
                ))
                .child(self.render_button(key, Act::Unlist, Some(root.clone()), None, false, p, cx))
        } else {
            spot
        };
        // At rest: how many files a shut folder holds, and whether it is pinned.
        let facts = h_flex()
            .flex_none()
            .gap(px(6.))
            .when(!folder.open && !folder.files.is_empty(), |facts| {
                facts.child(
                    div()
                        .text_size(px(11.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(p.text_tertiary)
                        .child(folder.files.len().to_string()),
                )
            })
            .when(folder.pinned, |facts| {
                facts.child(
                    Icon::new(IconName::Pin)
                        .size(px(13.))
                        .text_color(p.accent_hover),
                )
            });
        let drag = DragFolder {
            root: root.clone(),
            name: folder.name.clone(),
        };
        let toggled = root.clone();
        let item = h_flex()
            .id(("folder", key as usize))
            .relative()
            .h(ROW)
            .pl(INSET)
            .pr(INSET)
            .gap(px(4.))
            .rounded(size::RADIUS_SM)
            .font_weight(FontWeight::SEMIBOLD)
            // Room for the two buttons, so a long name fades out before them.
            .when(acting, |item| item.pr(px(8.) + (SPOT + px(1.)) * 2.))
            .child(fading(
                h_flex()
                    .flex_none()
                    .gap(px(4.))
                    .child(div().flex_none().child(folder.name.clone()))
                    .child(
                        div().flex_none().opacity(arrows).child(
                            Icon::new(IconName::ChevronRight)
                                .size(px(12.))
                                .text_color(p.text_tertiary)
                                .rotate(radians(FRAC_PI_2 * turn)),
                        ),
                    ),
                p.surface_app.blend(p.surface_hover.opacity(warmth)),
            ))
            .when(!acting, |item| item.child(facts))
            .child(spot)
            .when(chosen, |item| {
                item.child(
                    div()
                        .absolute()
                        .inset_0()
                        .rounded(size::RADIUS_SM)
                        .border_1()
                        .border_color(p.accent),
                )
            })
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_folder(&toggled, cx)))
            // Its name is what a folder is moved by.
            .on_drag(drag, |drag, _, _, cx| cx.new(|_| drag.clone()));
        let item = self.hoverable(item, hot, p.surface_hover, window, cx);
        self.with_menu(item, root, Menu::Folder(folder.pinned), p, cx)
    }

    /// A subfolder: a name with a file count while it is shut.
    #[allow(clippy::too_many_arguments)]
    fn render_dir(
        &self,
        folder: usize,
        row: usize,
        key: u64,
        arrows: f32,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let listed = &self.folders[folder].rows[row];
        if let Some(input) = self.naming_input(listed) {
            return self.render_naming(&input, p, cx);
        }
        let hot = Hot::Folder(key);
        let warmth = self.warmth(hot, window, cx);
        let chosen = self.panel.cursor == Some(key);
        let turn = transition(
            ElementId::from(("chevron", key as usize)),
            if listed.open { 1. } else { 0. },
            motion::glide(motion::QUICK, cx),
            window,
            cx,
        );
        let path = listed.path.clone();
        let toggled = path.clone();
        let item = h_flex()
            .id(("dir", key as usize))
            .relative()
            .h(ROW)
            .pl(INSET)
            .pr(INSET)
            .gap(px(4.))
            .rounded(size::RADIUS_SM)
            .font_weight(FontWeight::MEDIUM)
            .child(fading(
                h_flex()
                    .flex_none()
                    .gap(px(4.))
                    .child(div().flex_none().child(listed.name.clone()))
                    .child(
                        div().flex_none().opacity(arrows).child(
                            Icon::new(IconName::ChevronRight)
                                .size(px(12.))
                                .text_color(p.text_tertiary)
                                .rotate(radians(FRAC_PI_2 * turn)),
                        ),
                    ),
                p.surface_app.blend(p.surface_hover.opacity(warmth)),
            ))
            .when(!listed.open, |item| {
                item.child(
                    div()
                        .flex_none()
                        .text_size(px(11.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(p.text_tertiary)
                        .child(listed.count.to_string()),
                )
            })
            .when(chosen, |item| {
                item.child(
                    div()
                        .absolute()
                        .inset_0()
                        .rounded(size::RADIUS_SM)
                        .border_1()
                        .border_color(p.accent),
                )
            })
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_dir(folder, &toggled, cx)));
        let item = self.hoverable(item, hot, p.surface_hover, window, cx);
        self.with_menu(item, path, Menu::Dir, p, cx)
    }

    /// Give a row its right-click menu, and mark the row while the menu is open.
    fn with_menu(
        &self,
        item: Stateful<Div>,
        path: PathBuf,
        menu: Menu,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let marked = path.clone();
        let item = item.on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, _, _, cx| {
                this.menu_row = Some(marked.clone());
                cx.notify();
            }),
        );
        let item = item.when(self.menu_row.as_ref() == Some(&path), |item| {
            let ring = div()
                .absolute()
                .inset_0()
                .rounded(px(7.))
                .border_1()
                .border_color(p.accent_border);
            item.child(ring)
        });
        // Each choice carries the row it was offered for.
        type Run = fn(&mut Workspace, &mut Window, &mut Context<Workspace>);
        let workspace = cx.weak_entity();
        item.context_menu(move |popup, _, _| {
            let choice = |label: &'static str, run: Run| {
                let (workspace, path) = (workspace.clone(), path.clone());
                PopupMenuItem::new(label).on_click(move |_, window, cx| {
                    workspace
                        .update(cx, |this, cx| {
                            this.context_row = Some(path.clone());
                            run(this, window, cx);
                        })
                        .ok();
                })
            };
            let new_file = choice("New File", |this, window, cx| {
                this.new_file_here(window, cx)
            });
            let reveal = choice(system::REVEAL, |this, _, cx| this.reveal_here(cx));
            let copy_path = choice("Copy Path", |this, _, cx| this.copy_path_here(cx));
            let copy_content = choice("Copy Content", |this, _, cx| this.copy_content_here(cx));
            let rename = choice("Rename…", |this, window, cx| this.rename_here(window, cx));
            let trash = choice(system::TRASH, |this, window, cx| {
                this.trash_here(window, cx)
            });
            let popup = popup.min_w(px(190.));
            match menu {
                Menu::File => popup
                    .item(copy_content)
                    .item(copy_path)
                    .item(reveal)
                    .separator()
                    .item(new_file)
                    .item(rename)
                    .item(trash),
                Menu::Single => popup
                    .item(copy_content)
                    .item(copy_path)
                    .item(reveal)
                    .separator()
                    .item(rename)
                    .item(trash)
                    .separator()
                    .item(choice("Close", |this, window, cx| {
                        let path = this.context_row.take();
                        let open = this.docs.iter().position(|doc| doc.path == path);
                        if let Some(ix) = open {
                            this.request_close(ix, window, cx);
                        }
                    })),
                Menu::Dir => popup
                    .item(new_file)
                    .separator()
                    .item(reveal)
                    .item(copy_path),
                Menu::Folder(pinned) => popup
                    .item(new_file)
                    .separator()
                    .item(reveal)
                    .item(copy_path)
                    .separator()
                    .item(choice(
                        if pinned { "Unpin" } else { "Pin to the Top" },
                        |this, _, cx| {
                            if let Some(root) = this.context_row.take() {
                                this.toggle_pin(&root, cx);
                            }
                        },
                    ))
                    .item(choice("Take off the List", |this, _, cx| {
                        if let Some(root) = this.context_row.take() {
                            this.remove_folder(&root, cx);
                        }
                    })),
            }
        })
        .into_any_element()
    }

    /// What the panel says when there is nothing in it: the only time it explains itself.
    fn render_nothing_open(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let (hover, strong) = (p.surface_hover, p.text_primary);
        v_flex()
            .flex_1()
            .min_h_0()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .pb(px(48.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(36.))
                    .mb(px(4.))
                    .rounded(px(10.))
                    .bg(p.surface_hover)
                    .text_color(p.text_secondary)
                    .child(Icon::new(IconName::Plus).size(px(16.))),
            )
            .child(
                div()
                    .text_size(px(13.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Nothing open yet"),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(p.text_secondary)
                    .child("Open a file, or drop a folder here."),
            )
            .child(
                h_flex()
                    .id("open-folder")
                    .mt(px(8.))
                    .h(px(28.))
                    .pl(px(11.))
                    .pr(px(6.))
                    .gap(px(8.))
                    .rounded(size::RADIUS_MD)
                    .border_1()
                    .border_color(p.border_strong)
                    .bg(p.surface_raised)
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .hover(move |style| style.bg(hover).text_color(strong))
                    .child("Open folder")
                    .child(key_cap(shortcut!("O"), p.text_tertiary, p.surface_sunken))
                    .on_click(cx.listener(|this, _, window, cx| this.open_dialog(window, cx))),
            )
            .into_any_element()
    }

    /// What the panel says while a folder is held over it: what letting go will add.
    fn render_dropping(&self, p: &Palette) -> Option<AnyElement> {
        let dropping = self.panel.dropping.as_ref()?;
        let found = match dropping.found {
            Some(1) => "1 Markdown file found".to_string(),
            Some(found) => format!("{found} Markdown files found"),
            None => "Looking inside…".to_string(),
        };
        Some(
            v_flex()
                .absolute()
                .top(px(50.))
                .bottom(px(6.))
                .left(px(6.))
                .right(px(6.))
                .items_center()
                .justify_center()
                .gap(px(6.))
                .rounded(px(10.))
                .border_2()
                .border_dashed()
                .border_color(p.accent)
                .bg(p.surface_app.opacity(0.9).blend(p.accent_soft.opacity(0.9)))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(36.))
                        .mb(px(4.))
                        .rounded(px(10.))
                        .bg(p.accent)
                        .text_color(p.text_on_accent)
                        .child(Icon::new(IconName::Plus).size(px(16.))),
                )
                .child(
                    div()
                        .text_size(px(13.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format!("Drop to add “{}”", dropping.name)),
                )
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(p.text_secondary)
                        .child(found),
                )
                .into_any_element(),
        )
    }

    fn render_outline(
        &self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let Some(doc) = self.active_doc().filter(|doc| !doc.headings.is_empty()) else {
            return div();
        };
        // The heading the caret is in: the last one at or above its line.
        let caret_line = doc.editor.read(cx).cursor_position().line;
        let current = doc
            .headings
            .iter()
            .rposition(|heading| heading.line <= caret_line);
        let top_level = doc
            .headings
            .iter()
            .map(|heading| heading.level)
            .min()
            .unwrap_or(1);
        let strong = p.text_primary;

        let mut rows = Vec::with_capacity(doc.headings.len());
        for (ix, heading) in doc.headings.iter().enumerate() {
            let line = heading.line;
            let item = h_flex()
                .id(("heading", ix))
                .h(OUTLINE_ROW_HEIGHT)
                .ml(GUTTER + INSET + px(1.))
                .mr(GUTTER)
                .pl(px(10.) + px(12.) * (heading.level - top_level) as f32)
                .pr(px(8.))
                .rounded_r(size::RADIUS_SM)
                .map(|item| match current == Some(ix) {
                    true => item
                        .text_color(p.accent_text)
                        .font_weight(FontWeight::MEDIUM),
                    false => item
                        .text_color(p.text_secondary)
                        .hover(move |style| style.text_color(strong)),
                })
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(heading.title.clone()),
                )
                .on_click(
                    cx.listener(move |this, _, window, cx| this.jump_to_line(line, window, cx)),
                );
            rows.push(self.hoverable(item, Hot::Heading(ix), p.surface_hover, window, cx));
        }

        // A thin rule runs down the outline; a yellow marker rides it to the current heading.
        let rule = div()
            .absolute()
            .top_0()
            .bottom_0()
            .left(GUTTER + INSET)
            .w(px(1.))
            .bg(p.border_subtle);
        let marker = current.map(|ix| {
            let top = OUTLINE_ROW_HEIGHT * ix as f32;
            let top = transition(
                "outline-marker",
                top,
                motion::glide(motion::QUICK, cx),
                window,
                cx,
            );
            div()
                .absolute()
                .top(top)
                .left(GUTTER + INSET)
                .w(px(2.))
                .h(OUTLINE_ROW_HEIGHT)
                .bg(p.accent)
        });

        // The outline folds away under its title, and comes back when the title is clicked.
        let open = self.outline_open;
        let full = OUTLINE_ROW_HEIGHT * doc.headings.len() as f32 + px(10.);
        // Never more than a good share of the window: the files keep their room.
        let full = full.min((window.viewport_size().height * 0.42).max(px(120.)));
        let height = transition(
            "outline-height",
            if open { full } else { px(0.) },
            motion::glide(motion::PANEL, cx),
            window,
            cx,
        );
        let shown = transition(
            "outline-shown",
            if open { 1. } else { 0. },
            motion::glide(motion::PANEL, cx),
            window,
            cx,
        );
        // The arrow says the title can be clicked: there while the pointer is
        // in the panel, and always while the outline is shut.
        let arrow = transition(
            "outline-arrow",
            if self.panel.pointer_in || !open {
                1.
            } else {
                0.
            },
            Transition::new(motion::QUICK).ease(motion::ease),
            window,
            cx,
        );
        let title = h_flex()
            .id("outline-title")
            .flex_none()
            .h(px(28.))
            .mt(px(8.))
            .mx(GUTTER)
            .pl(INSET)
            .pr(px(8.))
            .gap(px(4.))
            .rounded(size::RADIUS_SM)
            .child(section_label("OUTLINE", p))
            .child(
                div().flex_none().opacity(arrow).child(
                    Icon::new(IconName::ChevronRight)
                        .size(px(12.))
                        .text_color(p.text_tertiary)
                        .rotate(radians(FRAC_PI_2 * shown)),
                ),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.outline_open = !this.outline_open;
                cx.notify();
            }));
        let title = self.hoverable(
            title,
            Hot::Button("outline-title"),
            p.surface_hover,
            window,
            cx,
        );

        div()
            .flex_none()
            .flex()
            .flex_col()
            .pb(px(6.))
            .child(title)
            .child(
                // A window that opens over the headings: they keep their place
                // and their size while it grows and shrinks.
                div().h(height).overflow_hidden().opacity(shown).child(
                    div()
                        .id("outline")
                        .h(full)
                        .overflow_y_scroll()
                        .pb(px(10.))
                        .child(div().relative().child(rule).children(marker).children(rows)),
                ),
            )
    }
}

/// Which right-click menu a row has.
#[derive(Clone, Copy)]
enum Menu {
    /// A file inside a folder of the panel.
    File,
    /// A file open on its own.
    Single,
    /// A subfolder.
    Dir,
    /// A folder of the panel, and whether it is pinned.
    Folder(bool),
}

/// Paints nothing: it listens for what closes a menu (a click, Escape, Return)
/// and takes the mark off the row the menu was opened on.
fn unmark_menu_row(cx: &mut Context<Workspace>) -> impl IntoElement + use<> {
    let workspace = cx.weak_entity();
    canvas(
        |_, _, _| {},
        move |_, _, window, _| {
            let unmark = move |cx: &mut App| {
                workspace
                    .update(cx, |this, cx| {
                        this.menu_row = None;
                        cx.notify();
                    })
                    .ok();
            };
            let on_click = unmark.clone();
            window.on_mouse_event(move |_: &MouseDownEvent, phase, _, cx| {
                // Before the click reaches a row, so a right-click on another row marks that one.
                if phase == DispatchPhase::Capture {
                    on_click(cx);
                }
            });
            window.on_key_event(move |event: &KeyDownEvent, phase, _, cx| {
                let closes = matches!(event.keystroke.key.as_str(), "escape" | "enter");
                if phase == DispatchPhase::Capture && closes {
                    unmark(cx);
                }
            });
        },
    )
    .absolute()
    .size_0()
}
