//! Managing files from the sidebar: new file, new folder, rename, move to the
//! Trash, show in Finder. A right-click on a row offers them for that row;
//! naming happens in a small field at the top of the file list.

use std::path::{Path, PathBuf};

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Icon, Sizable as _, h_flex};
use gpui_kit::*;

use super::{Workspace, titlebar};
use crate::theme::{Palette, size};
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

    /// Show the name field, holding `current`, and put the caret in it.
    fn ask_name(
        &mut self,
        naming: Naming,
        current: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let placeholder = match naming {
            Naming::NewFile(_) => "Name of the new file",
            Naming::NewFolder(_) => "Name of the new folder",
            Naming::Rename(_) => "New name",
        };
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
                    // Clicking away leaves things as they were.
                    InputEvent::Blur => this.cancel_naming(window, cx),
                    _ => {}
                },
            );
        self.name_field = Some(NameField {
            naming,
            input,
            _events: events,
        });
        cx.notify();
    }

    pub(super) fn cancel_naming(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.name_field.take().is_some() {
            if let Some(doc) = self.active_doc() {
                doc.editor.update(cx, |editor, cx| editor.focus(window, cx));
            }
            cx.notify();
        }
    }

    /// Type a name into the field and press Return, for scripted runs.
    #[cfg(feature = "shot")]
    pub(super) fn debug_name(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(field) = &self.name_field {
            let name = name.to_string();
            field
                .input
                .update(cx, |input, cx| input.set_value(name, window, cx));
            self.finish_naming(window, cx);
        }
    }

    /// Carry out what the name was asked for.
    fn finish_naming(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(field) = self.name_field.take() else {
            return;
        };
        let name = field.input.read(cx).value().trim().to_string();
        // A name is one path component; anything else is a slip, not a wish.
        if name.is_empty() || name.contains('/') || name == "." || name == ".." {
            return self.cancel_naming(window, cx);
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

    /// The field a name is typed into, shown at the top of the file list while one is wanted.
    pub(super) fn render_name_field(
        &self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let field = self.name_field.as_ref()?;
        let icon = match field.naming {
            Naming::NewFile(_) => IconName::FilePlus,
            Naming::NewFolder(_) => IconName::FolderPlus,
            Naming::Rename(_) => IconName::Pencil,
        };
        Some(
            h_flex()
                .flex_none()
                .h(px(30.))
                .mx(px(10.))
                .mb(px(4.))
                .pl(px(8.))
                .gap(px(2.))
                .rounded(size::RADIUS_SM)
                .border_1()
                .border_color(p.accent_border)
                .bg(p.surface_raised)
                // Escape gives up.
                .on_action(
                    cx.listener(|this, _: &gpui_kit::base::input::Escape, window, cx| {
                        this.cancel_naming(window, cx)
                    }),
                )
                .child(Icon::new(icon).size(px(14.)).text_color(p.accent_hover))
                .child(
                    div().flex_1().min_w_0().child(
                        Input::new(&field.input)
                            .appearance(false)
                            .bordered(false)
                            .small(),
                    ),
                ),
        )
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
        let folder = std::env::temp_dir().join(format!("zwinmd-fileops-{}", std::process::id()));
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
