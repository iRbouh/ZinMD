//! Front matter: the YAML between `---` lines at the top of a document.
//!
//! Shown as a quiet header: tags become chips, other fields a line of small
//! print. `title` is shown as a heading only when the document has no top-level
//! heading of its own, so the title never appears twice.

use gpui_kit::component::text::{
    MarkdownNode, MarkdownParseContext, MarkdownPlugin, markdown_ast as mdast,
};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::theme::{ActivePalette as _, size};

/// What we read out of a front matter block.
#[derive(Debug, Default, PartialEq)]
pub struct Meta {
    pub title: Option<String>,
    pub tags: Vec<String>,
    /// Every other top-level field, in order, as plain text.
    pub fields: Vec<(String, String)>,
}

/// Read the top-level fields of a YAML mapping. This is not a YAML parser: it
/// understands what front matter is used for in practice (scalars, `[a, b]`
/// lists and `- item` lists) and shows anything else as written.
pub fn parse(yaml: &str) -> Meta {
    let mut fields: Vec<(String, Vec<String>)> = Vec::new();
    for line in yaml.lines() {
        let text = line.trim();
        if text.is_empty() || text.starts_with('#') {
            continue;
        }
        let nested = line.starts_with([' ', '\t']) || text.starts_with("- ");
        match (nested, fields.last_mut()) {
            // A list item or a continuation line of the field above.
            (true, Some((_, values))) => {
                values.push(unquote(text.strip_prefix("- ").unwrap_or(text)).to_string())
            }
            _ => {
                let Some((key, value)) = text.split_once(':') else {
                    continue;
                };
                let value = value.trim();
                let values = match value
                    .strip_prefix('[')
                    .and_then(|list| list.strip_suffix(']'))
                {
                    Some(list) => list
                        .split(',')
                        .map(|item| unquote(item.trim()).to_string())
                        .collect(),
                    None => vec![unquote(value).to_string()],
                };
                fields.push((key.trim().to_string(), values));
            }
        }
    }

    let mut meta = Meta::default();
    for (key, values) in fields {
        let values: Vec<String> = values
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect();
        match key.to_ascii_lowercase().as_str() {
            "title" => meta.title = values.into_iter().next(),
            "tags" | "tag" | "keywords" | "categories" => {
                // `tags: a, b` is as common as a real list.
                meta.tags.extend(
                    values
                        .iter()
                        .flat_map(|value| value.split(','))
                        .map(|tag| tag.trim().to_string()),
                );
                meta.tags.retain(|tag| !tag.is_empty());
            }
            _ if values.is_empty() => {}
            _ => meta.fields.push((key, values.join(", "))),
        }
    }
    meta
}

fn unquote(value: &str) -> &str {
    let quoted = |q: char| value.len() >= 2 && value.starts_with(q) && value.ends_with(q);
    if quoted('"') || quoted('\'') {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

struct Header {
    meta: Meta,
    show_title: bool,
}

pub struct FrontMatter;

impl MarkdownPlugin for FrontMatter {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        "front-matter"
    }

    fn parse(&self, node: &mdast::Node, cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let mdast::Node::Yaml(yaml) = node else {
            return None;
        };
        let meta = parse(&yaml.value);
        let has_heading = crate::doc::outline(cx.source())
            .first()
            .is_some_and(|heading| heading.level == 1);
        let show_title = meta.title.is_some() && !has_heading;
        let source = cx
            .node_source(node)
            .unwrap_or(yaml.value.as_str())
            .to_string();
        Some(
            MarkdownNode::new("front-matter", Header { meta, show_title })
                .text(yaml.value.clone())
                .markdown(source),
        )
    }

    fn render(&self, node: &MarkdownNode, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = *cx.palette();
        let Some(Header { meta, show_title }) = node.data::<Header>() else {
            return div();
        };

        let title = meta.title.clone().filter(|_| *show_title).map(|title| {
            div()
                .text_size(size::FONT_H1)
                .font_weight(FontWeight::BOLD)
                .line_height(relative(1.25))
                .child(title)
        });
        let tags = (!meta.tags.is_empty()).then(|| {
            h_flex()
                .flex_wrap()
                .gap(px(6.))
                .children(meta.tags.iter().map(|tag| {
                    div()
                        .px(px(8.))
                        .rounded_full()
                        .border_1()
                        .border_color(p.border_subtle)
                        .bg(p.surface_sunken)
                        .text_size(px(12.))
                        .line_height(px(20.))
                        .text_color(p.text_secondary)
                        .child(tag.clone())
                }))
        });
        let fields = (!meta.fields.is_empty()).then(|| {
            h_flex()
                .flex_wrap()
                .gap_x(px(16.))
                .gap_y(px(2.))
                .text_size(px(12.))
                .line_height(px(18.))
                .children(meta.fields.iter().map(|(key, value)| {
                    h_flex()
                        .gap(px(6.))
                        .child(div().text_color(p.text_tertiary).child(key.clone()))
                        .child(div().text_color(p.text_secondary).child(value.clone()))
                }))
        });

        let empty = title.is_none() && tags.is_none() && fields.is_none();
        v_flex()
            .w_full()
            .gap(px(10.))
            .when(!empty, |header| header.pb(px(4.)))
            .children(title)
            .children(tags)
            .children(fields)
    }
}

#[cfg(test)]
mod tests {
    use super::{Meta, parse};

    #[test]
    fn reads_scalars_and_lists() {
        let meta = parse(
            "title: \"ZinMD architecture\"\ntags: [gpui, rust, 'markdown']\nauthor: Ismail\ndate: 2026-11-10\n",
        );
        assert_eq!(meta.title.as_deref(), Some("ZinMD architecture"));
        assert_eq!(meta.tags, ["gpui", "rust", "markdown"]);
        assert_eq!(
            meta.fields,
            [
                ("author".into(), "Ismail".into()),
                ("date".into(), "2026-11-10".into())
            ]
        );
    }

    #[test]
    fn reads_block_lists_and_comma_tags() {
        let meta = parse(
            "# comment\ntags:\n  - one\n  - two\nkeywords: three, four\naliases:\n- a\n- b\nempty:\n",
        );
        assert_eq!(meta.tags, ["one", "two", "three", "four"]);
        assert_eq!(meta.fields, [("aliases".into(), "a, b".into())]);
    }

    #[test]
    fn ignores_what_it_cannot_read() {
        assert_eq!(parse("just some words\n\n"), Meta::default());
    }
}
