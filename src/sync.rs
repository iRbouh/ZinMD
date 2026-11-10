//! Scroll sync between the editor and the preview.
//!
//! Both panes are compared with where they were on the previous frame. The one
//! the user moved becomes the leader and the other is scrolled to show the same
//! source line at its top. While a pane leads, movement of the other is ours and
//! is ignored, so the two never chase each other.

use std::time::{Duration, Instant};

use gpui_kit::component::input::EditorState;
use gpui_kit::component::text::TextViewState;
use gpui_kit::*;

use crate::preview::BlockMap;

/// How long a pane stays leader after it last moved. Covers the frame or two
/// the follower needs to settle, and momentum scrolling between wheel events.
const LEAD: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Editor,
    Preview,
}

#[derive(Default)]
pub struct ScrollSync {
    editor_y: Pixels,
    preview_top: (usize, Pixels),
    leader: Option<(Side, Instant)>,
    /// A pane to copy from on the next frame, whether it moved or not.
    pending: Option<Side>,
    /// Frames spent on `pending` without it settling.
    attempts: u8,
}

impl ScrollSync {
    /// Line the other pane up with `side` on the next frame. Use after the
    /// layout changed under the panes (a pane was shown, the document reloaded).
    pub fn follow(&mut self, side: Side) {
        self.pending = Some(side);
        self.attempts = 0;
    }

    /// Call once a frame while both panes are shown. Returns whether it needs
    /// to run again next frame to finish lining the panes up.
    pub fn update(
        &mut self,
        editor: &Entity<EditorState>,
        preview: &Entity<TextViewState>,
        blocks: &BlockMap,
        cx: &mut App,
    ) -> bool {
        let list = preview.read(cx).list_state().clone();
        let editor_y = editor.read(cx).scroll_offset().y;
        let top = list.logical_scroll_top();
        let preview_top = (top.item_ix, top.offset_in_item);

        // The map is rebuilt in the background after an edit; until it matches
        // the rows on screen again there is nothing sound to map through.
        let ready = blocks.len() > 0 && blocks.len() == list.item_count();
        if ready {
            let now = Instant::now();
            let leader = self
                .leader
                .filter(|(_, at)| now.duration_since(*at) < LEAD)
                .map(|(side, _)| side);
            let editor_moved = editor_y != self.editor_y && leader != Some(Side::Preview);
            let preview_moved = preview_top != self.preview_top && leader != Some(Side::Editor);

            let from = match self.pending {
                Some(side) => Some(side),
                None if editor_moved => Some(Side::Editor),
                None if preview_moved => Some(Side::Preview),
                None => None,
            };
            if let Some(side) = from {
                let settled = match side {
                    Side::Editor => editor_top_line(editor.read(cx))
                        .is_some_and(|line| scroll_preview_to(&list, blocks, line)),
                    Side::Preview => preview_top_line(&list, blocks)
                        .is_some_and(|line| scroll_editor_to(editor, line, cx)),
                };
                self.leader = Some((side, now));
                // Not settled means a pane was not measured yet; a few frames always suffice.
                self.attempts = if settled { 0 } else { self.attempts + 1 };
                self.pending = (!settled && self.attempts < 4).then_some(side);
            }
        }

        self.editor_y = editor_y;
        let top = list.logical_scroll_top();
        self.preview_top = (top.item_ix, top.offset_in_item);
        self.pending.is_some()
    }
}

/// The source line at the top of the editor, with a fraction for a line that
/// is partly scrolled off. `None` before the editor has been laid out.
fn editor_top_line(editor: &EditorState) -> Option<f32> {
    let line_height = editor.line_height()?;
    let row = (-editor.scroll_offset().y / line_height).max(0.);
    let map = editor.display_map();
    let line = map.display_row_to_buffer_line(row.floor() as usize);
    // A soft-wrapped line takes several rows; measure through all of them.
    let rows = map.buffer_line_to_display_row_range(line)?;
    let through = (row - rows.start as f32) / rows.len().max(1) as f32;
    Some(line as f32 + through.clamp(0., 0.999))
}

/// Returns whether the editor could be placed (it can once it has been laid out).
fn scroll_editor_to(editor: &Entity<EditorState>, line: f32, cx: &mut App) -> bool {
    editor.update(cx, |editor, cx| {
        let Some(line_height) = editor.line_height() else {
            return false;
        };
        let map = editor.display_map();
        let last = map.buffer_line_count().saturating_sub(1);
        let Some(rows) = map.buffer_line_to_display_row_range((line.floor() as usize).min(last))
        else {
            return false;
        };
        let row = rows.start as f32 + line.fract() * rows.len() as f32;
        let x = editor.scroll_offset().x;
        editor.set_scroll_offset(point(x, -(line_height * row)), cx);
        true
    })
}

/// The source line at the top of the preview. `None` until that row is measured.
fn preview_top_line(list: &ListState, blocks: &BlockMap) -> Option<f32> {
    let top = list.logical_scroll_top();
    let height = list.bounds_for_item(top.item_ix)?.size.height;
    let through = if height > px(0.) {
        top.offset_in_item / height
    } else {
        0.
    };
    blocks.line_at(top.item_ix, through)
}

/// Returns whether the preview was placed exactly. A row that has never been
/// on screen has no height yet: its top is shown, and once it is measured the
/// next call places it exactly.
fn scroll_preview_to(list: &ListState, blocks: &BlockMap, line: f32) -> bool {
    let Some((ix, through)) = blocks.locate(line) else {
        return true;
    };
    let height = list.bounds_for_item(ix).map(|bounds| bounds.size.height);
    list.scroll_to(ListOffset {
        item_ix: ix,
        offset_in_item: height.unwrap_or_default() * through,
    });
    height.is_some() || through == 0.
}
