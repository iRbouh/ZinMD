//! Matching what is typed against text: forgiving, and still instant.
//!
//! Three things make a search useful without making it slow:
//!
//! - case and accents are ignored, so `cafe` finds `Café`;
//! - several words are found in any order, when they are not found as typed,
//!   as long as they are in the same line or paragraph;
//! - when nothing is found, a slip of the finger is forgiven: `pgae` finds
//!   `page`, and `recieve` finds `receive`.
//!
//! None of it needs an index or a model: it is a walk through the text.

use std::ops::Range;

use unicode_normalization::UnicodeNormalization as _;
use unicode_normalization::char::is_combining_mark;

/// Text with its case and accents taken off, which remembers where in the
/// original each of its letters came from.
#[derive(Default)]
pub struct Folded {
    letters: Vec<char>,
    /// For each letter, the bytes of the original it stands for.
    spans: Vec<(usize, usize)>,
}

/// Letters `from..to` of folded text.
type Run = (usize, usize);

impl Folded {
    pub fn new(text: &str) -> Self {
        let mut folded = Self::default();
        folded.fill(text);
        folded
    }

    /// Fold other text into this, in the room already made: searching a file
    /// line by line makes room once.
    pub fn fill(&mut self, text: &str) {
        self.letters.clear();
        self.spans.clear();
        if text.is_ascii() {
            // Most lines are plain letters: one letter a byte, and nothing
            // to take off but the case.
            let lowered = text.bytes().map(|byte| byte.to_ascii_lowercase() as char);
            self.letters.extend(lowered);
            self.spans.extend((0..text.len()).map(|at| (at, at + 1)));
            return;
        }
        for (at, letter) in text.char_indices() {
            let span = (at, at + letter.len_utf8());
            if letter.is_ascii() {
                self.letters.push(letter.to_ascii_lowercase());
                self.spans.push(span);
                continue;
            }
            // An accented letter comes apart into the letter and its accent;
            // the accent is dropped.
            for part in std::iter::once(letter)
                .nfd()
                .filter(|part| !is_combining_mark(*part))
            {
                for lower in part.to_lowercase() {
                    self.letters.push(lower);
                    self.spans.push(span);
                }
            }
        }
    }

    /// Where a run of letters is in the original.
    fn place(&self, (from, to): Run) -> Range<usize> {
        self.spans[from].0..self.spans[to - 1].1
    }

    /// The first letter at or after `from`, and before `to`, where `wanted`
    /// is.
    fn next(&self, wanted: &[char], from: usize, to: usize) -> Option<usize> {
        let (first, last) = (*wanted.first()?, to.checked_sub(wanted.len())?);
        (from..=last).find(|at| {
            self.letters[*at] == first && self.letters[*at..*at + wanted.len()] == *wanted
        })
    }

    /// Every place `wanted` is, one after another: places do not overlap.
    fn find(&self, wanted: &[char]) -> Vec<Range<usize>> {
        let mut found = Vec::new();
        let mut from = 0;
        while let Some(at) = self.next(wanted, from, self.letters.len()) {
            from = at + wanted.len();
            found.push(self.place((at, from)));
        }
        found
    }

    /// Whether a letter is part of a word.
    fn in_word(&self, at: usize) -> bool {
        self.letters
            .get(at)
            .is_some_and(|letter| letter.is_alphanumeric())
    }

