//! Managing files from the sidebar: new file, new folder, rename, move to the
//! Trash, show in Finder. A right-click on a row offers them for that row;
//! naming happens in the file list itself, in the row being named.

use std::path::{Path, PathBuf};

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;

use super::sidebar::ROW_HEIGHT;
use super::{Workspace, titlebar};
use crate::files::Row;
use crate::theme::size;
use crate::ui;

/// What the name being typed is for.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Naming {
    /// A new file, or folder, inside this folder.
    NewFile(PathBuf),
    NewFolder(PathBuf),
    /// A new name for this file or folder.
    Rename(PathBuf),
}

impl Naming {
    /// Whether `row` is the one this name is typed in: the row being renamed,
    /// or the blank row standing in for something new.
    fn is_for(&self, row: &Row) -> bool {
        match self {
            Naming::Rename(path) => *path == row.path,
            Naming::NewFile(_) | Naming::NewFolder(_) => row.name.is_empty(),
        }
    }
}

pub(super) struct NameField {
    naming: Naming,
    input: Entity<InputState>,
    _events: Subscription,
}

impl Workspace {
    /// The folder a "new" command should create in: the row that was
    /// right-clicked if it is a folder, the folder it is in if it is a file,
    /// and otherwise the folder the sidebar shows.
    fn target_folder(&self) -> Option<PathBuf> {
        match self.context_row.as_deref() {
            Some(path) if path.is_dir() => Some(path.to_path_buf()),
            Some(path) => path.parent().map(Path::to_path_buf),
            None => self.tree.root.clone(),
        }
    }

    pub(super) fn new_file_here(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.target_folder() {
            Some(folder) => self.ask_name(Naming::NewFile(folder), "", window, cx),
            // No folder to create it in: an untitled tab, named when it is saved.
            None => self.open_untitled(window, cx),
        }
    }

    pub(super) fn new_folder_here(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(folder) = self.target_folder() {
            self.ask_name(Naming::NewFolder(folder), "", window, cx);
        }
    }

    pub(super) fn rename_here(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self
            .context_row
            .clone()
            .or_else(|| self.active_doc().and_then(|doc| doc.path.clone()))
        else {
            return;
        };
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.ask_name(Naming::Rename(path), &name, window, cx);
    }

