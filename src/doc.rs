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
        if (1..=6).contains(&hashes) && body[hashes..].chars().next().is_none_or(|c| c == ' ' || c == '\t') {
            let title = body[hashes..].trim().trim_end_matches('#').trim_end();
            headings.push(Heading { level: hashes as u8, title: title.to_string().into(), line: line_ix });
            paragraph = None;
            continue;
        }

        if let Some((title, title_line)) = paragraph {
            let underline = |ch: char| !body.is_empty() && body.chars().all(|c| c == ch);
            if underline('=') || underline('-') {
                let level = if underline('=') { 1 } else { 2 };
                headings.push(Heading { level, title: title.trim().to_string().into(), line: title_line });
                paragraph = None;
                continue;
            }
        }

        let starts_block = body.is_empty() || body.starts_with(['>', '-', '*', '+', '|', '<']);
        paragraph = if starts_block { None } else { Some((body, line_ix)) };
    }
    headings
}

/// Size of a document, for the status bar.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
    pub words: usize,
}

impl Stats {
    pub fn of(text: &str) -> Self {
        Self { words: text.split_whitespace().filter(|w| w.chars().any(char::is_alphanumeric)).count() }
    }

    /// Reading time in minutes at 220 words a minute, never less than one.
    pub fn minutes(&self) -> usize {
        self.words.div_ceil(220).max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titles(text: &str) -> Vec<(u8, String, u32)> {
        outline(text).into_iter().map(|h| (h.level, h.title.to_string(), h.line)).collect()
    }

    #[test]
    fn finds_atx_headings() {
        let text = "# One\n\ntext\n\n## Two ##\n###Not a heading\n####### seven\n#\n";
        assert_eq!(titles(text), [(1, "One".into(), 0), (2, "Two".into(), 4), (1, "".into(), 7)]);
    }

    #[test]
    fn skips_front_matter_and_code() {
        let text = "---\ntitle: x\n# not\n---\n# Real\n```sh\n# comment\n```\n~~~~\n# also not\n~~~\n~~~~\n    # indented\n## After\n";
        assert_eq!(titles(text), [(1, "Real".into(), 4), (2, "After".into(), 13)]);
    }

    #[test]
    fn finds_setext_headings() {
        let text = "Title\n=====\n\nSub\n---\n\n- item\n---\n";
        assert_eq!(titles(text), [(1, "Title".into(), 0), (2, "Sub".into(), 3)]);
    }

    #[test]
    fn counts_words() {
        let stats = Stats::of("# Hello world\n\n- one\n- two --> three\n");
        assert_eq!(stats.words, 5);
        assert_eq!(stats.minutes(), 1);
        assert_eq!(Stats { words: 441 }.minutes(), 3);
    }
}