    /// The lines of the text, as runs of letters.
    fn lines(&self) -> impl Iterator<Item = Run> + '_ {
        let mut from = 0;
        std::iter::from_fn(move || {
            (from <= self.letters.len()).then(|| {
                let to = self.letters[from..]
                    .iter()
                    .position(|letter| *letter == '\n')
                    .map_or(self.letters.len(), |at| from + at);
                let line = (from, to);
                from = to + 1;
                line
            })
        })
    }

    /// The words between two letters: each run of letters and digits.
    fn words(&self, from: usize, to: usize) -> impl Iterator<Item = Run> + '_ {
        let mut at = from;
        std::iter::from_fn(move || {
            while at < to && !self.in_word(at) {
                at += 1;
            }
            let start = at;
            while at < to && self.in_word(at) {
                at += 1;
            }
            (at > start).then_some((start, at))
        })
    }

    /// Where `word` begins a word between two letters. A word of one or two
    /// letters has to be the whole word: `a` is not in every word with an a.
    fn as_word(&self, word: &[char], (from, to): Run, found: &mut Vec<Run>) {
        let mut at = from;
        while let Some(start) = self.next(word, at, to) {
            let end = start + word.len();
            let begins = start == 0 || !self.in_word(start - 1) || !self.in_word(start);
            if begins && (word.len() > 2 || !self.in_word(end)) {
                found.push((start, end));
            }
            at = end;
        }
    }

    /// Each word between two letters that is `word` with a slip of the finger.
    fn nearly(&self, word: &[char], (from, to): Run, rows: &mut Rows, found: &mut Vec<Run>) {
        for (start, end) in self.words(from, to) {
            if close(&self.letters[start..end], word, rows) {
                found.push((start, end));
            }
        }
    }

    /// Where each of the words is in a line. Nothing is added, and it is
    /// `false`, unless every word is there: with `near`, there or nearly.
    fn every_word(
        &self,
        wanted: &Wanted,
        line: Run,
        near: bool,
        rows: &mut Rows,
        found: &mut Vec<Run>,
    ) -> bool {
        let kept = found.len();
        for word in &wanted.words {
            let before = found.len();
            self.as_word(word, line, found);
            if near && found.len() == before {
                self.nearly(word, line, rows, found);
            }
            if found.len() == before {
                found.truncate(kept);
                return false;
            }
        }
        true
    }
}

/// What was typed, folded the same way: the whole of it, and its words.
pub struct Wanted {
    whole: Vec<char>,
    words: Vec<Vec<char>>,
}

impl Wanted {
    pub fn new(typed: &str) -> Self {
        let whole = Folded::new(typed.trim()).letters;
        let words = whole
            .split(|letter| letter.is_whitespace())
            .filter(|word| !word.is_empty())
            .map(<[char]>::to_vec)
            .collect();
        Self { whole, words }
    }

    pub fn is_empty(&self) -> bool {
        self.whole.is_empty()
    }
}

/// How what was found answers what was typed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    /// Just as typed.
    Exact,
    /// Each of the words, but not together or not in that order.
    Words,
    /// Nothing as typed: words that are nearly it.
    Close,
}

/// Room kept between lines, so that a long search makes it once.
#[derive(Default)]
pub struct Room {
    rows: Rows,
    found: Vec<Run>,
}

/// Every place in `text` that answers `wanted`, in order, and how well. The
/// best kind of answer there is wins: nearly-right words are offered only
/// when nothing is right.
pub fn search(text: &Folded, wanted: &Wanted) -> (Vec<Range<usize>>, Answer) {
    let exact = text.find(&wanted.whole);
    if !exact.is_empty() || wanted.is_empty() {
        return (exact, Answer::Exact);
    }
    let mut room = Room::default();
    for (near, answer) in [(false, Answer::Words), (true, Answer::Close)] {
        // One word that is not there as typed is not there as a word either.
        if !near && wanted.words.len() < 2 {
            continue;
        }
        room.found.clear();
        for line in text.lines() {
            text.every_word(wanted, line, near, &mut room.rows, &mut room.found);
        }
        if !room.found.is_empty() {
            // In the order they come in the text, without any that overlap.
            room.found.sort_unstable();
            let mut end = 0;
            room.found.retain(|(from, to)| {
                let clear = *from >= end;
                if clear {
                    end = *to;
                }
                clear
            });
            let places = room.found.iter().map(|run| text.place(*run)).collect();
            return (places, answer);
        }
    }
    (Vec::new(), Answer::Close)
}