    /// Start naming: the row turns into a field holding `current`, with the caret in it.
    fn ask_name(
        &mut self,
        naming: Naming,
        current: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let placeholder = match &naming {
            Naming::NewFile(_) => "File name",
            Naming::NewFolder(_) => "Folder name",
            Naming::Rename(_) => "New name",
        };
        // The name is typed in the file list: the list, and the row, must be there to see.
        self.sidebar_open = true;
        if let Naming::Rename(path) = &naming {
            if let Some(folder) = path.parent() {
                self.tree.reveal(folder);
            }
        }
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(placeholder)
                .default_value(current.to_string())
        });
        // Renaming usually keeps the extension: select only what comes before it.
        let stem = Path::new(current).file_stem().map_or(0, |stem| stem.len());
        input.update(cx, |input, cx| {
            input.focus(window, cx);
            input.set_selected_range(0..stem, cx);
        });
        let events =
            cx.subscribe_in(
                &input,
                window,
                |this, _, event: &InputEvent, window, cx| match event {
                    InputEvent::PressEnter { .. } => this.finish_naming(window, cx),
                    // Moving on keeps what was typed, as Finder does. Escape is the way out.
                    InputEvent::Blur => this.finish_naming(window, cx),
                    _ => {}
                },
            );
        self.name_field = Some(NameField {
            naming,
            input,
            _events: events,
        });
        self.place_draft();
        self.show_naming_row();
        cx.notify();
    }

    pub(super) fn cancel_naming(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.name_field.take().is_some() {
            self.place_draft();
            if let Some(doc) = self.active_doc() {
                doc.editor.update(cx, |editor, cx| editor.focus(window, cx));
            }
            cx.notify();
        }
    }

    /// Type a name into the field, for scripted runs. `key:enter` then confirms it.
    #[cfg(feature = "shot")]
    pub(super) fn debug_name(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(field) = &self.name_field {
            let name = name.to_string();
            field
                .input
                .update(cx, |input, cx| input.set_value(name, window, cx));
        }
    }

    /// Carry out what the name was asked for. An empty name asks for nothing.
    pub(super) fn finish_naming(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(field) = self.name_field.take() else {
            return;
        };
        self.place_draft();
        let name = field.input.read(cx).value().trim().to_string();
        // A name is one path component; anything else is a slip, not a wish.
        if name.is_empty() || name.contains('/') || name == "." || name == ".." {
            if let Some(doc) = self.active_doc() {
                doc.editor.update(cx, |editor, cx| editor.focus(window, cx));
            }
            return cx.notify();
        }
        let result = match &field.naming {
            Naming::NewFile(folder) => {
                let path = folder.join(with_extension(&name, "md"));
                create_new(&path).map(|()| {
                    self.tree.refresh();
                    self.open(path, window, cx);
                })
            }
            Naming::NewFolder(folder) => {
                let path = folder.join(&name);
                std::fs::create_dir(&path).map(|()| {
                    self.tree.refresh();
                    self.tree.reveal(&path.join("."));
                })
            }
            Naming::Rename(old) => {
                // A file keeps its extension unless a new one was typed.
                let kept = old
                    .extension()
                    .filter(|_| old.is_file())
                    .map(|ext| ext.to_string_lossy().into_owned());
                let name = kept.map_or(name.clone(), |ext| with_extension(&name, &ext));
                let new = old.with_file_name(name);
                rename(old, &new).map(|()| self.renamed(old, &new, window, cx))
            }
        };
        if let Err(error) = result {
            let message = match field.naming {
                Naming::Rename(_) => "The name could not be changed.",
                _ => "That could not be created.",
            };
            drop(window.prompt(
                PromptLevel::Critical,
                message,
                Some(&error.to_string()),
                &["OK"],
                cx,
            ));
        }
        if let Some(doc) = self.active_doc() {
            doc.editor.update(cx, |editor, cx| editor.focus(window, cx));
        }
        cx.notify();
    }

    /// A file or folder moved from `old` to `new`: open documents follow it.
    fn renamed(&mut self, old: &Path, new: &Path, window: &mut Window, cx: &mut Context<Self>) {
        for doc in &mut self.docs {
            let Some(rest) = doc
                .path
                .as_deref()
                .and_then(|path| path.strip_prefix(old).ok())
            else {
                continue;
            };
            // `rest` is empty for the file itself, and the way down for files in a renamed folder.
            let path = if rest.as_os_str().is_empty() {
                new.to_path_buf()
            } else {
                new.join(rest)
            };
            doc.name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
                .into();
            doc.tab_width = titlebar::tab_width(ui::text_width(
                &doc.name,
                size::FONT_UI,
                FontWeight::MEDIUM,
                window,
            ));
            doc.disk = crate::watch::stamp(&path);
            doc.notice = None;
            if let Some(watcher) = &mut self.watcher {
                watcher.include(&path);
            }
            doc.path = Some(path);
        }
        self.tree.refresh();
        self.refresh_tree_soon(cx);
        if let Some(doc) = self.active_doc() {
            window.set_window_title(&doc.name);
        }
    }

    /// Move the right-clicked file or folder to the Trash, where it can be got back from.
    pub(super) fn trash_here(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.context_row.take() else {
            return;
        };
        if let Err(error) = move_to_trash(&path) {
            drop(window.prompt(
                PromptLevel::Critical,
                "That could not be moved to the Trash.",
                Some(&error.to_string()),
                &["OK"],
                cx,
            ));
            return;
        }
        // Tabs showing what was trashed close, unless they hold unsaved edits:
        // those stay, and say their file is gone.
        let gone: Vec<usize> = self
            .docs
            .iter()
            .filter(|doc| {
                !doc.dirty()
                    && doc
                        .path
                        .as_deref()
                        .is_some_and(|open| open.starts_with(&path))
            })
            .map(|doc| doc.id)
            .collect();
        for id in gone {
            if self.docs.len() > 1 {
                self.close_doc(id, window, cx);
            }
        }
        self.tree.refresh();
        self.refresh_tree_soon(cx);
        cx.notify();
    }

    pub(super) fn reveal_here(&mut self, cx: &mut Context<Self>) {
        let path = self
            .context_row
            .clone()
            .or_else(|| self.active_doc().and_then(|doc| doc.path.clone()));
        if let Some(path) = path {
            cx.reveal_path(&path);
        }
    }

    pub(super) fn copy_path_here(&mut self, cx: &mut Context<Self>) {
        let path = self
            .context_row
            .clone()
            .or_else(|| self.active_doc().and_then(|doc| doc.path.clone()));
        if let Some(path) = path {
            cx.write_to_clipboard(ClipboardItem::new_string(
                path.to_string_lossy().into_owned(),
            ));
        }
    }

    /// The field for naming `row`, if it is the row being named.
    pub(super) fn naming_input(&self, row: &Row) -> Option<Entity<InputState>> {
        let field = self.name_field.as_ref()?;
        field.naming.is_for(row).then(|| field.input.clone())
    }

    /// Keep the file list in step with what is being named. Something new gets
    /// a blank row at the top of the folder it will be created in, which is
    /// opened if it was shut. Without anything being named, that row goes.
    pub(super) fn place_draft(&mut self) {
        // A real row always has a name: an empty one marks the draft.
        self.tree.rows.retain(|row| !row.name.is_empty());
        let Some(naming) = self.name_field.as_ref().map(|field| field.naming.clone()) else {
            return;
        };
        // No real file can have this name, so the draft is never mistaken for one.
        let unnamed = |folder: &Path| folder.join("\0");
        let (folder, path, is_dir) = match naming {
            Naming::NewFile(folder) => (Some(folder.clone()), unnamed(&folder), false),
            Naming::NewFolder(folder) => (Some(folder.clone()), unnamed(&folder), true),
            Naming::Rename(path) => {
                if self.tree.rows.iter().any(|row| row.path == path) {
                    return;
                }
                // A file from outside the folder shown has no row: it borrows one.
                let is_dir = path.is_dir();
                (None, path, is_dir)
            }
        };
        let under = folder.and_then(|folder| {
            let mut rows = self.tree.rows.iter();
            rows.position(|row| row.is_dir && row.path == folder)
        });
        let (at, depth) = match under {
            Some(ix) => {
                if !self.tree.rows[ix].open {
                    self.tree.toggle(ix);
                }
                (ix + 1, self.tree.rows[ix].depth + 1)
            }
            // The folder the sidebar shows has no row of its own: its files start the list.
            None => (0, 0),
        };
        let draft = Row {
            path,
            name: SharedString::default(),
            is_dir,
            depth,
            open: false,
        };
        self.tree.rows.insert(at, draft);
    }

    /// Scroll the file list just enough to show the row being named.
    fn show_naming_row(&mut self) {
        let Some(naming) = self.name_field.as_ref().map(|field| &field.naming) else {
            return;
        };
        let Some(ix) = self.tree.rows.iter().position(|row| naming.is_for(row)) else {
            return;
        };
        let top = ROW_HEIGHT * ix as f32;
        let view = self.tree_scroll.bounds().size.height;
        let scrolled = -self.tree_scroll.offset().y;
        if top < scrolled {
            self.tree_scroll.set_offset(point(px(0.), -top));
        } else if view > px(0.) && top + ROW_HEIGHT > scrolled + view {
            self.tree_scroll
                .set_offset(point(px(0.), -(top + ROW_HEIGHT - view)));
        }
    }
}

