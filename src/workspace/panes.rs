//! The two document panes: the source editor and the rendered preview, and
//! what clicking things in the preview does.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui_kit::base::animation::ease_out_cubic;
use gpui_kit::component::clipboard::Clipboard;
use gpui_kit::component::input::{Editor, Position};
use gpui_kit::component::text::TextView;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::reader::{PAGE_BOTTOM, PAGE_TOP};
use super::{Doc, Mode, ViewMode, Workspace};
use crate::theme::{ActivePalette as _, Palette, size};
use crate::{doc, files, motion, preview};

/// Widest the text column of the preview gets, for comfortable line lengths.
const PREVIEW_COLUMN: Pixels = px(760.);
/// Clear space above the preview text.
const PREVIEW_TOP: Pixels = px(28.);
/// Narrowest a pane's text is laid out. A pane sliding shut is clipped below
/// this instead of re-wrapping its text into a sliver on every frame.
const PANE_MIN: Pixels = px(280.);
/// Past this many bytes a document counts as long: slow enough to wrap that
/// doing it on every frame of an animation would make the animation stutter.
const LONG_DOCUMENT: usize = 30_000;
/// How long the editor takes to glide to a line it was asked to jump to.
const GLIDE: Duration = Duration::from_millis(260);

/// The editor scrolling smoothly to a place, rather than cutting to it.
pub(super) struct Glide {
    doc: usize,
    from: Pixels,
    to: Pixels,
    started: Instant,
}

/// What dragging the divider between the editor and the preview carries.
pub(super) struct DragSplit;

impl Workspace {
    /// Put the caret on a source line and glide there.
    pub(super) fn jump_to_line(&mut self, line: u32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(doc) = self.active_doc() else { return };
        let id = doc.id;
        let glide = doc.editor.update(cx, |editor, cx| {
            let from = editor.scroll_offset();
            // One line of context above the target, when there is one.
            let to = editor.line_height().map(|line_height| {
                let row = editor
                    .display_map()
                    .buffer_line_to_display_row(line as usize);
                -(line_height * row.saturating_sub(1) as f32)
            });
            editor.set_cursor_position(Position::new(line, 0), window, cx);
            editor.focus(window, cx);
            let to = to.filter(|_| !cx.reduce_motion())?;
            // Moving the caret jumps the view to it; start from where we were instead.
            editor.set_scroll_offset(from, cx);
            Some(Glide {
                doc: id,
                from: from.y,
                to,
                started: Instant::now(),
            })
        });
        self.glide = glide;
        cx.notify();
    }

    /// Move a glide in progress one frame on.
    fn advance_glide(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(glide) = &self.glide else { return };
        let Some(doc) = self.active_doc().filter(|doc| doc.id == glide.doc) else {
            self.glide = None;
            return;
        };
        let progress = (glide.started.elapsed().as_secs_f32() / GLIDE.as_secs_f32()).min(1.);
        let y = glide.from + (glide.to - glide.from) * ease_out_cubic(progress);
        doc.editor.update(cx, |editor, cx| {
            let x = editor.scroll_offset().x;
            editor.set_scroll_offset(point(x, y), cx);
        });
        if progress >= 1. {
            self.glide = None;
        } else {
            window.request_animation_frame();
        }
    }