/// Whether a line of text answers `wanted`: where its first match is, and how
/// well it answers. With `near`, words that are nearly right count, for a
/// line with nothing better.
pub fn line_answers(
    line: &Folded,
    wanted: &Wanted,
    near: bool,
    room: &mut Room,
) -> Option<(usize, Answer)> {
    let all = (0, line.letters.len());
    if let Some(at) = line.next(&wanted.whole, 0, all.1) {
        return Some((line.spans[at].0, Answer::Exact));
    }
    room.found.clear();
    let answer = if wanted.words.len() > 1
        && line.every_word(wanted, all, false, &mut room.rows, &mut room.found)
    {
        Answer::Words
    } else if near && line.every_word(wanted, all, true, &mut room.rows, &mut room.found) {
        Answer::Close
    } else {
        return None;
    };
    let first = room.found.iter().map(|run| run.0).min()?;
    Some((line.spans[first].0, answer))
}

/// How well a short name, such as a file's, answers what was typed. Lower is
/// better; `None` is not at all. With it, the bytes of the name to mark.
pub fn name_answers(name: &Folded, wanted: &Wanted) -> Option<(u8, Option<Range<usize>>)> {
    let all = (0, name.letters.len());
    if let Some(at) = name.next(&wanted.whole, 0, all.1) {
        // At the start of the name, then at the start of a word in it, then
        // anywhere.
        let rank = match at {
            0 => 0,
            _ if !name.in_word(at - 1) => 1,
            _ => 2,
        };
        return Some((rank, Some(name.place((at, at + wanted.whole.len())))));
    }
    let mut room = Room::default();
    if wanted.words.len() > 1
        && name.every_word(wanted, all, false, &mut room.rows, &mut room.found)
    {
        return Some((3, None));
    }
    // The letters in order, with others between: `fg` for `field-guide`.
    let typed = || wanted.whole.iter().filter(|letter| !letter.is_whitespace());
    let mut rest = name.letters.iter();
    if typed().count() >= 2 && typed().all(|letter| rest.any(|other| other == letter)) {
        return Some((4, None));
    }
    name.every_word(wanted, all, true, &mut room.rows, &mut room.found)
        .then_some((5, None))
}

/// The rows of the sum that counts slips.
#[derive(Default)]
struct Rows {
    before: Vec<usize>,
    last: Vec<usize>,
    row: Vec<usize>,
}

