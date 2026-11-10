//! The sidebar: jump field, the folder's Markdown files, and the outline of the
//! open document.
//!
//! The file list draws only the rows in view, so a folder of thousands stays
//! cheap, and places each row itself. That is what lets a folder slide open:
//! its rows come out from under it while the rows below move down.

use std::f32::consts::FRAC_PI_2;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::time::Instant;

use gpui_kit::assets::IconName;
use gpui_kit::base::transition;
use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::component::{Icon, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::hover::Hot;
use super::{
    CopyFilePath, NewFileHere, NewFolderHere, RenameFile, RevealFile, TrashFile, Workspace,
};
use crate::files::Row;
use crate::motion;
use crate::theme::{ActivePalette as _, Palette, size};
use crate::ui::{icon_button, section_label};

const ROW_HEIGHT: Pixels = px(28.);
const OUTLINE_ROW_HEIGHT: Pixels = px(26.);
/// Space on each side of the rows, so selection highlights float inside the sidebar.
const GUTTER: Pixels = px(10.);

/// A folder in the middle of opening or closing.
pub(super) struct TreeMotion {
    /// Row of the folder.
    at: usize,
    /// The rows that are appearing or leaving. When leaving they are no longer
    /// in the tree, so they are kept here to be drawn on their way out.
    rows: Vec<Row>,
    opening: bool,
    started: Instant,
}

impl Workspace {
    /// Open or close the folder at `ix`, with its rows sliding out or away.
    pub(super) fn toggle_folder(&mut self, ix: usize, cx: &mut Context<Self>) {
        let leaving = self.tree.rows[self.tree.descendants(ix)].to_vec();
        self.tree.toggle(ix);
        let (opening, rows) = match leaving.is_empty() {
            true => (true, self.tree.rows[self.tree.descendants(ix)].to_vec()),
            false => (false, leaving),
        };
        self.tree_motion = (!rows.is_empty()).then(|| TreeMotion {
            at: ix,
            rows,
            opening,
            started: Instant::now(),
        });
        cx.notify();
    }

    pub(super) fn render_sidebar(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let p = *cx.palette();
        let files = self.render_files(&p, window, cx);
        let jump = self
            .render_jump_field(&p)
            .on_click(cx.listener(|this, _, window, cx| this.open_palette(window, cx)));
        let new_file = self
            .hoverable(
                icon_button("new-file", IconName::Plus, &p),
                Hot::Button("new-file"),
                p.surface_hover,
                window,
                cx,
            )
            .size(px(22.))
            .on_click(cx.listener(|this, _, window, cx| {
                // The plus makes a file in the folder on show, not under some earlier right-click.
                this.context_row = None;
                this.new_file_here(window, cx);
            }));

        v_flex()
            .size_full()
            .child(jump)
            .child(
                h_flex()
                    .flex_none()
                    .justify_between()
                    .h(px(28.))
                    .mt(px(10.))
                    .pl(px(18.))
                    .pr(px(8.))
                    .child(section_label("FILES", &p))
                    .child(new_file),
            )
            .children(self.render_name_field(&p, cx))
            .child(files)
            .child(self.render_outline(&p, window, cx))
    }

    /// Looks like a field; opens the command palette.
    fn render_jump_field(&self, p: &Palette) -> Stateful<Div> {
        h_flex()
            .id("jump")
            .flex_none()
            .h(px(28.))
            .mx(GUTTER)
            .mt(px(12.))
            .px(px(8.))
            .gap(px(8.))
            .rounded(size::RADIUS_SM)
            .border_1()
            .border_color(p.border_subtle)
            .bg(p.surface_raised)
            .text_color(p.text_tertiary)
            .child(Icon::new(IconName::Search).size(px(14.)))
            .child(div().flex_1().truncate().child("Search or jump to"))
            .child(
                div()
                    .px(px(5.))
                    .rounded(size::RADIUS_XS)
                    .bg(p.surface_sunken)
                    .text_size(size::FONT_UI_SMALL)
                    .child("⌘K"),
            )
    }

    fn render_files(
        &mut self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.tree.rows.is_empty() {
            return self.render_no_files(p, cx);
        }

        // The folder in motion, if any: its row, how many rows are moving,
        // which way, and how far along it is.
        let motion = self.tree_motion.as_ref().map(|motion| {
            let done = motion::progress(motion.started, motion::QUICK, cx);
            (motion.at, motion.rows.len(), motion.opening, done)
        });
        let motion = match motion {
            Some((.., done)) if done >= 1. => {
                self.tree_motion = None;
                None
            }
            Some(motion) => {
                window.request_animation_frame();
                Some(motion)
            }
            None => None,
        };
        let count = self.tree.rows.len();
        let block = motion.map_or(px(0.), |(_, rows, ..)| ROW_HEIGHT * rows as f32);
        // How far the rows after the folder are from where they will rest.
        let offset = match motion {
            Some((.., true, done)) => -block * (1. - done),
            Some((.., false, done)) => block * (1. - done),
            None => px(0.),
        };
        // While opening, the moving rows are already in the tree; while closing, they have left it.
        let (steady_end, first_after) = match motion {
            Some((at, rows, true, _)) => (at + 1, (at + 1 + rows).min(count)),
            Some((at, _, false, _)) => (at + 1, at + 1),
            None => (count, count),
        };

        // Only rows in view are built. Before the first layout the size of the
        // view is unknown; a screenful is a safe guess.
        let scrolled = -self.tree_scroll.offset().y;
        let viewport = match self.tree_scroll.bounds().size.height {
            height if height > px(0.) => height,
            _ => px(1200.),
        };
        // The rows of `within` that are in view when the first of them is drawn at `top`.
        let in_view = |top: Pixels, within: Range<usize>| {
            let first = ((scrolled - top) / ROW_HEIGHT).floor().max(0.) as usize;
            let last = ((scrolled + viewport - top) / ROW_HEIGHT).ceil().max(0.) as usize + 1;
            within.start + first.min(within.len())..within.start + last.min(within.len())
        };
        let selected = self
            .active_doc()
            .and_then(|doc| doc.path.as_deref())
            .and_then(|path| {
                self.tree
                    .rows
                    .iter()
                    .position(|row| !row.is_dir && row.path == path)
            });
        let place = |top: Pixels, row: Div| div().absolute().left_0().right_0().top(top).child(row);

        let mut rows = Vec::new();
        // Rows that are where they belong: all of them, or those down to the folder.
        for ix in in_view(px(0.), 0..steady_end) {
            let row = self.render_file_row(
                &self.tree.rows[ix],
                Some(ix),
                selected == Some(ix),
                p,
                window,
                cx,
            );
            rows.push(place(ROW_HEIGHT * ix as f32, row).into_any_element());
        }
        if let Some((at, row_count, opening, done)) = motion {
            // Rows below the folder ride down or up with it.
            let after_top = ROW_HEIGHT * first_after as f32 + offset;
            for ix in in_view(after_top, first_after..count) {
                let row = self.render_file_row(
                    &self.tree.rows[ix],
                    Some(ix),
                    selected == Some(ix),
                    p,
                    window,
                    cx,
                );
                rows.push(place(ROW_HEIGHT * ix as f32 + offset, row).into_any_element());
            }
            // The folder's own rows are seen through a window under the folder
            // that grows or shrinks, so they come out from beneath it.
            let revealed = if opening {
                block * done
            } else {
                block * (1. - done)
            };
            let window_top = ROW_HEIGHT * (at + 1) as f32;
            let slide = revealed - block;
            let mut children = Vec::new();
            for child in in_view(window_top + slide, 0..row_count) {
                let ix = opening.then_some(at + 1 + child);
                let row = match ix {
                    Some(ix) => self.tree.rows[ix].clone(),
                    None => self
                        .tree_motion
                        .as_ref()
                        .map(|motion| motion.rows[child].clone())
                        .unwrap(),
                };
                let row =
                    self.render_file_row(&row, ix, ix.is_some() && selected == ix, p, window, cx);
                children.push(place(ROW_HEIGHT * child as f32 + slide, row));
            }
            rows.push(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(window_top)
                    .h(revealed)
                    .overflow_hidden()
                    .children(children)
                    .into_any_element(),
            );
        }

        // The selection is one piece that glides from file to file, drawn under the rows.
        let highlight = selected.map(|ix| {
            let top = transition(
                "file-highlight",
                ROW_HEIGHT * ix as f32,
                motion::glide(motion::QUICK, cx),
                window,
                cx,
            );
            div()
                .absolute()
                .top(top)
                .left(GUTTER)
                .right(GUTTER)
                .h(ROW_HEIGHT - px(1.))
                .rounded(size::RADIUS_SM)
                .bg(p.surface_selected)
        });

        let height = ROW_HEIGHT * count as f32 + offset;
        div()
            .id("files")
            // As tall as its rows, giving way when the sidebar runs out of room.
            .h(height)
            .min_h(ROW_HEIGHT * count.min(3) as f32)
            .overflow_y_scroll()
            .track_scroll(&self.tree_scroll)
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(height)
                    .children(highlight)
                    .children(rows),
            )
            .into_any_element()
    }

    fn render_no_files(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let hint = if self.tree.root.is_some() {
            "No Markdown files here"
        } else {
            "No folder open"
        };
        let (hover, strong) = (p.surface_hover, p.text_primary);
        v_flex()
            .flex_none()
            .items_start()
            .gap(px(8.))
            .px(px(18.))
            .py(px(6.))
            .text_color(p.text_tertiary)
            .child(hint)
            .child(
                div()
                    .id("open-folder")
                    .px(px(10.))
                    .py(px(4.))
                    .rounded(size::RADIUS_SM)
                    .border_1()
                    .border_color(p.border_subtle)
                    .bg(p.surface_raised)
                    .text_color(p.text_secondary)
                    .hover(move |style| style.bg(hover).text_color(strong))
                    .child("Open…")
                    .on_click(cx.listener(|this, _, window, cx| this.open_dialog(window, cx))),
            )
            .into_any_element()
    }

    /// One row of the file list. `ix` is its place in the tree; a row that is
    /// sliding away has none and no longer answers clicks.
    fn render_file_row(
        &self,
        row: &Row,
        ix: Option<usize>,
        selected: bool,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        row.path.hash(&mut hasher);
        let key = hasher.finish();

        // The arrow turns to point down as the folder opens.
        let chevron = div().flex_none().size(px(12.)).when(row.is_dir, |slot| {
            let id = ElementId::from(("chevron", key as usize));
            let turn = transition(
                id,
                if row.open { 1. } else { 0. },
                motion::glide(motion::QUICK, cx),
                window,
                cx,
            );
            let arrow = Icon::new(IconName::ChevronRight)
                .size(px(12.))
                .text_color(p.text_tertiary);
            slot.child(arrow.rotate(radians(FRAC_PI_2 * turn)))
        });
        let icon = match (row.is_dir, row.open) {
            (true, true) => IconName::FolderOpen,
            (true, false) => IconName::Folder,
            _ => IconName::FileText,
        };

        let item = h_flex()
            .id(("file", key as usize))
            .h(ROW_HEIGHT - px(1.))
            .pl(px(6.) + px(16.) * row.depth as f32)
            .pr(px(8.))
            .gap(px(6.))
            .rounded(size::RADIUS_SM)
            .when(selected, |item| item.font_weight(FontWeight::MEDIUM))
            .child(chevron)
            .child(Icon::new(icon).size(px(15.)).text_color(if selected {
                p.accent_hover
            } else {
                p.text_secondary
            }))
            .child(div().flex_1().min_w_0().truncate().child(row.name.clone()))
            .when_some(ix, |item, ix| {
                item.on_click(
                    cx.listener(move |this, _, window, cx| this.on_tree_click(ix, window, cx)),
                )
            });
        // The selected row already has its highlight; the others answer the pointer.
        let item = if selected {
            item
        } else {
            self.hoverable(item, Hot::File(key), p.surface_hover, window, cx)
        };
        // A right-click offers what can be done with this row.
        let path = row.path.clone();
        let item = item
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, _, _, _| this.context_row = Some(path.clone())),
            )
            .context_menu(|menu, _, _| {
                menu.menu("New File", Box::new(NewFileHere))
                    .menu("New Folder", Box::new(NewFolderHere))
                    .separator()
                    .menu("Rename…", Box::new(RenameFile))
                    .menu("Move to Trash", Box::new(TrashFile))
                    .separator()
                    .menu("Show in Finder", Box::new(RevealFile))
                    .menu("Copy Path", Box::new(CopyFilePath))
            });
        div().h(ROW_HEIGHT).px(GUTTER).child(item)
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
                .ml(px(19.))
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
            .left(px(18.))
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
                .left(px(18.))
                .w(px(2.))
                .h(OUTLINE_ROW_HEIGHT)
                .bg(p.accent)
        });

        div()
            .flex_none()
            .max_h(relative(0.5))
            .flex()
            .flex_col()
            .child(
                h_flex()
                    .flex_none()
                    .h(px(28.))
                    .mt(px(10.))
                    .pl(px(18.))
                    .child(section_label("OUTLINE", p)),
            )
            .child(
                div()
                    .id("outline")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .pb(px(10.))
                    .child(div().relative().child(rule).children(marker).children(rows)),
            )
    }
}
