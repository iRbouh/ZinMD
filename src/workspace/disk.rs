//! Keeping open documents and the sidebar in step with what is on disk.

use std::path::PathBuf;
use std::time::Duration;

use futures::StreamExt as _;
use gpui_kit::component::h_flex;
use gpui_kit::*;

use super::{Workspace, palette};
use crate::theme::{Palette, size};
use crate::watch;

/// Something the user should know about a document's file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Notice {
    /// It changed on disk while there are unsaved edits here.
    Changed,
    /// It was deleted, moved or renamed.
    Missing,
}

impl Workspace {
    /// Begin watching the disk. Called once the first frame is on screen, so
    /// it costs startup nothing.
    pub(super) fn start_watching(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((mut watcher, mut changes)) = watch::start() else {
            return;
        };
        watcher.set_root(self.tree.root.as_deref());
        for doc in &self.docs {
            if let Some(path) = &doc.path {
                watcher.include(path);
            }
        }
        self.watcher = Some(watcher);
        self._watching = cx.spawn_in(window, async move |this, cx| {
            while let Some(mut paths) = changes.next().await {
                // Changes come in bursts (a save is often several steps); act once per burst.
                cx.background_executor()
                    .timer(Duration::from_millis(120))
                    .await;
                while let Ok(more) = changes.try_recv() {
                    paths.extend(more);
                }
                if this
                    .update_in(cx, |this, window, cx| this.disk_changed(&paths, window, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
    }

    fn disk_changed(&mut self, paths: &[PathBuf], window: &mut Window, cx: &mut Context<Self>) {
        // Documents: one look at each open file says whether it is the one that changed.
        for ix in 0..self.docs.len() {
            let doc = &self.docs[ix];
            let Some(path) = doc.path.clone() else {
                continue;
            };
            let now = watch::stamp(&path);
            if now == doc.disk {
                continue;
            }
            crate::startup::trace(|| format!("changed on disk: {path:?}"));
            match now {
                None => {
                    self.docs[ix].disk = None;
                    self.docs[ix].notice = Some(Notice::Missing);
                }
                // Nothing unsaved here: simply follow the disk.
                Some(_) if !doc.dirty() => self.reload_doc(ix, window, cx),
                Some(stamp) => {
                    // Unsaved edits. If the disk now holds exactly what is in the
                    // editor there is no conflict, only nothing left to save.
                    let on_disk = std::fs::read(&path)
                        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
                    let doc = &mut self.docs[ix];
                    doc.disk = Some(stamp);
                    match on_disk {
                        Ok(text) if text == doc.text.as_ref() => doc.saved = doc.text.clone(),
                        _ => doc.notice = Some(Notice::Changed),
                    }
                }
            }
        }

        // Sidebar: files came or went somewhere it lists.
        let listed = self
            .tree
            .root
            .as_deref()
            .is_some_and(|root| paths.iter().any(|path| watch::is_listed(path, root)));
        if listed {
            self.refresh_tree_soon(cx);
        }
        cx.notify();
    }

    /// Replace a document's text with what is on disk, keeping the caret and
    /// the scroll position where they were as far as the new text allows.
    pub(super) fn reload_doc(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let doc = &mut self.docs[ix];
        let Some(path) = &doc.path else { return };
        let Ok(bytes) = std::fs::read(path) else {
            return;
        };
        let text = SharedString::from(String::from_utf8_lossy(&bytes).into_owned());
        doc.disk = watch::stamp(path);
        doc.notice = None;
        doc.saved = text.clone();
        if text == doc.text {
            return;
        }
        let editor = doc.editor.clone();
        editor.update(cx, |editor, cx| {
            let mut caret = editor.cursor_position();
            let scroll = editor.scroll_offset();
            editor.set_value(text.clone(), window, cx);
            caret.line = caret
                .line
                .min(text.lines().count().saturating_sub(1) as u32);
            editor.set_cursor_position(caret, window, cx);
            editor.set_scroll_offset(scroll, cx);
        });
        self.on_edit(&editor, cx);
    }

    /// Read the folder again shortly. Several changes in a row cause one read.
    pub(super) fn refresh_tree_soon(&mut self, cx: &mut Context<Self>) {
        self._tree_refresh = cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(250))
                .await;
            let Ok(root) = this.update(cx, |this, cx| {
                this.tree.refresh();
                // Reading the folder again dropped the row a new name is being typed in.
                this.place_draft();
                cx.notify();
                this.tree.root.clone()
            }) else {
                return;
            };
            // The palette's list of every file is rebuilt off the main thread.
            let Some(root) = root else { return };
            let listed = root.clone();
            let index = cx
                .background_executor()
                .spawn(async move { palette::index_files(&listed, 5000) })
                .await;
            this.update(cx, |this, _| {
                if this.tree.root.as_deref() == Some(root.as_path()) {
                    this.file_index = index;
                }
            })
            .ok();
        });
    }

    /// Answer a notice: `keep` keeps what is in the editor, otherwise the disk wins.
    fn resolve_notice(
        &mut self,
        id: usize,
        keep: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ix) = self.doc_ix(id) else { return };
        match (self.docs[ix].notice.take(), keep) {
            (Some(Notice::Changed), false) => {
                // Drop the edits: the reload below must not see them as unsaved.
                self.docs[ix].saved = self.docs[ix].text.clone();
                self.reload_doc(ix, window, cx);
            }
            // The file is gone: keeping the document means writing it back.
            (Some(Notice::Missing), true) => {
                self.write_doc(ix, window, cx);
            }
            (Some(Notice::Missing), false) => self.close_doc(id, window, cx),
            _ => {}
        }
        cx.notify();
    }

    /// The strip above a document that tells of a notice and offers the two ways out.
    pub(super) fn render_notice(
        &self,
        id: usize,
        notice: Notice,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let (message, keep, other) = match notice {
            Notice::Changed => (
                "This file changed on disk, and you have unsaved edits here.",
                "Keep my edits",
                "Load the new version",
            ),
            Notice::Missing => (
                "This file is no longer on disk.",
                "Save it again",
                "Close the tab",
            ),
        };
        let (hover, strong) = (p.surface_hover, p.text_primary);
        let button =
            move |label: &'static str, keep: bool, cx: &mut Context<Self>| {
                div()
                    .id(label)
                    .px(px(10.))
                    .py(px(3.))
                    .rounded(size::RADIUS_SM)
                    .border_1()
                    .border_color(p.border_strong)
                    .bg(p.surface_raised)
                    .hover(move |style| style.bg(hover).text_color(strong))
                    .child(label)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.resolve_notice(id, keep, window, cx)
                    }))
            };
        h_flex()
            .flex_none()
            .w_full()
            .gap(px(10.))
            .px(px(16.))
            .py(px(8.))
            .border_b_1()
            .border_color(p.border_subtle)
            .bg(p.status_warning_soft)
            .child(div().flex_1().child(message))
            .child(button(keep, true, cx))
            .child(button(other, false, cx))
    }
}
