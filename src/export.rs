//! Export a document as a single HTML file that needs nothing else to display:
//! no scripts, no network. Math and diagrams are embedded as vector pictures,
//! drawn by the same engines the preview uses.

use markdown::mdast::Node;

use crate::preview::{self, frontmatter};
use crate::theme::Palette;

/// Render `source` to a complete HTML page. `name` is the title to fall back on.
pub fn html(source: &str, name: &str) -> String {
    let options = markdown::Options {
        parse: preview::parse_options(),
        compile: markdown::CompileOptions {
            allow_dangerous_html: true,
            ..markdown::CompileOptions::gfm()
        },
    };

    // Swap what plain Markdown cannot express for ready-made HTML, then let the
    // Markdown compiler do the rest. Replacing from the end keeps offsets valid.
    let mut body = source.to_string();
    let mut meta = frontmatter::Meta::default();
    if let Ok(tree) = markdown::to_mdast(source, &options.parse) {
        let mut swaps = Vec::new();
        collect_swaps(&tree, &mut swaps, &mut meta);
        swaps.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
        for (range, html) in swaps {
            if body.is_char_boundary(range.start) && body.is_char_boundary(range.end) {
                body.replace_range(range, &html);
            }
        }
    }
    let content =
        markdown::to_html_with_options(&body, &options).unwrap_or_else(|_| escape(source));
    let content = style_callouts(&content);

    let has_heading = crate::doc::outline(source)
        .first()
        .is_some_and(|heading| heading.level == 1);
    let title = meta
        .title
        .clone()
        .unwrap_or_else(|| name.trim_end_matches(".md").to_string());
    let mut header = String::new();
    if meta.title.is_some() && !has_heading {
        header.push_str(&format!("<h1>{}</h1>\n", escape(&title)));
    }
    if !meta.tags.is_empty() {
        let tags: String = meta
            .tags
            .iter()
            .map(|tag| format!("<span>{}</span>", escape(tag)))
            .collect();
        header.push_str(&format!("<p class=\"tags\">{tags}</p>\n"));
    }

    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{}</title>\n<style>\n{}</style>\n</head>\n<body>\n<main>\n{header}{content}</main>\n</body>\n</html>\n",
        escape(&title),
        stylesheet(),
    )
}

/// Find the nodes to replace: math, diagrams and the front matter.
fn collect_swaps(
    node: &Node,
    swaps: &mut Vec<(std::ops::Range<usize>, String)>,
    meta: &mut frontmatter::Meta,
) {
    let range = node
        .position()
        .map(|position| position.start.offset..position.end.offset);
    let swap = match node {
        Node::Yaml(yaml) => {
            *meta = frontmatter::parse(&yaml.value);
            Some(String::new())
        }
        Node::InlineMath(math) => preview::math::html(&math.value, false),
        Node::Math(math) => preview::math::html(&math.value, true)
            .map(|svg| format!("<div class=\"math-block\">{svg}</div>")),
        Node::Code(code)
            if code
                .lang
                .as_deref()
                .is_some_and(|lang| lang.eq_ignore_ascii_case("mermaid")) =>
        {
            // Exports use the light palette: a page is read on white more often than not.
            preview::mermaid::svg(&code.value, &Palette::light())
                .ok()
                .map(|svg| {
                    // An HTML block in Markdown ends at the first blank line.
                    let svg: String = svg
                        .lines()
                        .filter(|line| !line.trim().is_empty())
                        .collect::<Vec<_>>()
                        .join(" ");
                    format!("<figure class=\"diagram\">{svg}</figure>")
                })
        }
        _ => None,
    };
    match (range, swap) {
        (Some(range), Some(html)) => swaps.push((range, html)),
        _ => {
            for child in node.children().into_iter().flatten() {
                collect_swaps(child, swaps, meta);
            }
        }
    }
}

