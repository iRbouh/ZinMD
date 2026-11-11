//! Going back to where you were, and on to the next file.
//!
//! Following a link, choosing a heading or what a search found, opening
//! another file: each leaves the place it took you from on a trail. The
//! command key with `[` goes back along it and with `]` forward again, as in
//! a browser. Scrolling leaves nothing: back is to where you last set off
//! from.
//!
//! The files beside the one on show, in the order the panel lists them, are
//! its previous and next: one key each, and two quiet links at the foot of
//! the page when its end is reached.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui_kit::assets::IconName;
use gpui_kit::base::transition;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{Mode, Workspace};
use crate::theme::{Palette, size};
use crate::{files, motion};

/// How many places the trail keeps.
const KEPT: usize = 100;

/// Somewhere the reader was: a document, and how far down it.
#[derive(Clone, Debug)]
struct Place {
    doc: usize,
    path: Option<PathBuf>,
    /// The top of the page, as its list had it.
    top: ListOffset,
    /// The line of the source there: the caret's in the editor. It is what
    /// is left to go by once the document has been closed.
    line: u32,
}

#[derive(Default)]
pub(super) struct Trail {
    back: Vec<Place>,
    ahead: Vec<Place>,
    /// Nothing more is left on the trail until the next frame: one move
    /// leaves one place, however many steps it is made of.
    held: bool,
    /// The Markdown files of a folder that is not in the panel, and when
    /// they were read from disk.
    beside: Option<(PathBuf, Instant, Vec<PathBuf>)>,
}

/// How long what was read of a folder that is not in the panel is good for:
/// long enough that typing does not read it at every key.
const BESIDE_KEPT: Duration = Duration::from_secs(2);

impl Trail {
    /// Called as each frame is drawn: the move that was being made is over.
    pub(super) fn settle(&mut self) {
        self.held = false;
    }
}

/// What the keys for the previous and next file are called.
const PREVIOUS_KEY: &str = match cfg!(target_os = "macos") {
    true => "⇧⌘[",
    false => "Ctrl+Shift+[",
};
const NEXT_KEY: &str = match cfg!(target_os = "macos") {
    true => "⇧⌘]",
    false => "Ctrl+Shift+]",
};

impl Workspace {
    /// Where the reader is.
    fn place_now(&self, cx: &App) -> Option<Place> {
        let doc = self.active_doc().filter(|doc| !doc.blank())?;
        let list = doc.preview.read(cx).list_state().clone();
        let line = match self.mode {
            Mode::Edit => doc.editor.read(cx).cursor_position().line,
            Mode::Read => crate::sync::preview_top_line(&list, &doc.blocks).unwrap_or(0.) as u32,
        };
        Some(Place {
            doc: doc.id,
            path: doc.path.clone(),
            top: list.logical_scroll_top(),
            line,
        })
    }

    /// Leave the place the reader is at on the trail. Call before taking
    /// them somewhere else.
    pub(super) fn leave_place(&mut self, cx: &App) {
        if self.trail.held {
            return;
        }
        let Some(here) = self.place_now(cx) else {
            return;
        };
        self.trail.held = true;
        // Setting off twice from one place leaves it once.
        let same = |last: &Place| {
            last.doc == here.doc
                && last.top.item_ix == here.top.item_ix
                && last.top.offset_in_item == here.top.offset_in_item
                && last.line == here.line
        };
        if !self.trail.back.last().is_some_and(same) {
            self.trail.back.push(here);
            let extra = self.trail.back.len().saturating_sub(KEPT);
            self.trail.back.drain(..extra);
        }
        // What was ahead was ahead of somewhere else.
        self.trail.ahead.clear();
    }

    /// Whether there is somewhere to go back to, or with `forward`, on to.
    pub(super) fn can_go(&self, forward: bool) -> bool {
        match forward {
            true => !self.trail.ahead.is_empty(),
            false => !self.trail.back.is_empty(),
        }
    }

    /// Go back along the trail, or with `forward`, on again.
    pub(super) fn go_along(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let trail = &mut self.trail;
        let Some(place) = (match forward {
            true => trail.ahead.pop(),
            false => trail.back.pop(),
        }) else {
            return;
        };
        // Where this was set off from is the other way from now on. Nothing
        // on the way there is left on the trail.
        let here = self.place_now(cx);
        self.trail.held = true;
        if let Some(here) = here {
            match forward {
                true => self.trail.back.push(here),
                false => self.trail.ahead.push(here),
            }
        }
        let open = self.doc_ix(place.doc).or_else(|| {
            let path = place.path.as_deref()?;
            self.docs
                .iter()
                .position(|doc| doc.path.as_deref() == Some(path))
        });
        match (open, place.path) {
            (Some(ix), _) => {
                if ix != self.active {
                    self.activate(ix, window, cx);
                }
                match self.mode {
                    Mode::Read => self.move_page_to(place.top, cx),
                    Mode::Edit => self.jump_to_line(place.line, window, cx),
                }
            }
            // It was closed since: open it again, at the line.
            (None, Some(path)) if path.is_file() => {
                self.open(path, window, cx);
                self.go_to_line(place.line, window, cx);
            }
            // It is gone, or was never saved: on to the place before it.
            (None, _) => {
                self.trail.held = false;
                match forward {
                    true => self.trail.back.pop(),
                    false => self.trail.ahead.pop(),
                };
                self.go_along(forward, window, cx);
            }
        }
        cx.notify();
    }