    /// Keep the two panes on the same part of the document.
    pub(super) fn sync_scroll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.advance_glide(window, cx);
        // After a change of mode the two are lined up once, whatever is on show.
        let carrying = self.carry > 0;
        self.carry = self.carry.saturating_sub(1);
        let split = self.mode == Mode::Edit && self.view == ViewMode::Split;
        if !(carrying || (self.sync_enabled && split)) {
            return;
        }
        let Some(doc) = self.docs.get_mut(self.active) else {
            return;
        };
        if doc.sync.update(&doc.editor, &doc.preview, &doc.blocks, cx) || carrying {
            window.request_animation_frame();
        }
    }

    /// The editor and the preview, side by side in `width`. The editor takes
    /// `share` of it: 1 in editor view, 0 in preview view, and on its way
    /// between the two while the view is changing.
    ///
    /// `target` is the width and share they are heading for.
    pub(super) fn render_panes(
        &mut self,
        width: Pixels,
        share: f32,
        target: (Pixels, f32),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = *cx.palette();
        let editor_width = width * share;
        let preview_width = width - editor_width;
        let (show_editor, show_preview) = (share > 0.001, share < 0.999);

        // The widths the text is wrapped to. For most documents that is simply
        // the width of the pane, every frame. Wrapping a long document again
        // takes several frames' worth of time, so while its panes are moving
        // the text is held as it was and wrapped once, on arrival.
        let moving = (width, share) != target || cx.has_active_drag();
        let long = self
            .active_doc()
            .is_some_and(|doc| doc.text.len() > LONG_DOCUMENT);
        let (editor_text, preview_text) = if moving && long {
            let held =
                |held: Pixels, arriving: Pixels| if held >= PANE_MIN { held } else { arriving };
            let arriving = target.0 * target.1;
            (
                held(self.settled.0, arriving),
                held(self.settled.1, target.0 - arriving),
            )
        } else {
            (editor_width, preview_width)
        };
        if !moving {
            self.settled = (editor_width, preview_width);
        }
        let Some(doc) = self.active_doc() else {
            return div().into_any_element();
        };

        let editor = show_editor.then(|| {
            div()
                .flex_none()
                .h_full()
                .w(editor_width)
                .overflow_hidden()
                .bg(p.surface_editor)
                .child(
                    div().h_full().w(editor_text.max(PANE_MIN)).child(
                        Editor::new(&doc.editor)
                            .appearance(false)
                            .bordered(false)
                            .h_full()
                            .line_height(relative(1.6))
                            // What is typed is what is shown: `->` stays two characters.
                            .font_features(FontFeatures::disable_ligatures()),
                    ),
                )
        });
        let preview = show_preview.then(|| {
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .overflow_hidden()
                .when(show_editor, |pane| {
                    pane.border_l_1().border_color(p.border_subtle)
                })
                .child(
                    div()
                        .h_full()
                        .w(preview_text.max(PANE_MIN))
                        .child(self.render_preview(doc, preview_text.max(PANE_MIN), None, &p, cx)),
                )
        });
        // The divider can be dragged while both panes are fully open.
        let divider = (self.view == ViewMode::Split && (share - self.split).abs() < 0.001)
            .then(|| motion::resize_handle("split-handle", editor_width, DragSplit, &p));

        let notice = doc
            .notice
            .map(|notice| self.render_notice(doc.id, notice, &p, cx));
        div()
            .flex()
            .flex_col()
            .size_full()
            .children(notice)
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .children(editor)
                    .children(preview)
                    .children(divider),
            )
            .into_any_element()
    }

    /// The rendered document, `width` wide. As a page it is the whole window:
    /// its text fills a `sheet` of paper in the middle, with margins at the
    /// sides, and starts below the menu that floats over its top.
    pub(super) fn render_preview(
        &self,
        doc: &Doc,
        width: Pixels,
        sheet: Option<Pixels>,
        p: &Palette,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let list = doc.preview.read(cx).list_state().clone();
        let (margin, clear, foot) = match sheet {
            Some(sheet) => (((width - sheet) / 2.).max(px(24.)), PAGE_TOP, PAGE_BOTTOM),
            // Beside the editor the status bar is under the text, not the window's edge.
            None => (
                ((width - PREVIEW_COLUMN) / 2.).max(px(40.)),
                PREVIEW_TOP,
                px(0.),
            ),
        };
        let top = list.logical_scroll_top();
        let scrolled = top.item_ix > 0 || top.offset_in_item > px(0.);
        // More text waits below, unless the list knows it is at its end.
        let more_below = foot > px(0.) && list.is_scrolled_to_end() != Some(true);
        let see_through = p.surface_preview.opacity(0.);
        let (id, folder) = (
            doc.id,
            doc.path
                .as_deref()
                .and_then(Path::parent)
                .map(Path::to_path_buf),
        );
        let (on_task, on_link) = (cx.weak_entity(), cx.weak_entity());

        let text = TextView::new(&doc.preview)
            .scrollable(true)
            .selectable(true)
            .style(preview::style(p))
            .markdown_extensions(preview::extensions())
            .image_source(move |uri| preview::image_source(uri, folder.as_deref()))
            .code_block_actions(|code, _, _| {
                // The block's place in the source tells its copy button apart from the others.
                let at = code.span.map_or(0, |span| span.start);
                Clipboard::new(("copy-code", at)).value(code.code())
            })
            .on_task_toggle(move |event, window, cx| {
                on_task
                    .update(cx, |this, cx| {
                        this.toggle_task(id, event.marker_range(), event.checked(), window, cx)
                    })
                    .ok();
            })
            .on_link_click(move |url, _, window, cx| {
                on_link
                    .update(cx, |this, cx| this.follow_link(url, window, cx))
                    .ok();
            })
            .text_size(size::FONT_BODY)
            .line_height(relative(1.65))
            .px(margin)
            .pt(clear)
            .pb(foot);

        div()
            .id(("preview", doc.id))
            .relative()
            .size_full()
            .bg(p.surface_preview)
            // The margins beside the text column scroll the document too.
            .on_scroll({
                let list = list.clone();
                move |event, window, _| {
                    if !list.viewport_bounds().contains(&event.position) {
                        list.scroll_by(-event.delta.pixel_delta(px(20.)).y);
                        window.refresh();
                    }
                }
            })
            .child(text)
            // Text scrolling out at the top fades away instead of being cut off.
            .when(scrolled, |pane| {
                pane.child(
                    div()
                        .absolute()
                        .top(clear)
                        .left_0()
                        .right(px(12.))
                        .h(px(20.))
                        .bg(linear_gradient(
                            180.,
                            linear_color_stop(p.surface_preview, 0.),
                            linear_color_stop(see_through, 1.),
                        )),
                )
            })
            // And so does text scrolling out at the foot of a page.
            .when(more_below, |pane| {
                pane.child(
                    div()
                        .absolute()
                        .bottom(foot)
                        .left_0()
                        .right(px(12.))
                        .h(px(20.))
                        .bg(linear_gradient(
                            180.,
                            linear_color_stop(see_through, 0.),
                            linear_color_stop(p.surface_preview, 1.),
                        )),
                )
            })
    }

    /// A task checkbox was clicked in the preview: write the new state into the source.
    pub(super) fn toggle_task(
        &mut self,
        id: usize,
        marker: Range<usize>,
        checked: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(doc) = self.doc_ix(id).map(|ix| &self.docs[ix]) else {
            return;
        };
        // The preview can be a keystroke behind the editor: only ever replace a real task marker.
        if !matches!(doc.text.get(marker.clone()), Some("[ ]" | "[x]" | "[X]")) {
            return;
        }
        doc.editor.update(cx, |editor, cx| {
            // Both markers are three bytes, so the caret can go back exactly where it was.
            let selection = editor.selected_range();
            editor.set_selected_range(marker, cx);
            editor.replace(if checked { "[x]" } else { "[ ]" }, window, cx);
            editor.set_selected_range(selection, cx);
        });
    }

    /// A link was clicked in the preview.
    pub(super) fn follow_link(&mut self, url: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(anchor) = url.strip_prefix('#') {
            self.jump_to_anchor(anchor, window, cx);
            return;
        }
        // Anything with a scheme (https:, mailto:, obsidian:) is for the system to open.
        let has_scheme = url.split_once(':').is_some_and(|(scheme, _)| {
            scheme.len() > 1
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        });
        if has_scheme {
            cx.open_url(url);
            return;
        }

        // Otherwise it is a path, relative to the document it is written in.
        let (path, anchor) = url.split_once('#').unwrap_or((url, ""));
        let path = PathBuf::from(crate::open::percent_decode(path));
        let folder = self
            .active_doc()
            .and_then(|doc| doc.path.as_deref())
            .and_then(Path::parent);
        let target = match (path.is_absolute(), folder) {
            (false, Some(folder)) => folder.join(path),
            _ => path,
        };
        if files::is_markdown(&target) && target.is_file() {
            self.open(target, window, cx);
            if !anchor.is_empty() {
                self.jump_to_anchor(anchor, window, cx);
            }
        } else if target.exists() {
            cx.open_with_system(&target);
        }
    }

    /// Go to the heading a `#anchor` link names.
    fn jump_to_anchor(&mut self, anchor: &str, window: &mut Window, cx: &mut Context<Self>) {
        let anchor = crate::open::percent_decode(anchor).to_lowercase();
        let line = self
            .active_doc()
            .and_then(|doc| {
                doc.headings
                    .iter()
                    .find(|heading| doc::anchor(&heading.title) == anchor)
            })
            .map(|heading| heading.line);
        if let Some(line) = line {
            self.jump_to_line(line, window, cx);
        }
    }
}
