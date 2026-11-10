//! The main window: open documents, the layout around them, and the commands
//! that change either. Each strip of the window draws itself in a submodule.

mod sidebar;
mod statusbar;
mod titlebar;

use std::path::{Path, PathBuf};
use std::time::Instant;

use gpui_kit::component::input::{Editor, EditorState, InputEvent, Position};
use gpui_kit::component::text::TextView;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::doc::{self, Heading, Stats};
use crate::files::{self, FileTree};
use crate::startup;
use crate::theme::{self, ActivePalette as _, size};

actions!(
    zwinmd,
    [ToggleSidebar, ToggleTheme, ViewEditor, ViewSplit, ViewPreview, CloseTab, NextTab, PreviousTab, Quit]
);

/// Register the app's keyboard shortcuts. Call once at startup.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-b", ToggleSidebar, None),
        KeyBinding::new("cmd-1", ViewEditor, None),
        KeyBinding::new("cmd-2", ViewSplit, None),
        KeyBinding::new("cmd-3", ViewPreview, None),
        KeyBinding::new("cmd-w", CloseTab, None),
        KeyBinding::new("cmd-}", NextTab, None),
        KeyBinding::new("cmd-{", PreviousTab, None),
        KeyBinding::new("ctrl-tab", NextTab, None),
        KeyBinding::new("ctrl-shift-tab", PreviousTab, None),
        KeyBinding::new("cmd-q", Quit, None),
    ]);
    cx.on_action(|_: &Quit, cx| cx.quit());
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
    /// The source as of the last edit; what the preview renders.
    text: SharedString,
    headings: Vec<Heading>,
    stats: Stats,
    dirty: bool,
    _changes: Subscription,
}

pub struct Workspace {
    docs: Vec<Doc>,
    active: usize,
    next_doc_id: usize,
    view: ViewMode,
    sidebar_open: bool,
    tree: FileTree,
    tree_scroll: UniformListScrollHandle,
    focus: FocusHandle,
    /// When the process started, to report how long opening took.
    started: Instant,
    opened_ms: Option<f64>,
    first_frame_requested: bool,
}

impl Workspace {
    /// `path` may be a Markdown file, a folder, or nothing.
    pub fn new(path: Option<PathBuf>, started: Instant, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            docs: Vec::new(),
            active: 0,
            next_doc_id: 0,
            view: ViewMode::Split,
            sidebar_open: true,
            tree: FileTree::default(),
            tree_scroll: UniformListScrollHandle::new(),
            focus: cx.focus_handle(),
            started,
            opened_ms: None,
            first_frame_requested: false,
        };

