//! Pictures that stand on their own line.
//!
//! A picture alone in its paragraph is shown as a figure: in the middle of
//! the column, never bigger than it is, with its description under it as a
//! caption. A click opens it large over the page; while the pointer is over
//! it, two small buttons offer to open it or copy it.
//!
//! A picture in the middle of a sentence is left to the kit, which sets it
//! in the line like a word.

use std::path::PathBuf;

use gpui_kit::assets::IconName;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::text::{
    MarkdownNode, MarkdownParseContext, MarkdownPlugin, markdown_ast as mdast,
};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{Shown, Subject, look_at, tool};
use crate::theme::{ActivePalette as _, size};

/// What `parse` keeps about a picture.
struct Picture {
    url: SharedUri,
    /// What is said under it: its title if it has one, else its description.
    caption: SharedString,
    /// Where it starts in the document: what tells one picture from another.
    start: usize,
}

pub struct Figure;

impl MarkdownPlugin for Figure {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        "figure"
    }

    fn parse(&self, node: &mdast::Node, cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let mdast::Node::Paragraph(paragraph) = node else {
            return None;
        };
        // Exactly one picture, and nothing else in the paragraph.
        let [mdast::Node::Image(image)] = paragraph.children.as_slice() else {
            return None;
        };
        let start = cx.offset()
            + paragraph
                .position
                .as_ref()
                .map_or(0, |position| position.start.offset);
        let caption = image
            .title
            .clone()
            .filter(|title| !title.trim().is_empty())
            .unwrap_or_else(|| image.alt.clone());
        let picture = Picture {
            url: image.url.clone().into(),
            caption: caption.trim().to_string().into(),
            start,
        };
        Some(
            MarkdownNode::new("figure", picture)
                .text(image.alt.clone())
                .markdown(cx.node_source(node).unwrap_or_default().to_string()),
        )
    }

    fn render(&self, node: &MarkdownNode, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some(picture) = node.data::<Picture>() else {
            return div().into_any_element();
        };
        let p = *cx.palette();
        let folder = cx
            .try_global::<Shown>()
            .and_then(|shown| shown.folder.clone());
        let source = super::image_source(&picture.url, folder.as_deref());
        let file = match &source {
            ImageSource::Resource(Resource::Path(path)) => Some(PathBuf::from(path.as_ref())),
            _ => None,
        };
        let subject = Subject::Picture {
            source: source.clone(),
            name: picture.caption.clone(),
            file,
        };
        let body = window.text_style().font_size.to_pixels(window.rem_size());
        let dark = cx.theme().is_dark();
        // How wide and how tall the picture is, once it has been read.
        let natural = match &source {
            ImageSource::Resource(resource) => window
                .use_asset::<ImageAssetLoader>(resource, cx)
                .and_then(Result::ok)
                .map(|read| read.size(0))
                .filter(|size| size.width.0 > 0 && size.height.0 > 0),
            _ => None,
        };

        let (open, expand, copy) = (subject.clone(), subject.clone(), subject);
        let tools = h_flex()
            .absolute()
            .top(px(8.))
            .right(px(8.))
            .gap(px(2.))
            .p(px(2.))
            .rounded(size::RADIUS_MD)
            .border_1()
            .border_color(p.border_subtle)
            .bg(p.surface_raised)
            .shadow_sm()
            // There while the pointer is over the picture.
            .opacity(0.)
            .group_hover("figure", |style| style.opacity(1.))
            .child(
                tool(("figure-open", picture.start), IconName::Expand, &p).on_click(
                    move |_, window, cx| {
                        cx.stop_propagation();
                        look_at(expand.clone(), window, cx);
                    },
                ),
            )
            .child(
                tool(("figure-copy", picture.start), IconName::Copy, &p).on_click(
                    move |_, _, cx| {
                        cx.stop_propagation();
                        copy.copy(cx);
                    },
                ),
            );

        v_flex()
            .w_full()
            .items_center()
            .gap(body * 0.5)
            .child(
                div()
                    .id(("figure", picture.start))
                    .group("figure")
                    .relative()
                    .max_w_full()
                    .cursor_pointer()
                    .map(|figure| {
                        let picture = img(source)
                            .object_fit(ObjectFit::Contain)
                            .rounded(size::RADIUS_MD)
                            // A faint edge, so a picture with a white ground
                            // does not melt into a white page.
                            .border_1()
                            .border_color(p.border_subtle.opacity(if dark { 0.6 } else { 1. }));
                        match natural {
                            // Two boxes, as for a diagram: the outer as wide
                            // as the picture, or as the column when that is
                            // narrower, the inner taking the picture's shape
                            // at that width. (Without them a picture wider
                            // than the column kept its full height, and left
                            // a blank between itself and its caption.)
                            Some(natural) => {
                                let (wide, tall) =
                                    (natural.width.0 as f32, natural.height.0 as f32);
                                figure.w_full().max_w(px(wide)).child(
                                    div()
                                        .w_full()
                                        .aspect_ratio(wide / tall)
                                        .child(picture.size_full()),
                                )
                            }
                            None => figure.child(picture.max_w(relative(1.))),
                        }
                    })
                    .child(tools)
                    // Spreading two fingers on it opens it too, and the
                    // same spread then goes on to zoom it.
                    .on_pinch({
                        let open = open.clone();
                        move |event, window, cx| {
                            if event.delta > 0. {
                                look_at(open.clone(), window, cx);
                            }
                        }
                    })
                    .on_click(move |_, window, cx| look_at(open.clone(), window, cx)),
            )
            .when(!picture.caption.is_empty(), |figure| {
                figure.child(
                    div()
                        .max_w_full()
                        .text_center()
                        .text_size(body * 0.86)
                        .line_height(relative(1.45))
                        .text_color(p.text_tertiary)
                        .child(picture.caption.clone()),
                )
            })
            .into_any_element()
    }
}