/// Move a file or folder to the Trash.
///
/// Done through the file system rather than by asking Finder to: asking
/// Finder makes macOS stop and ask the user for permission to control it.
fn move_to_trash(path: &Path) -> Result<(), trash::Error> {
    #[allow(unused_mut)]
    let mut trash = trash::TrashContext::default();
    #[cfg(target_os = "macos")]
    {
        use trash::macos::{DeleteMethod, TrashContextExtMacos as _};
        trash.set_delete_method(DeleteMethod::NsFileManager);
    }
    trash.delete(path)
}

/// `name`, with `.extension` added if it has none of its own.
fn with_extension(name: &str, extension: &str) -> String {
    match Path::new(name).extension() {
        Some(_) => name.to_string(),
        None => format!("{name}.{extension}"),
    }
}

/// Create an empty file, refusing to replace one that is already there.
fn create_new(path: &Path) -> std::io::Result<()> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map(|_| ())
}

/// Rename, refusing to replace something that already has the new name.
fn rename(old: &Path, new: &Path) -> std::io::Result<()> {
    if old == new {
        return Ok(());
    }
    // On a case-insensitive disk "a.md" and "A.md" are the same file; that rename is fine.
    let same_file = old.to_string_lossy().to_lowercase() == new.to_string_lossy().to_lowercase();
    if new.exists() && !same_file {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "Something with that name is already there.",
        ));
    }
    std::fs::rename(old, new)
}

#[cfg(test)]
mod tests {
    use super::{create_new, rename, with_extension};

    #[test]
    fn adds_an_extension_only_when_missing() {
        assert_eq!(with_extension("notes", "md"), "notes.md");
        assert_eq!(with_extension("notes.txt", "md"), "notes.txt");
        assert_eq!(with_extension("v1.2 plan", "md"), "v1.2 plan");
    }

    #[test]
    fn never_replaces_existing_files() {
        let folder = std::env::temp_dir().join(format!("zinmd-fileops-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let (a, b) = (folder.join("a.md"), folder.join("b.md"));
        create_new(&a).unwrap();
        assert!(create_new(&a).is_err());
        std::fs::write(&b, "keep me").unwrap();
        assert!(rename(&a, &b).is_err());
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "keep me");
        rename(&a, &folder.join("c.md")).unwrap();
        assert!(folder.join("c.md").exists() && !a.exists());
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
