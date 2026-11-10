//! The two document panes: the source editor and the rendered preview, and
//! what clicking things in the preview does.

use std::ops::Range;
use std::path::{Path, PathBuf};

use gpui_kit::component::clipboard::Clipboard;
use gpui_kit::component::input::Editor;
use gpui_kit::component::resizable::{h_resizable, resizable_panel};
use gpui_kit::component::text::TextView;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{Doc, ViewMode, Workspace};
use crate::theme::{ActivePalette as _, Palette, size};
use crate::{doc, files, preview};

/// Widest the text column of the preview gets, for comfortable line lengths.
const PREVIEW_COLUMN: Pixels = px(760.);
/// Clear space above the preview text.
const PREVIEW_TOP: Pixels = px(28.);

impl Workspace {
    /// Keep the two panes on the same part of the document.
    pub(super) fn sync_scroll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.sync_enabled || self.view != ViewMode::Split {
            return;
        }
        let Some(doc) = self.docs.get_mut(self.active) else {
            return;
        };
        if doc.sync.update(&doc.editor, &doc.preview, &doc.blocks, cx) {
            window.request_animation_frame();
        }
    }

    /// How wide the preview pane is, to centre its text column.
    fn preview_width(&self, window: &Window, cx: &App) -> Pixels {
        let sidebar = match self.sidebar_open {
            true => self
                .sidebar_split
                .read(cx)
                .sizes()
                .first()
                .copied()
                .unwrap_or(size::SIDEBAR),
            false => px(0.),
        };
        let area = window.viewport_size().width - sidebar;
        match self.view {
            ViewMode::Split => self
                .pane_split
                .read(cx)
                .sizes()
                .get(1)
                .copied()
                .unwrap_or(area / 2.),
            _ => area,
        }
    }

    pub(super) fn render_panes(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = *cx.palette();
        let Some(doc) = self.active_doc() else {
            return div().into_any_element();
        };

        let editor = div().size_full().bg(p.surface_editor).child(
            Editor::new(&doc.editor)
                .appearance(false)
                .bordered(false)
                .h_full(),
        );
        let preview = self.render_preview(doc, self.preview_width(window, cx), &p, cx);

        h_resizable("panes")
            .with_state(&self.pane_split)
            .child(
                resizable_panel()
                    .visible(self.view != ViewMode::Preview)
                    .size_range(px(240.)..Pixels::MAX)
                    .child(editor),
            )
            .child(
                resizable_panel()
                    .visible(self.view != ViewMode::Editor)
                    .size_range(px(240.)..Pixels::MAX)
                    .child(preview),
            )
            .into_any_element()
    }

    fn render_preview(
        &self,
        doc: &Doc,
        width: Pixels,
        p: &Palette,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let list = doc.preview.read(cx).list_state().clone();
        let margin = ((width - PREVIEW_COLUMN) / 2.).max(px(40.));
        let top = list.logical_scroll_top();
        let scrolled = top.item_ix > 0 || top.offset_in_item > px(0.);
        let clear = p.surface_preview.opacity(0.);
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
            .pt(PREVIEW_TOP);

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
                        .top(PREVIEW_TOP)
                        .left_0()
                        .right(px(12.))
                        .h(px(20.))
                        .bg(linear_gradient(
                            180.,
                            linear_color_stop(p.surface_preview, 0.),
                            linear_color_stop(clear, 1.),
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
        let path = PathBuf::from(percent_decode(path));
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
        let anchor = percent_decode(anchor).to_lowercase();
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

/// Turn `%20` and friends in a link back into the characters they stand for.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut ix = 0;
    while ix < bytes.len() {
        let hex = (bytes[ix] == b'%')
            .then(|| text.get(ix + 1..ix + 3))
            .flatten();
        match hex.and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
            Some(byte) => {
                decoded.push(byte);
                ix += 3;
            }
            None => {
                decoded.push(bytes[ix]);
                ix += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

#[cfg(test)]
mod tests {
    use super::percent_decode;

    #[test]
    fn decodes_percent_escapes() {
        assert_eq!(
            percent_decode("my%20notes/caf%C3%A9.md"),
            "my notes/café.md"
        );
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("a%zzb"), "a%zzb");
    }
}
