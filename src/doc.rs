//! What we know about a Markdown source without rendering it: its headings and
//! its size. Both are single passes over the text, so they can run on every edit.

use gpui_kit::SharedString;

/// One heading in the document outline.
#[derive(Clone, Debug, PartialEq)]
pub struct Heading {
    /// 1 for `#` through 6 for `######`.
    pub level: u8,
    pub title: SharedString,
    /// Zero-based line of the heading in the source.
    pub line: u32,
}

/// Collect the headings of a Markdown source, skipping front matter and code fences.
pub fn outline(text: &str) -> Vec<Heading> {
    let mut headings = Vec::new();
    // The fence that opened the code block we are in: its character and length.
    let mut fence: Option<(u8, usize)> = None;
    let mut in_front_matter = false;
    // The previous line, when it could become a setext heading (`Title` over `===`).
    let mut paragraph: Option<(&str, u32)> = None;

    for (line_ix, raw) in text.lines().enumerate() {
        let line_ix = line_ix as u32;
        let line = raw.trim_end();

        if line_ix == 0 && line == "---" {
            in_front_matter = true;
            continue;
        }
        if in_front_matter {
            in_front_matter = !(line == "---" || line == "...");
            continue;
        }

        let indent = line.len() - line.trim_start_matches(' ').len();
        let body = &line[indent..];

        if let Some((ch, len)) = fence {
            let run = body.bytes().take_while(|b| *b == ch).count();
            if indent < 4 && run >= len && body[run..].trim().is_empty() {
                fence = None;
            }
            continue;
        }
        if indent >= 4 {
            // Indented code, or a continuation line: never a heading.
            paragraph = None;
            continue;
        }
        if let Some(ch) = body.bytes().next().filter(|b| *b == b'`' || *b == b'~') {
            let run = body.bytes().take_while(|b| *b == ch).count();
            if run >= 3 {
                fence = Some((ch, run));
                paragraph = None;
                continue;
            }
        }

        let hashes = body.bytes().take_while(|b| *b == b'#').count();
        if (1..=6).contains(&hashes)
            && body[hashes..]
                .chars()
                .next()
                .is_none_or(|c| c == ' ' || c == '\t')
        {
            let title = body[hashes..].trim().trim_end_matches('#').trim_end();
            headings.push(Heading {
                level: hashes as u8,
                title: title.to_string().into(),
                line: line_ix,
            });
            paragraph = None;
            continue;
        }

        if let Some((title, title_line)) = paragraph {
            let underline = |ch: char| !body.is_empty() && body.chars().all(|c| c == ch);
            if underline('=') || underline('-') {
                let level = if underline('=') { 1 } else { 2 };
                headings.push(Heading {
                    level,
                    title: title.trim().to_string().into(),
                    line: title_line,
                });
                paragraph = None;
                continue;
            }
        }

        let starts_block = body.is_empty() || body.starts_with(['>', '-', '*', '+', '|', '<']);
        paragraph = if starts_block {
            None
        } else {
            Some((body, line_ix))
        };
    }
    headings
}

/// The anchor a heading is linked to with `[text](#anchor)`, as GitHub derives
/// it: lower case, punctuation dropped, spaces turned into hyphens.
pub fn anchor(title: &str) -> String {
    title
        .trim()
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            '-' | '_' => Some(c),
            c if c.is_alphanumeric() => Some(c.to_lowercase().next().unwrap_or(c)),
            _ => None,
        })
        .collect()
}

/// A few plain words from the source lines `from..to`, for a card that says
/// what a section, or a file, is about: the first sentences of its prose, with
/// the Markdown taken out. Headings, code, tables and front matter are passed over.
pub fn excerpt(text: &str, from: u32, to: Option<u32>, limit: usize) -> String {
    let mut words = String::new();
    let mut fence: Option<(u8, usize)> = None;
    let mut in_front_matter = false;
    let to = to.unwrap_or(u32::MAX);
    for (line_ix, raw) in text.lines().enumerate() {
        let line_ix = line_ix as u32;
        if line_ix >= to || words.len() >= limit {
            break;
        }
        let line = raw.trim();
        if line_ix == 0 && line == "---" {
            in_front_matter = true;
            continue;
        }
        if in_front_matter {
            in_front_matter = !(line == "---" || line == "...");
            continue;
        }
        if let Some((ch, len)) = fence {
            let run = line.bytes().take_while(|b| *b == ch).count();
            if run >= len && line[run..].trim().is_empty() {
                fence = None;
            }
            continue;
        }
        if let Some(ch) = line.bytes().next().filter(|b| *b == b'`' || *b == b'~') {
            let run = line.bytes().take_while(|b| *b == ch).count();
            if run >= 3 {
                fence = Some((ch, run));
                continue;
            }
        }
        if line_ix < from {
            continue;
        }
        // One paragraph says enough, once it says something.
        if line.is_empty() && words.len() >= 60 {
            break;
        }
        // Not prose: headings, tables, rules, images, the label of a callout, HTML.
        let skip = line.is_empty()
            || line.starts_with('#')
            || line.starts_with('|')
            || line.starts_with("![")
            || line.starts_with('<')
            || line.starts_with("$$")
            || line.trim_start_matches(['>', ' ']).starts_with("[!")
            || line
                .chars()
                .all(|c| matches!(c, '-' | '=' | '*' | '_' | ' '));
        if skip {
            continue;
        }
        // A formula does not read as words: the excerpt stops where one starts.
        let (line, formula) = match line.find('$') {
            Some(at) => (line[..at].trim_end(), true),
            None => (line, false),
        };
        let plain = plain_text(line);
        if !plain.is_empty() {
            if !words.is_empty() {
                words.push(' ');
            }
            words.push_str(&plain);
        }
        if formula {
            if !words.is_empty() {
                words.push('…');
            }
            return words;
        }
    }
    if words.len() > limit {
        // Cut at a word, not through one.
        let mut end = limit;
        while !words.is_char_boundary(end) {
            end -= 1;
        }
        let cut = words[..end].rfind(' ').unwrap_or(end);
        words.truncate(cut);
        words.push('…');
    }
    words
}

