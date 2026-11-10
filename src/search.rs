//! Searching the text of every Markdown file in the folder.
//!
//! A plain scan: read each file and look at each line. Notes folders are small
//! enough that this answers faster than a keystroke, and it needs no index to
//! build or keep fresh.

use std::path::PathBuf;

/// One line that contains what was searched for.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub path: PathBuf,
    /// Zero-based line in the file.
    pub line: u32,
    /// The line, trimmed, and shortened around the match if it is long.
    pub text: String,
}

/// Files larger than this are skipped: they are not notes, and would slow every search.
const LARGEST_FILE: u64 = 4 * 1024 * 1024;
/// How much of a long line is shown.
const SHOWN: usize = 110;

/// Find `query` in `files`, ignoring case. Stops after `limit` hits.
pub fn run(files: &[PathBuf], query: &str, limit: usize) -> Vec<Hit> {
    let query = query.trim().to_lowercase();
    let mut hits = Vec::new();
    if query.is_empty() {
        return hits;
    }
    for path in files {
        if std::fs::metadata(path).is_ok_and(|file| file.len() > LARGEST_FILE) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        for (line, content) in text.lines().enumerate() {
            let Some(at) = content.to_lowercase().find(&query) else {
                continue;
            };
            hits.push(Hit {
                path: path.clone(),
                line: line as u32,
                text: excerpt(content, at),
            });
            if hits.len() >= limit {
                return hits;
            }
        }
    }
    hits
}

/// A long line cut down to the part around byte `at`.
fn excerpt(line: &str, at: usize) -> String {
    let line = line.trim();
    if line.chars().count() <= SHOWN {
        return line.to_string();
    }
    // Lower-casing can shift byte positions slightly; land on a character either way.
    let chars: Vec<char> = line.chars().collect();
    let hit = line
        .char_indices()
        .take_while(|(ix, _)| *ix < at)
        .count()
        .min(chars.len());
    let start = hit.saturating_sub(SHOWN / 3);
    let end = (start + SHOWN).min(chars.len());
    let mut shown: String = chars[start..end].iter().collect();
    if start > 0 {
        shown.insert(0, '…');
    }
    if end < chars.len() {
        shown.push('…');
    }
    shown
}

#[cfg(test)]
mod tests {
    use super::{excerpt, run};

    #[test]
    fn finds_lines_ignoring_case() {
        let folder = std::env::temp_dir().join(format!("zwinmd-search-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let (a, b) = (folder.join("a.md"), folder.join("b.md"));
        std::fs::write(&a, "# Title\n\nThe Quick brown fox.\nnothing here\n").unwrap();
        std::fs::write(&b, "quick again\nand QUICK once more\n").unwrap();

        let hits = run(
            &[a.clone(), b.clone(), folder.join("missing.md")],
            "quick",
            10,
        );
        let found: Vec<(&std::path::Path, u32)> = hits
            .iter()
            .map(|hit| (hit.path.as_path(), hit.line))
            .collect();
        assert_eq!(
            found,
            [(a.as_path(), 2), (b.as_path(), 0), (b.as_path(), 1)]
        );
        assert_eq!(hits[0].text, "The Quick brown fox.");

        assert_eq!(run(&[a.clone(), b.clone()], "quick", 2).len(), 2);
        assert!(run(&[a, b], "   ", 10).is_empty());
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn shortens_long_lines_around_the_match() {
        let line = format!("{}needle{}", "a ".repeat(200), " b".repeat(200));
        let shown = excerpt(&line, 400);
        assert!(shown.contains("needle"));
        assert!(shown.starts_with('…') && shown.ends_with('…'));
        assert!(shown.chars().count() <= 112);
    }
}