        let path = path.map(|path| path.canonicalize().unwrap_or(path));
        let (root, file) = match path {
            Some(dir) if dir.is_dir() => {
                let readme = ["README.md", "readme.md", "index.md"].iter().map(|name| dir.join(name)).find(|p| p.is_file());
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
        startup::mark("document loaded");
        this
    }

    fn active_doc(&self) -> Option<&Doc> {
        self.docs.get(self.active)
    }

    // ------------------------------------------------------------ documents

    /// Show `path` in a tab, reusing its tab if it is already open.
    pub fn open_path(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.docs.iter().position(|doc| doc.path.as_deref() == Some(path)) {
            self.activate(ix, window, cx);
            return;
        }
        // Not valid UTF-8, or unreadable: open what we can rather than nothing.
        let text = std::fs::read(path).map(|bytes| String::from_utf8_lossy(&bytes).into_owned()).unwrap_or_default();
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
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
            EditorState::new(window, cx).language("markdown").line_number(true).soft_wrap(true).default_value(text.clone())
        });
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
            headings: doc::outline(&text),
            stats: Stats::of(&text),
            text,
            dirty: false,
            _changes: changes,
        });
        self.activate(self.docs.len() - 1, window, cx);
    }

    fn on_edit(&mut self, editor: &Entity<EditorState>, cx: &mut Context<Self>) {
        let Some(doc) = self.docs.iter_mut().find(|doc| doc.editor == *editor) else { return };
        let text = SharedString::from(editor.read(cx).text().to_string());
        doc.headings = doc::outline(&text);
        doc.stats = Stats::of(&text);
        doc.text = text;
        doc.dirty = true;
        cx.notify();
    }

    fn activate(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.docs.len() {
            return;
        }
        self.active = ix;
        self.docs[ix].editor.update(cx, |editor, cx| editor.focus(window, cx));
        if let Some(path) = self.docs[ix].path.clone() {
            self.tree.reveal(&path);
        }
        cx.notify();
    }

    fn close_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.docs.len() {
            return;
        }
        self.docs.remove(ix);
        if self.docs.is_empty() {
            window.remove_window();
            return;
        }
        let next = if ix < self.active { self.active - 1 } else { self.active.min(self.docs.len() - 1) };
        self.activate(next, window, cx);
    }

    fn cycle_tab(&mut self, step: isize, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.docs.len() as isize;
        if count > 1 {
            self.activate((self.active as isize + step).rem_euclid(count) as usize, window, cx);
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

    // ------------------------------------------------------------- sidebar

    /// Point the file tree at a folder. The folder is read off the main thread.
    fn set_root(&mut self, root: Option<PathBuf>, cx: &mut Context<Self>) {
        self.tree = FileTree { root: root.clone(), rows: Vec::new() };
        let Some(root) = root else { return };
        let reveal = self.active_doc().and_then(|doc| doc.path.clone());
        cx.spawn(async move |this, cx| {
            let tree = cx
                .background_executor()
                .spawn(async move {
                    let mut tree = FileTree { rows: files::read_dir(&root, 0), root: Some(root) };
                    if let Some(path) = reveal {
                        tree.reveal(&path);
                    }
                    tree
                })
                .await;
            this.update(cx, |this, cx| {
                this.tree = tree;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn on_tree_click(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(row) = self.tree.rows.get(ix) else { return };
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
        self.view = view;
        cx.notify();
    }

    fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_open = !self.sidebar_open;
        cx.notify();
    }

    // ---------------------------------------------------------------- panes

    fn render_panes(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = *cx.palette();
        let row = h_flex().flex_1().min_w_0().h_full();
        let Some(doc) = self.active_doc() else { return row };

        let editor = (self.view != ViewMode::Preview).then(|| {
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .bg(p.surface_editor)
                .child(Editor::new(&doc.editor).appearance(false).bordered(false).h_full())
        });
        let preview = (self.view != ViewMode::Editor).then(|| {
            div()
                .id(("preview", doc.id))
                .flex_1()
                .min_w_0()
                .h_full()
                .bg(p.surface_preview)
                .when(self.view == ViewMode::Split, |pane| pane.border_l_1().border_color(p.border_subtle))
                .overflow_y_scroll()
                .child(
                    div()
                        .mx_auto()
                        .max_w(px(820.))
                        .px(px(48.))
                        .py(px(40.))
                        .text_size(size::FONT_BODY)
                        .child(TextView::markdown(("markdown", doc.id), doc.text.clone()).selectable(true)),
                )
        });
        row.children(editor).children(preview)
    }

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
        let p = *cx.palette();
        let titlebar = self.render_titlebar(window, cx);
        let sidebar = self.sidebar_open.then(|| self.render_sidebar(cx));
        let panes = self.render_panes(cx);
        let statusbar = self.render_statusbar(cx);

        v_flex()
            .id("workspace")
            .size_full()
            .bg(p.surface_app)
            .text_color(p.text_primary)
            .text_size(size::FONT_UI)
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| this.toggle_sidebar(cx)))
            .on_action(cx.listener(|_, _: &ToggleTheme, _, cx| theme::toggle(cx)))
            .on_action(cx.listener(|this, _: &ViewEditor, _, cx| this.set_view(ViewMode::Editor, cx)))
            .on_action(cx.listener(|this, _: &ViewSplit, _, cx| this.set_view(ViewMode::Split, cx)))
            .on_action(cx.listener(|this, _: &ViewPreview, _, cx| this.set_view(ViewMode::Preview, cx)))
            .on_action(cx.listener(|this, _: &CloseTab, window, cx| this.close_tab(this.active, window, cx)))
            .on_action(cx.listener(|this, _: &NextTab, window, cx| this.cycle_tab(1, window, cx)))
            .on_action(cx.listener(|this, _: &PreviousTab, window, cx| this.cycle_tab(-1, window, cx)))
            .child(titlebar)
            .child(h_flex().flex_1().min_h_0().w_full().children(sidebar).child(panes))
            .child(statusbar)
    }
}
