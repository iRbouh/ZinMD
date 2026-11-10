//! Keeping unsaved work safe from a crash or a power cut.
//!
//! While a document has unsaved edits, its text is written to a backup file
//! shortly after each burst of typing. Saving or discarding the edits removes
//! the backup. If the app dies instead, the backups are still there at the next
//! launch and the edits come back, unsaved, in their tabs.

use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::*;
use serde_json::{Value, json};

use super::Workspace;
use crate::session::data_dir;

/// How long after the last keystroke the backup is written.
const SETTLE: Duration = Duration::from_millis(900);

fn folder() -> PathBuf {
    data_dir().join("unsaved")
}

fn file(key: &str) -> PathBuf {
    folder().join(format!("{key}.json"))
}

impl Workspace {
    /// Bring the backup of a document up to date: written soon if it has
    /// unsaved edits, removed if it has none.
    pub(super) fn back_up(&mut self, ix: usize, cx: &mut Context<Self>) {
        let doc = &mut self.docs[ix];
        let key = doc.backup_key.clone();
        if !doc.dirty() {
            doc._backup = Task::ready(());
            std::fs::remove_file(file(&key)).ok();
            return;
        }
        let backup =
            json!({ "path": doc.path, "name": doc.name.as_ref(), "text": doc.text.as_ref() });
        let pause = cx.background_executor().timer(SETTLE);
        doc._backup = cx.background_executor().spawn(async move {
            // Dropping this task (the next keystroke does) cancels the wait: only a pause in typing writes.
            pause.await;
            std::fs::create_dir_all(folder()).ok();
            std::fs::write(file(&key), backup.to_string()).ok();
        });
    }

    /// Forget every backup: the user has just chosen what happens to all unsaved edits.
    pub(super) fn drop_backups(&mut self) {
        for doc in &mut self.docs {
            doc._backup = Task::ready(());
            Self::drop_backup(&doc.backup_key);
        }
    }

    /// Forget the backup of a document that is closing.
    pub(super) fn drop_backup(key: &str) {
        std::fs::remove_file(file(key)).ok();
    }

    /// Bring back edits that were never saved, from backups left by a session
    /// that did not end properly. Called after the first frame.
    pub(super) fn restore_backups(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Ok(entries) = std::fs::read_dir(folder()) else {
            return;
        };
        for entry in entries.flatten() {
            let Ok(text) = std::fs::read_to_string(entry.path()) else {
                continue;
            };
            // Whatever happens next, this backup has been dealt with: the tab
            // it becomes writes its own as soon as it is seen to be unsaved.
            std::fs::remove_file(entry.path()).ok();
            let Ok(backup) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            let Some(unsaved) = backup["text"].as_str() else {
                continue;
            };
            let path = backup["path"].as_str().map(PathBuf::from);
            let name = backup["name"].as_str().unwrap_or("Untitled.md");
            crate::startup::trace(|| format!("recovering unsaved edits to {name:?}"));

            let open = path.as_ref().and_then(|path| {
                self.docs
                    .iter()
                    .position(|doc| doc.path.as_ref() == Some(path))
            });
            let ix = match open {
                Some(ix) => ix,
                None => {
                    // What is on disk now is the saved state; the backup is the unsaved edit on top.
                    let on_disk = path
                        .as_ref()
                        .and_then(|path| std::fs::read_to_string(path).ok())
                        .unwrap_or_default();
                    let at = self.docs.len();
                    self.insert_doc(at, path, name.to_string().into(), on_disk, None, window, cx);
                    at
                }
            };
            let editor = self.docs[ix].editor.clone();
            if self.docs[ix].text.as_ref() != unsaved {
                let unsaved = unsaved.to_string();
                editor.update(cx, |editor, cx| editor.set_value(unsaved, window, cx));
                self.on_edit(&editor, cx);
            }
        }
        cx.notify();
    }
}