/// Whether `word` is `wanted` with a slip of the finger: a letter wrong,
/// missing or extra, or two letters swapped. A long word is allowed two
/// slips, a short one none: three letters with one wrong is another word.
/// A slip in the first letter counts twice, as it is the one people get
/// right. The start of a longer word counts, so `pgae` finds `pages`.
fn close(word: &[char], wanted: &[char], rows: &mut Rows) -> bool {
    let mut slips = match wanted.len() {
        0..=3 => return false,
        4..=7 => 1,
        _ => 2,
    };
    if word.first() != wanted.first() {
        slips -= 1;
    }
    if slips == 0 || word.len() + slips < wanted.len() {
        return false;
    }
    // As much of the start of the word as could be what was meant.
    let longest = word.len().min(wanted.len() + slips);
    let shortest = wanted.len() - slips;
    let width = wanted.len() + 1;
    rows.before.clear();
    rows.before.resize(width, 0);
    rows.last.clear();
    rows.last.extend(0..width);
    for (i, x) in word[..longest].iter().enumerate() {
        rows.row.clear();
        rows.row.resize(width, i + 1);
        for (j, y) in wanted.iter().enumerate() {
            // A letter changed, added or dropped.
            let mut least = (rows.last[j] + usize::from(x != y))
                .min(rows.last[j + 1] + 1)
                .min(rows.row[j] + 1);
            // Two neighbours swapped.
            if i > 0 && j > 0 && *x == wanted[j - 1] && word[i - 1] == *y {
                least = least.min(rows.before[j - 1] + 1);
            }
            rows.row[j + 1] = least;
        }
        std::mem::swap(&mut rows.before, &mut rows.last);
        std::mem::swap(&mut rows.last, &mut rows.row);
        if i + 1 >= shortest && rows.last[wanted.len()] <= slips {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(text: &'static str, typed: &str) -> (Vec<&'static str>, Answer) {
        let (places, answer) = search(&Folded::new(text), &Wanted::new(typed));
        let pieces = places.into_iter().map(|place| &text[place]).collect();
        (pieces, answer)
    }

    #[test]
    fn case_and_accents_are_ignored() {
        assert_eq!(
            found("The page. THE end", "the"),
            (vec!["The", "THE"], Answer::Exact)
        );
        assert_eq!(
            found("Un café à Évian", "cafe a evian"),
            (vec!["café à Évian"], Answer::Exact)
        );
        assert_eq!(
            found("naïve Straße", "NAIVE"),
            (vec!["naïve"], Answer::Exact)
        );
    }

    #[test]
    fn places_do_not_overlap_and_nothing_finds_nothing() {
        assert_eq!(found("aaaa", "aa").0, vec!["aa", "aa"]);
        assert!(found("anything", "").0.is_empty());
        assert!(found("short", "longer than the text").0.is_empty());
        assert!(found("", "word").0.is_empty());
    }

    #[test]
    fn words_are_found_in_any_order_in_the_same_paragraph() {
        let text = "the page is judged\nthe reading of a page is slow\nreading alone";
        assert_eq!(
            found(text, "reading page"),
            (vec!["reading", "page"], Answer::Words)
        );
        // As typed wins when it is there.
        assert_eq!(found("a reading page", "reading page").1, Answer::Exact);
        // A word begins a word, and a very short one is the whole word.
        assert_eq!(
            found("a pagan read a page", "page a").0,
            vec!["a", "a", "page"]
        );
        assert!(found("the stage, the readings", "age read").0.is_empty());
    }

    #[test]
    fn a_slip_of_the_finger_is_forgiven_when_nothing_is_right() {
        assert_eq!(
            found("the page turns", "pgae"),
            (vec!["page"], Answer::Close)
        );
        assert_eq!(found("two pages", "pgae"), (vec!["pages"], Answer::Close));
        assert_eq!(
            found("to receive it", "recieve"),
            (vec!["receive"], Answer::Close)
        );
        assert_eq!(
            found("the budget of a frame", "frame bugdet"),
            (vec!["budget", "frame"], Answer::Close)
        );
        // Two slips in a long word, one of them in its first letter.
        assert_eq!(
            found("the performance", "berformance").0,
            vec!["performance"]
        );
        // A short word with a letter wrong is another word, and so is one
        // with its first letter wrong.
        assert!(found("the cat sat", "cot").0.is_empty());
        assert!(found("the page", "cage").0.is_empty());
        // What is right is not made fuzzy: `page` does not find `pane`.
        assert_eq!(
            found("page and pane", "page"),
            (vec!["page"], Answer::Exact)
        );
    }

    #[test]
    fn names_are_ranked_by_how_well_they_answer() {
        let rank = |name: &str, typed: &str| {
            name_answers(&Folded::new(name), &Wanted::new(typed)).map(|(rank, _)| rank)
        };
        assert_eq!(rank("field-guide", "field"), Some(0));
        assert_eq!(rank("field-guide", "guide"), Some(1));
        assert_eq!(rank("field-guide", "uid"), Some(2));
        assert_eq!(rank("field-guide", "guide field"), Some(3));
        assert_eq!(rank("field-guide", "fg"), Some(4));
        assert_eq!(rank("field-guide", "giude"), Some(5));
        assert_eq!(rank("field-guide", "roadmap"), None);
        // The part to mark is given in the bytes of the name itself.
        let marked = name_answers(&Folded::new("Résumé d'été"), &Wanted::new("ete"));
        assert_eq!(marked, Some((1, Some(11..16))));
    }

    #[test]
    fn a_line_answers_whole_then_by_words_then_nearly() {
        let line = Folded::new("  Frame time must stay under budget");
        let mut room = Room::default();
        let mut answers =
            |typed: &str, near| line_answers(&line, &Wanted::new(typed), near, &mut room);
        assert_eq!(answers("frame time", false), Some((2, Answer::Exact)));
        assert_eq!(answers("budget frame", false), Some((2, Answer::Words)));
        assert_eq!(answers("budget missing", false), None);
        assert_eq!(answers("bugdet", false), None);
        assert_eq!(answers("bugdet", true), Some((29, Answer::Close)));
        // Nearly is only for a line with nothing better.
        assert_eq!(answers("budget frame", true), Some((2, Answer::Words)));
    }
}