/// One line of Markdown as it reads once rendered: without the marks for
/// lists, quotes, emphasis and code, and with links reduced to their text.
fn plain_text(line: &str) -> String {
    let mut rest = line.trim_start_matches(['>', ' ']);
    for marker in ["- [ ] ", "- [x] ", "- [X] ", "- ", "* ", "+ "] {
        if let Some(after) = rest.strip_prefix(marker) {
            rest = after;
            break;
        }
    }
    // `1. ` and `12) `.
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0 && rest[digits..].starts_with(['.', ')']) {
        rest = rest[digits + 1..].trim_start();
    }

    let mut plain = String::with_capacity(rest.len());
    let mut chars = rest.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' | '_' | '`' | '~' => {}
            // `[text](target)` reads as `text`.
            '[' => {}
            ']' => {
                if chars.peek() == Some(&'(') {
                    for skipped in chars.by_ref() {
                        if skipped == ')' {
                            break;
                        }
                    }
                }
            }
            '\\' => {
                if let Some(escaped) = chars.next() {
                    plain.push(escaped);
                }
            }
            c => plain.push(c),
        }
    }
    plain.trim().to_string()
}

/// How many words there are in the source lines `from..to`.
pub fn words_between(text: &str, from: u32, to: Option<u32>) -> usize {
    text.lines()
        .skip(from as usize)
        .take(to.map_or(usize::MAX, |to| to.saturating_sub(from) as usize))
        .flat_map(str::split_whitespace)
        .filter(|word| word.chars().any(char::is_alphanumeric))
        .count()
}

/// Size of a document, for the status bar.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
    pub words: usize,
}

impl Stats {
    pub fn of(text: &str) -> Self {
        Self {
            words: text
                .split_whitespace()
                .filter(|w| w.chars().any(char::is_alphanumeric))
                .count(),
        }
    }

    /// Reading time in minutes at 220 words a minute, never less than one.
    pub fn minutes(&self) -> usize {
        self.words.div_ceil(220).max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excerpts_are_prose_without_the_markdown() {
        let text = "---\ntitle: x\n---\n# Title\n\nA **native** editor built on [GPUI](https://gpui.rs).\n\n```rust\nfn main() {}\n```\n\n> [!TIP]\n> Press `Cmd K` to jump.\n\n## Next\n\n- [x] One thing\n| a | b |\n";
        assert_eq!(
            excerpt(text, 0, None, 200),
            "A native editor built on GPUI. Press Cmd K to jump. One thing"
        );
        // Only the lines of one section, and no longer than asked.
        assert_eq!(excerpt(text, 14, None, 200), "One thing");
        assert_eq!(excerpt(text, 0, Some(6), 12), "A native…");
        // A formula ends it; and so does the end of a paragraph that said enough.
        assert_eq!(
            excerpt(
                "Frame time stays under $t_f = 8$ ms.\n\nMore.",
                0,
                None,
                200
            ),
            "Frame time stays under…"
        );
        let long =
            "This first paragraph is comfortably longer than sixty characters in all.\n\nSecond.";
        assert!(!excerpt(long, 0, None, 200).contains("Second"));
        assert_eq!(words_between(text, 14, None), 6);
    }

    fn titles(text: &str) -> Vec<(u8, String, u32)> {
        outline(text)
            .into_iter()
            .map(|h| (h.level, h.title.to_string(), h.line))
            .collect()
    }

    #[test]
    fn finds_atx_headings() {
        let text = "# One\n\ntext\n\n## Two ##\n###Not a heading\n####### seven\n#\n";
        assert_eq!(
            titles(text),
            [
                (1, "One".into(), 0),
                (2, "Two".into(), 4),
                (1, "".into(), 7)
            ]
        );
    }

    #[test]
    fn skips_front_matter_and_code() {
        let text = "---\ntitle: x\n# not\n---\n# Real\n```sh\n# comment\n```\n~~~~\n# also not\n~~~\n~~~~\n    # indented\n## After\n";
        assert_eq!(
            titles(text),
            [(1, "Real".into(), 4), (2, "After".into(), 13)]
        );
    }

    #[test]
    fn finds_setext_headings() {
        let text = "Title\n=====\n\nSub\n---\n\n- item\n---\n";
        assert_eq!(titles(text), [(1, "Title".into(), 0), (2, "Sub".into(), 3)]);
    }

    #[test]
    fn derives_anchors() {
        assert_eq!(anchor("Render pipeline"), "render-pipeline");
        assert_eq!(anchor("  What's new in v2.0?  "), "whats-new-in-v20");
        assert_eq!(anchor("snake_case & kebab-case"), "snake_case--kebab-case");
    }

    #[test]
    fn counts_words() {
        let stats = Stats::of("# Hello world\n\n- one\n- two --> three\n");
        assert_eq!(stats.words, 5);
        assert_eq!(stats.minutes(), 1);
        assert_eq!(Stats { words: 441 }.minutes(), 3);
    }
}
