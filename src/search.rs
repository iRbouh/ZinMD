//! Searching the text of every Markdown file in the folder.
//!
//! A plain scan: read each file and look at each line. Notes folders are small
//! enough that this answers faster than a keystroke, and it needs no index to
//! build or keep fresh. The files are shared out among the cores.
//!
//! What is found is a part of a file: the lines under one heading. Each part
//! is shown once, by its best line, and the best parts come first:
//!
//! 1. the heading itself holds what was typed;
//! 2. a line holds it, just as typed;
//! 3. a line holds each of its words, in any order;
//! 4. the words are all under the heading, on different lines;
//! 5. nothing anywhere holds it: lines that nearly do, a slip of the finger
//!    being forgiven.
//!
//! Case and accents never count (see [`crate::matching`]).

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::doc;
use crate::matching::{self, Answer, Folded, Room, Wanted};

/// How good an answer a part of a file is. The best come first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rank {
    Heading,
    Phrase,
    Words,
    Spread,
    Close,
}

/// One part of a file that answers the search, by its best line.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub path: PathBuf,
    /// Zero-based line in the file.
    pub line: u32,
    /// The heading the line is under. Empty above the first heading, and
    /// when the line is the heading.
    pub heading: String,
    /// Whether the line is a heading.
    pub in_heading: bool,
    /// The line, without its Markdown marker, shortened around what was
    /// found if it is long.
    pub text: String,
    /// The parts of `text` that answer, in its bytes.
    pub marks: Vec<Range<usize>>,
    pub rank: Rank,
}

/// Files larger than this are skipped: they are not notes, and would slow every search.
const LARGEST_FILE: u64 = 4 * 1024 * 1024;
/// How much of a long line is kept.
const SHOWN: usize = 96;
/// How much of that a row has the room to show, more or less.
const SEEN: usize = 70;
/// How much of a long line may come before what was found in it.
const LEAD: usize = 28;
/// Fewer files than this for each core are not worth sharing out.
const SHARE: usize = 24;

/// Find `query` in `files`: the `limit` best answers, best first, and among
/// equals in the order of `files`. Setting `stop` gives the search up.
pub fn run(files: &[PathBuf], query: &str, limit: usize, stop: &AtomicBool) -> Vec<Hit> {
    let wanted = Wanted::new(query);
    if wanted.is_empty() {
        return Vec::new();
    }
    // Lines that nearly answer are looked for only until one answers.
    let (next, answered) = (AtomicUsize::new(0), AtomicBool::new(false));
    let work = || {
        let mut hits: Vec<(usize, Hit)> = Vec::new();
        let (mut folded, mut room) = (Folded::default(), Room::default());
        loop {
            let nth = next.fetch_add(1, Ordering::Relaxed);
            let Some(path) = files.get(nth).filter(|_| !stop.load(Ordering::Relaxed)) else {
                return hits;
            };
            if std::fs::metadata(path).is_ok_and(|file| file.len() > LARGEST_FILE) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(path) else {
                continue;
            };
            let near = !answered.load(Ordering::Relaxed);
            let before = hits.len();
            look_in(path, &text, &wanted, near, &mut folded, &mut room, |hit| {
                hits.push((nth, hit))
            });
            if hits[before..]
                .iter()
                .any(|(_, hit)| hit.rank != Rank::Close)
            {
                answered.store(true, Ordering::Relaxed);
            }
        }
    };
    let cores = std::thread::available_parallelism().map_or(1, |cores| cores.get());
    let workers = (files.len() / SHARE).clamp(1, cores.min(8));
    let mut hits = match workers {
        1 => work(),
        _ => std::thread::scope(|scope| {
            let working: Vec<_> = (0..workers).map(|_| scope.spawn(work)).collect();
            working
                .into_iter()
                .flat_map(|worker| worker.join().unwrap_or_default())
                .collect()
        }),
    };
    if hits.iter().any(|(_, hit)| hit.rank != Rank::Close) {
        hits.retain(|(_, hit)| hit.rank != Rank::Close);
    }
    hits.sort_by_key(|(nth, hit)| (hit.rank, *nth, hit.line));
    hits.truncate(limit);
    hits.into_iter().map(|(_, hit)| hit).collect()
}

