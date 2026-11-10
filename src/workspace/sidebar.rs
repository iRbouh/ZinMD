//! The sidebar: jump field, the folder's Markdown files, and the outline of the
//! open document.

use gpui_kit::assets::IconName;
use gpui_kit::component::{Icon, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::Workspace;
use crate::theme::{ActivePalette as _, Palette, size};
use crate::ui::{icon_button, section_label};

const ROW_HEIGHT: Pixels = px(28.);
const OUTLINE_ROW_HEIGHT: Pixels = px(26.);
/// Space on each side of the rows, so selection highlights float inside the sidebar.
const GUTTER: Pixels = px(10.);

impl Workspace {
    pub(super) fn render_sidebar(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = *cx.palette();
        let file_count = self.tree.rows.len();

        let files = if file_count > 0 {
            uniform_list(
                "files",
                file_count,
                cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                    let p = *cx.palette();
                    range.map(|ix| this.render_file_row(ix, &p, cx)).collect::<Vec<_>>()
                }),
            )
            // As tall as its rows, giving way when the sidebar runs out of room.
            .h(ROW_HEIGHT * file_count as f32)
            .min_h(ROW_HEIGHT * 2.)
            .track_scroll(&self.tree_scroll)
            .into_any_element()
        } else {
            let hint = if self.tree.root.is_some() { "No Markdown files here" } else { "No folder open" };
            div().px(px(18.)).py(px(6.)).text_color(p.text_tertiary).child(hint).into_any_element()
        };

        v_flex()
            .flex_none()
            .w(size::SIDEBAR)
            .h_full()
            .border_r_1()
            .border_color(p.border_subtle)
            .child(self.render_jump_field(&p))
            .child(
                h_flex()
                    .flex_none()
                    .justify_between()
                    .h(px(28.))
                    .mt(px(10.))
                    .pl(px(18.))
                    .pr(px(8.))
                    .child(section_label("FILES", &p))
                    .child(icon_button("new-file", IconName::Plus, &p).size(px(22.))),
            )
            .child(files)
            .child(self.render_outline(&p, cx))
    }

    /// Looks like a field; opens the command palette.
    fn render_jump_field(&self, p: &Palette) -> impl IntoElement + use<> {
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

    fn render_file_row(&self, ix: usize, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let row = &self.tree.rows[ix];
        let selected = !row.is_dir && self.active_doc().is_some_and(|doc| doc.path.as_deref() == Some(row.path.as_path()));
        let hover = p.surface_hover;

        let chevron = div().flex_none().size(px(12.)).when(row.is_dir, |slot| {
            let icon = if row.open { IconName::ChevronDown } else { IconName::ChevronRight };
            slot.child(Icon::new(icon).size(px(12.)).text_color(p.text_tertiary))
        });
        let icon = match (row.is_dir, row.open) {
            (true, true) => IconName::FolderOpen,
            (true, false) => IconName::Folder,
            _ => IconName::FileText,
        };

        div().h(ROW_HEIGHT).px(GUTTER).child(
            h_flex()
                .id(("file", ix))
                .h(ROW_HEIGHT - px(1.))
                .pl(px(6.) + px(16.) * row.depth as f32)
                .pr(px(8.))
                .gap(px(6.))
                .rounded(size::RADIUS_SM)
                .map(|item| {
                    if selected {
                        item.bg(p.surface_selected).font_weight(FontWeight::MEDIUM)
                    } else {
                        item.hover(move |style| style.bg(hover))
                    }
                })
                .child(chevron)
                .child(Icon::new(icon).size(px(15.)).text_color(if selected { p.accent_hover } else { p.text_secondary }))
                .child(div().flex_1().min_w_0().truncate().child(row.name.clone()))
                .on_click(cx.listener(move |this, _, window, cx| this.on_tree_click(ix, window, cx))),
        )
    }

    fn render_outline(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let Some(doc) = self.active_doc().filter(|doc| !doc.headings.is_empty()) else {
            return div();
        };
        // The heading the caret is in: the last one at or above its line.
        let caret_line = doc.editor.read(cx).cursor_position().line;
        let current = doc.headings.iter().rposition(|heading| heading.line <= caret_line);
        let top_level = doc.headings.iter().map(|heading| heading.level).min().unwrap_or(1);
        let (hover, strong) = (p.surface_hover, p.text_primary);

        let rows = doc.headings.iter().enumerate().map(|(ix, heading)| {
            let line = heading.line;
            let indent = px(12.) * (heading.level - top_level) as f32;
            h_flex()
                .id(("heading", ix))
                .h(OUTLINE_ROW_HEIGHT)
                .ml(px(18.))
                .mr(GUTTER)
                .pr(px(8.))
                .rounded_r(size::RADIUS_SM)
                .map(|item| {
                    if current == Some(ix) {
                        item.border_l_2()
                            .border_color(p.accent)
                            .pl(px(10.) + indent)
                            .text_color(p.accent_text)
                            .font_weight(FontWeight::MEDIUM)
                    } else {
                        item.border_l_1()
                            .border_color(p.border_subtle)
                            .pl(px(11.) + indent)
                            .text_color(p.text_secondary)
                            .hover(move |style| style.bg(hover).text_color(strong))
                    }
                })
                .child(div().flex_1().min_w_0().truncate().child(heading.title.clone()))
                .on_click(cx.listener(move |this, _, window, cx| this.jump_to_line(line, window, cx)))
        });

        div()
            .flex_none()
            .max_h(relative(0.5))
            .flex()
            .flex_col()
            .child(h_flex().flex_none().h(px(28.)).mt(px(10.)).pl(px(18.)).child(section_label("OUTLINE", p)))
            .child(div().id("outline").flex_1().min_h_0().overflow_y_scroll().pb(px(10.)).children(rows))
    }
}