/// Turn `> [!TIP]` blockquotes into titled callout boxes.
fn style_callouts(html: &str) -> String {
    const KINDS: &[(&str, &str, &str)] = &[
        ("NOTE", "Note", "info"),
        ("INFO", "Note", "info"),
        ("TIP", "Tip", "accent"),
        ("HINT", "Tip", "accent"),
        ("IMPORTANT", "Important", "accent"),
        ("SUCCESS", "Success", "success"),
        ("WARNING", "Warning", "warning"),
        ("CAUTION", "Caution", "danger"),
        ("DANGER", "Caution", "danger"),
    ];
    let mut out = html.to_string();
    for (marker, title, tone) in KINDS {
        for marker in [marker.to_string(), marker.to_lowercase()] {
            let plain = format!("<blockquote>\n<p>[!{marker}]\n");
            let styled = format!(
                "<blockquote class=\"callout {tone}\">\n<p class=\"callout-title\">{title}</p>\n<p>"
            );
            out = out.replace(&plain, &styled);
        }
    }
    out
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The page's look: the app's palette, light or dark as the reader's system is.
fn stylesheet() -> String {
    let variables = |p: &Palette| {
        let hex = |color: gpui_kit::Hsla| {
            let gpui_kit::Rgba { r, g, b, a } = color.into();
            let byte = |v: f32| (v.clamp(0., 1.) * 255.).round() as u8;
            format!("rgb({} {} {} / {:.2})", byte(r), byte(g), byte(b), a)
        };
        [
            ("page", p.surface_preview),
            ("text", p.text_primary),
            ("muted", p.text_secondary),
            ("faint", p.text_tertiary),
            ("line", p.border_subtle),
            ("sunken", p.surface_sunken),
            ("link", p.accent_text),
            ("accent", p.accent),
            ("accent-soft", p.accent_soft),
            ("info", p.status_info),
            ("info-soft", p.status_info_soft),
            ("success", p.status_success),
            ("success-soft", p.status_success_soft),
            ("warning", p.status_warning),
            ("warning-soft", p.status_warning_soft),
            ("danger", p.status_danger),
            ("danger-soft", p.status_danger_soft),
        ]
        .iter()
        .map(|(name, color)| format!("--{name}:{};", hex(*color)))
        .collect::<String>()
    };
    format!(
        ":root{{{}}}\n@media (prefers-color-scheme: dark){{:root{{{}}}}}\n{}",
        variables(&Palette::light()),
        variables(&Palette::dark()),
        include_str!("export.css"),
    )
}

#[cfg(test)]
mod tests {
    use super::html;

    #[test]
    fn exports_a_self_contained_page() {
        let source = "---\ntitle: Report\ntags: [a, b]\n---\n\nText with $x^2$ and **bold**.\n\n> [!TIP]\n> Be kind.\n\n$$\n\\frac{a}{b}\n$$\n\n```mermaid\nflowchart LR\n  A --> B\n```\n\n| a | b |\n| - | - |\n| 1 | 2 |\n";
        let page = html(source, "report.md");
        assert!(page.starts_with("<!doctype html>"));
        assert!(page.contains("<title>Report</title>"));
        // No heading in the text, so the front matter title becomes one.
        assert!(page.contains("<h1>Report</h1>"));
        assert!(page.contains("<p class=\"tags\"><span>a</span><span>b</span></p>"));
        assert!(page.contains("<strong>bold</strong>"));
        assert!(page.contains("<table>"));
        assert!(page.contains("class=\"callout accent\""), "callout");
        assert!(page.contains("<svg class=\"math\""), "inline math");
        // Math takes the colour of the text around it, so it reads in dark mode too.
        assert!(!page.contains("rgba(0,0,0,1)"));
        assert!(
            page.contains("<div class=\"math-block\"><svg"),
            "display math"
        );
        assert!(page.contains("<figure class=\"diagram\"><svg"), "diagram");
        // The sources they replace are gone, and nothing is fetched from elsewhere.
        assert!(!page.contains("flowchart LR") && !page.contains("title: Report"));
        assert!(
            !page.contains("<script") && !page.contains("http://") || page.contains("www.w3.org")
        );
    }

    #[test]
    fn keeps_plain_documents_plain() {
        let page = html("# Hello\n\nWorld & <friends>\n", "hello.md");
        assert!(page.contains("<h1>Hello</h1>"));
        assert!(page.contains("<title>hello</title>"));
        assert!(!page.contains("class=\"tags\""));
    }
}