/// The best line found so far in one part of a file.
struct Best {
    rank: Rank,
    in_heading: bool,
    line: u32,
    text: String,
    marks: Vec<Range<usize>>,
}

/// Look in the text of one file, and give `found` each part that answers.
fn look_in(
    path: &Path,
    text: &str,
    wanted: &Wanted,
    near: bool,
    folded: &mut Folded,
    room: &mut Room,
    mut found: impl FnMut(Hit),
) {
    let headings = doc::outline(text);
    let mut coming = headings.iter().peekable();
    // The part being read: its heading, its best line, which of the words it
    // holds, and the line that holds the most of them.
    let mut title = "";
    let mut best: Option<Best> = None;
    let (mut held, mut most): (u64, Option<(u32, Best)>) = (0, None);

    let mut close = |title: &str, best: Option<Best>, held: u64, most: Option<(u32, Best)>| {
        // No line holds every word, but the part does.
        let spread = || {
            let (_, line) = most.filter(|_| held == wanted.every_word())?;
            Some(Best {
                rank: Rank::Spread,
                ..line
            })
        };
        if let Some(best) = best.or_else(spread) {
            found(Hit {
                path: path.to_path_buf(),
                line: best.line,
                heading: match best.in_heading {
                    true => String::new(),
                    false => title.to_string(),
                },
                in_heading: best.in_heading,
                text: best.text,
                marks: best.marks,
                rank: best.rank,
            });
        }
    };

    for (line, content) in text.lines().enumerate() {
        let line = line as u32;
        let heading = coming.next_if(|heading| heading.line == line);
        if let Some(heading) = heading {
            close(title, best.take(), held, most.take());
            (title, held) = (heading.title.as_ref(), 0);
        }
        folded.fill(content);
        let rank = match matching::line_answers(folded, wanted, near, room) {
            Some(Answer::Close) => Some(Rank::Close),
            Some(_) if heading.is_some() => Some(Rank::Heading),
            Some(Answer::Exact) => Some(Rank::Phrase),
            Some(Answer::Words) => Some(Rank::Words),
            None => None,
        };
        let shown = |room: &mut Room, rank| {
            let (text, marks) = excerpt(content, &room.places(folded));
            Best {
                rank,
                in_heading: heading.is_some(),
                line,
                text,
                marks,
            }
        };
        match rank {
            // The first of the best lines stands for the part.
            Some(rank) => {
                if best.as_ref().is_none_or(|best| rank < best.rank) {
                    best = Some(shown(room, rank));
                }
            }
            None if wanted.several() && best.is_none() => {
                let here = matching::words_in(folded, wanted, room);
                held |= here;
                if here.count_ones() > most.as_ref().map_or(0, |(count, _)| *count) {
                    most = Some((here.count_ones(), shown(room, Rank::Spread)));
                }
            }
            None => {}
        }
    }
    close(title, best, held, most);
}

/// How much of the start of a line is Markdown saying what the line is,
/// rather than words: the marks of a heading, a quote, a list, a task.
fn marker(line: &str) -> usize {
    let after = |rest: &str| line.len() - rest.len();
    let mut rest = line.trim_start_matches([' ', '\t', '>', '#']);
    // One list marker: a bullet, or a number and its dot.
    let numbered = rest.trim_start_matches(|letter: char| letter.is_ascii_digit());
    let item = match rest.as_bytes().first() {
        Some(b'-' | b'*' | b'+') => Some(&rest[1..]),
        _ if numbered.len() < rest.len() => numbered.strip_prefix(['.', ')']),
        _ => None,
    };
    let Some(item) = item.filter(|item| item.starts_with(' ')) else {
        return after(rest);
    };
    rest = item.trim_start_matches(' ');
    // And the box of a task.
    for done in ["[ ] ", "[x] ", "[X] "] {
        if let Some(task) = rest.strip_prefix(done) {
            rest = task.trim_start_matches(' ');
        }
    }
    after(rest)
}