    // ------------------------------------------------------- files beside

    /// The Markdown files in the folder `file` is in, in the order the panel
    /// lists them. Those of a folder of the panel are known; any other
    /// folder is read from disk, and what was read is kept a moment.
    pub(super) fn beside(&mut self, file: &Path) -> Vec<PathBuf> {
        if let Some(folder) = self.folder_of(file) {
            return files::beside(file, Some(&self.folders[folder].files));
        }
        let Some(dir) = file.parent() else {
            return Vec::new();
        };
        match &self.trail.beside {
            Some((read, when, beside)) if read == dir && when.elapsed() < BESIDE_KEPT => {
                beside.clone()
            }
            _ => {
                let beside = files::beside(file, None);
                self.trail.beside = Some((dir.to_path_buf(), Instant::now(), beside.clone()));
                beside
            }
        }
    }

    /// The file before the one on show and the one after, among the Markdown
    /// files of its folder.
    pub(super) fn files_beside(&mut self) -> (Option<PathBuf>, Option<PathBuf>) {
        let Some(path) = self.active_doc().and_then(|doc| doc.path.clone()) else {
            return (None, None);
        };
        let beside = self.beside(&path);
        let Some(at) = beside.iter().position(|file| *file == path) else {
            return (None, None);
        };
        let before = at.checked_sub(1).map(|at| beside[at].clone());
        (before, beside.get(at + 1).cloned())
    }

    /// Open the file before the one on show, or with `next`, the one after.
    pub(super) fn turn_to_file(&mut self, next: bool, window: &mut Window, cx: &mut Context<Self>) {
        let (before, after) = self.files_beside();
        if let Some(path) = if next { after } else { before } {
            self.open(path, window, cx);
        }
    }

    /// The links to the previous and next file, at the foot of a page whose
    /// end has been reached.
    pub(super) fn render_page_foot(
        &mut self,
        margin: Pixels,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let at_end = self.active_doc().is_some_and(|doc| {
            let list = doc.preview.read(cx).list_state();
            list.item_count() > 0 && list.is_scrolled_to_end() == Some(true)
        });
        let presence = transition(
            "page-foot",
            if at_end { 1. } else { 0. },
            motion::glide(motion::QUICK, cx),
            window,
            cx,
        );
        if presence <= 0.001 {
            return None;
        }
        let (before, after) = self.files_beside();
        if before.is_none() && after.is_none() {
            return None;
        }
        let link = |next: bool, path: &Path| {
            let name = SharedString::from(files::display_name(path));
            let (id, icon, says) = match next {
                true => ("next-file", IconName::ArrowRight, NEXT_KEY),
                false => ("previous-file", IconName::ArrowLeft, PREVIOUS_KEY),
            };
            let arrow = Icon::new(icon).size(px(14.)).flex_none();
            // A long name is cut short, so that the two links never meet.
            let name = div().max_w(px(260.)).truncate().child(name);
            h_flex()
                .id(id)
                .flex_none()
                .h(px(28.))
                .px(px(8.))
                .gap(px(7.))
                .rounded(size::RADIUS_MD)
                .text_size(size::FONT_UI)
                .text_color(p.text_tertiary)
                .hover(|style| style.text_color(p.text_primary).bg(p.surface_hover))
                .active(|style| style.opacity(0.7))
                .map(|link| match next {
                    true => link.child(name).child(arrow),
                    false => link.child(arrow).child(name),
                })
                .tooltip(move |window, cx| {
                    let tip = match next {
                        true => format!("Next file  {says}"),
                        false => format!("Previous file  {says}"),
                    };
                    Tooltip::new(tip).build(window, cx)
                })
                .on_click(
                    cx.listener(move |this, _, window, cx| this.turn_to_file(next, window, cx)),
                )
        };
        Some(
            h_flex()
                .absolute()
                .bottom(px(10.))
                .left(margin - px(8.))
                .right(margin - px(8.))
                .justify_between()
                .opacity(presence)
                .child(div().children(before.as_deref().map(|path| link(false, path))))
                .child(div().children(after.as_deref().map(|path| link(true, path)))),
        )
    }
}