/// A line without its Markdown marker, cut down to the part around what was
/// found if it is long, and where `places` are in what is left.
fn excerpt(line: &str, places: &[Range<usize>]) -> (String, Vec<Range<usize>>) {
    let lead = marker(line);
    let body = line[lead..].trim_end();
    let letters: Vec<(usize, char)> = body.char_indices().collect();
    // The part of `body` that is shown, in its bytes.
    let (from, to) = if letters.len() <= SHOWN {
        (0, body.len())
    } else {
        // In letters: where what was found begins and ends.
        let nth = |byte: usize| letters.partition_point(|(at, _)| at + lead < byte);
        let first = places.first().map_or(0, |place| nth(place.start));
        let last = places.last().map_or(first, |place| nth(place.end));
        // What comes before it is there to place it. The further apart the
        // words are, the less of that there is, so that more of them show.
        let before = (SEEN.saturating_sub(last - first) / 2).clamp(4, LEAD);
        let mut start = first.saturating_sub(before).min(letters.len() - SHOWN);
        // Not from the middle of a word.
        while start > 0 && start < first && !letters[start - 1].1.is_whitespace() {
            start += 1;
        }
        let byte = |nth: usize| letters.get(nth).map_or(body.len(), |(at, _)| *at);
        (byte(start), byte(start + SHOWN))
    };
    let mut shown = String::with_capacity(to - from + 6);
    if from > 0 {
        shown.push('…');
    }
    let moved = shown.len();
    shown.push_str(&body[from..to]);
    if to < body.len() {
        shown.push('…');
    }
    let marks = places
        .iter()
        .filter_map(|place| {
            let start = place.start.checked_sub(lead)?.max(from);
            let end = (place.end - lead).min(to);
            (start < end).then(|| start - from + moved..end - from + moved)
        })
        .collect();
    (shown, marks)
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::atomic::AtomicBool;

    use super::{Hit, Rank, excerpt, marker, run};

    /// Search two files written for the test, and say what was found as
    /// `(file, line, rank, text with its marks in brackets, heading)`.
    fn found(name: &str, query: &str) -> Vec<(String, u32, Rank, String, String)> {
        let folder = std::env::temp_dir().join(format!("zinmd-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let (a, b) = (folder.join("a.md"), folder.join("b.md"));
        let first = "Above every heading: a quick note.\n\n# Frame budget\n\nThe Quick brown fox.\nnothing here\n\n## Late frames\n\n- [ ] Measure the café queue\n- budget for the frame\n\n```\n# not a heading: quick\n```\n";
        let second = "# Notes\n\nquick again\nand QUICK once more\n\n## Reading\n\nA frame is drawn.\nIts budget is small.\n";
        std::fs::write(&a, first).unwrap();
        std::fs::write(&b, second).unwrap();
        let files = [a, b, folder.join("missing.md")];
        let hits = run(&files, query, 10, &AtomicBool::new(false));
        std::fs::remove_dir_all(&folder).unwrap();
        hits.into_iter().map(told).collect()
    }

    fn told(hit: Hit) -> (String, u32, Rank, String, String) {
        let mut text = hit.text.clone();
        for mark in hit.marks.iter().rev() {
            text.insert(mark.end, ']');
            text.insert(mark.start, '[');
        }
        let file = Path::new(&hit.path).file_name().unwrap().to_string_lossy();
        (file.into_owned(), hit.line, hit.rank, text, hit.heading)
    }

    fn row(
        file: &str,
        line: u32,
        rank: Rank,
        text: &str,
        heading: &str,
    ) -> (String, u32, Rank, String, String) {
        (file.into(), line, rank, text.into(), heading.into())
    }

    #[test]
    fn each_part_of_a_file_is_found_once_by_its_best_line() {
        assert_eq!(
            found("parts", "quick"),
            [
                row(
                    "a.md",
                    0,
                    Rank::Phrase,
                    "Above every heading: a [quick] note.",
                    ""
                ),
                row(
                    "a.md",
                    4,
                    Rank::Phrase,
                    "The [Quick] brown fox.",
                    "Frame budget"
                ),
                row(
                    "a.md",
                    13,
                    Rank::Phrase,
                    "not a heading: [quick]",
                    "Late frames"
                ),
                row("b.md", 2, Rank::Phrase, "[quick] again", "Notes"),
            ]
        );
        assert!(found("nothing", "   ").is_empty());
    }

    #[test]
    fn headings_come_first_then_lines_then_words_apart() {
        assert_eq!(
            found("ranks", "frame budget"),
            [
                // The heading holds it, as typed.
                row("a.md", 2, Rank::Heading, "[Frame budget]", ""),
                // A line holds both words, the other way round.
                row(
                    "a.md",
                    10,
                    Rank::Words,
                    "[budget] for the [frame]",
                    "Late frames"
                ),
                // The words are under one heading, on two lines.
                row("b.md", 7, Rank::Spread, "A [frame] is drawn.", "Reading"),
            ]
        );
    }

    #[test]
    fn accents_do_not_count_and_a_slip_is_forgiven_when_nothing_answers() {
        assert_eq!(
            found("accents", "cafe"),
            [row(
                "a.md",
                9,
                Rank::Phrase,
                "Measure the [café] queue",
                "Late frames"
            )]
        );
        assert_eq!(
            found("slips", "nothign"),
            [row(
                "a.md",
                5,
                Rank::Close,
                "[nothing] here",
                "Frame budget"
            )]
        );
        // Nearly is not offered beside what answers.
        assert!(
            found("exact", "nothing")
                .iter()
                .all(|hit| hit.2 == Rank::Phrase)
        );
        assert!(found("none", "fox again").is_empty());
    }

    #[test]
    fn only_the_best_are_kept() {
        let folder = std::env::temp_dir().join(format!("zinmd-limit-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        // Enough files to be shared out among the cores.
        let files: Vec<_> = (0..120)
            .map(|nth| {
                let file = folder.join(format!("{nth:03}.md"));
                let text = match nth {
                    70 => "# The needle\n",
                    _ => "a needle in the text\n",
                };
                std::fs::write(&file, text).unwrap();
                file
            })
            .collect();
        let hits = run(&files, "needle", 3, &AtomicBool::new(false));
        let names: Vec<_> = hits
            .iter()
            .map(|hit| hit.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["070.md", "000.md", "001.md"]);
        // A search that was given up finds nothing more.
        assert!(run(&files, "needle", 3, &AtomicBool::new(true)).is_empty());
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn the_marker_of_a_line_is_left_out() {
        assert_eq!(marker("plain words"), 0);
        assert_eq!(&"## A heading"[marker("## A heading")..], "A heading");
        assert_eq!(&"  - item"[marker("  - item")..], "item");
        assert_eq!(&"12. item"[marker("12. item")..], "item");
        assert_eq!(&"> - [x] done"[marker("> - [x] done")..], "done");
        // What only looks like a marker is words.
        assert_eq!(marker("-dash"), 0);
        assert_eq!(marker("2024 was long"), 0);
        assert_eq!(marker("*emphasis* first"), 0);
    }

    #[test]
    fn shortens_long_lines_around_the_match() {
        let line = format!("- {}needle{}", "a ".repeat(200), " b".repeat(200));
        let (shown, marks) = excerpt(&line, &[402..408]);
        assert!(shown.starts_with('…') && shown.ends_with('…'));
        assert!(shown.chars().count() <= 98);
        // It starts at a word, with a little of what comes before.
        assert!(shown.starts_with("…a a a ") && shown.contains("a needle b"));
        assert_eq!(marks.len(), 1);
        assert_eq!(&shown[marks[0].clone()], "needle");
        // A mark that runs off the end of what is shown is cut with it.
        let (shown, marks) = excerpt(&line, &[402..408, 450..line.len()]);
        assert_eq!(marks.len(), 2);
        assert!(
            shown[marks[1].clone()].ends_with('b') && marks[1].end + '…'.len_utf8() == shown.len()
        );
    }
}
