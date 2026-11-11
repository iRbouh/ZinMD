//! Links between notes: `[[wiki links]]`, and a glimpse of where a link
//! leads.
//!
//! A note names another by its file name between two pairs of brackets:
//! `[[Roadmap]]`, `[[Roadmap|the plan]]`, `[[Roadmap#Next year]]`, or
//! `[[#A heading]]` for a heading of its own. Before a page is drawn, each
//! one that names a file the app knows is swapped for an ordinary Markdown
//! link to that file, which the page then shows and follows as it does any
//! link. One that names nothing is left as it was written. Nothing else in
//! the text changes and no line is added or taken away, so a line of the
//! page is still the same line of the source.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use crate::doc;
use crate::files;

/// The text of a document as its page shows it: with its wiki links made
/// into ordinary links.
pub struct Shown {
    pub text: String,
    /// For each link that was swapped in: where it ends in `text`, and how
    /// much further on the source is from there.
    moved: Vec<(usize, isize)>,
}

impl Shown {
    /// Give up the text, once the page has it: what is kept says where its
    /// places are in the source.
    pub fn without_text(mut self) -> Self {
        self.text = String::new();
        self
    }

    /// Where a place in the shown text is in the source it was made from.
    /// It is exact outside the links that were swapped in.
    pub fn in_source(&self, at: usize) -> usize {
        let before = self.moved.partition_point(|(end, _)| *end <= at);
        match before.checked_sub(1) {
            Some(last) => (at as isize + self.moved[last].1) as usize,
            None => at,
        }
    }
}

/// `text` with each wiki link that `leads` somewhere made into an ordinary
/// link, or `None` when there is none. `leads` is given the name between the
/// brackets, and answers with where the link goes.
pub fn linked(text: &str, leads: impl Fn(&str) -> Option<String>) -> Option<Shown> {
    // Most documents have none: one look says so.
    if !text.contains("[[") {
        return None;
    }
    let mut shown = String::with_capacity(text.len() + 64);
    let mut moved = Vec::new();
    // How much of the source has been copied, and where the line begins.
    let (mut copied, mut start) = (0, 0);
    let mut fence: Option<(u8, usize)> = None;
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let begins = start;
        start += line.len();
        // Code is shown as it is written.
        let indent = body.len() - body.trim_start_matches(' ').len();
        let rest = &body[indent..];
        let run = |mark: u8| rest.bytes().take_while(|byte| *byte == mark).count();
        if let Some((mark, length)) = fence {
            if indent < 4 && run(mark) >= length && rest[run(mark)..].trim().is_empty() {
                fence = None;
            }
            continue;
        }
        fence = (*b"`~")
            .into_iter()
            .map(|mark| (mark, run(mark)))
            .find(|(_, length)| indent < 4 && *length >= 3);
        if fence.is_some() {
            continue;
        }
        let bytes = body.as_bytes();
        let mut at = 0;
        while at < bytes.len() {
            match bytes[at] {
                // What a backslash is before is meant as written.
                b'\\' => at += 2,
                b'`' => {
                    let ticks = bytes[at..].iter().take_while(|byte| **byte == b'`').count();
                    let closing = "`".repeat(ticks);
                    let mut from = at + ticks;
                    // The same number of ticks closes it; more or fewer do not.
                    at = loop {
                        let Some(found) = body[from..].find(&closing).map(|found| from + found)
                        else {
                            break at + ticks;
                        };
                        let after = bytes[found..]
                            .iter()
                            .take_while(|byte| **byte == b'`')
                            .count();
                        if after == ticks {
                            break found + ticks;
                        }
                        from = found + after;
                    };
                }
                b'[' if bytes.get(at + 1) == Some(&b'[')
                    && at.checked_sub(1).is_none_or(|before| bytes[before] != b'!') =>
                {
                    let inner = body[at + 2..]
                        .find("]]")
                        .map(|length| &body[at + 2..at + 2 + length]);
                    match inner.and_then(|inner| Some((inner, link(inner, &leads)?))) {
                        Some((inner, link)) => {
                            shown.push_str(&text[copied..begins + at]);
                            shown.push_str(&link);
                            at += inner.len() + 4;
                            copied = begins + at;
                            moved.push((shown.len(), copied as isize - shown.len() as isize));
                        }
                        None => at += 2,
                    }
                }
                _ => at += 1,
            }
        }
    }
    if moved.is_empty() {
        return None;
    }
    shown.push_str(&text[copied..]);
    Some(Shown { text: shown, moved })
}

/// The ordinary link that what is between the brackets of a wiki link stands
/// for, if it leads somewhere: `name`, `name|words to show`, `name#heading`.
fn link(inner: &str, leads: &impl Fn(&str) -> Option<String>) -> Option<String> {
    if inner.contains(['[', ']', '\n', '<', '>']) {
        return None;
    }
    let (target, words) = match inner.split_once('|') {
        Some((target, words)) => (target, Some(words.trim())),
        None => (inner, None),
    };
    let (name, heading) = match target.split_once('#') {
        Some((name, heading)) => (name.trim(), Some(heading.trim())),
        None => (target.trim(), None),
    };
    let heading = heading.filter(|heading| !heading.is_empty());
    let mut place = match name.is_empty() {
        // A heading of this very document.
        true => String::new(),
        false => leads(name)?,
    };
    if let Some(heading) = heading {
        place.push('#');
        place.push_str(&doc::anchor(heading));
    }
    // What is shown: the words given, or the name of the note without the
    // folders on the way to it, and the heading.
    let named = name.rsplit('/').next().unwrap_or(name);
    let words = match (words.filter(|words| !words.is_empty()), heading) {
        (Some(words), _) => words.to_string(),
        (None, Some(heading)) if name.is_empty() => heading.to_string(),
        (None, Some(heading)) => format!("{named} › {heading}"),
        (None, None) => named.to_string(),
    };
    (!place.is_empty() && !words.is_empty()).then(|| format!("[{words}](<{place}>)"))
}

/// The files a wiki link can name, by their names.
pub struct Known<'a> {
    /// Each name, in lower case and without `.md`, and the files that have it.
    named: HashMap<String, Vec<&'a Path>>,
}

impl<'a> Known<'a> {
    pub fn new(files: impl IntoIterator<Item = &'a Path>) -> Self {
        let mut named: HashMap<String, Vec<&'a Path>> = HashMap::new();
        for file in files {
            let name = files::display_name(file).to_lowercase();
            let held = named.entry(name).or_default();
            if !held.contains(&file) {
                held.push(file);
            }
        }
        Self { named }
    }

    /// The file that `name` is the name of, for a note in the folder `from`:
    /// `Roadmap`, `roadmap.md`, or `plans/Roadmap` to tell two apart. Of
    /// several, the one in the note's own folder, then the one whose path is
    /// shortest.
    pub fn find(&self, name: &str, from: Option<&Path>) -> Option<&'a Path> {
        let steps: Vec<String> = name
            .split('/')
            .map(|step| step.trim().to_lowercase())
            .filter(|step| !step.is_empty())
            .collect();
        let (last, folders) = steps.split_last()?;
        let last = last.strip_suffix(".md").unwrap_or(last);
        // The folders named on the way must be the ones the file is in.
        let within = |file: &Path| {
            let mut above = file.parent().into_iter().flat_map(Path::components).rev();
            folders.iter().rev().all(|folder| {
                above.next().is_some_and(|step| {
                    step.as_os_str().to_string_lossy().to_lowercase() == *folder
                })
            })
        };
        self.named
            .get(last)?
            .iter()
            .copied()
            .filter(|file| within(file))
            .min_by_key(|file| (file.parent() != from, file.as_os_str().len()))
    }
}

/// The way from the folder `from` to `file`, as a link in a note there says
/// it: `Roadmap.md`, `plans/Roadmap.md`, `../Roadmap.md`.
pub fn way(from: Option<&Path>, file: &Path) -> String {
    let Some(from) = from else {
        return file.to_string_lossy().into_owned();
    };
    let (here, there): (Vec<Component>, Vec<Component>) =
        (from.components().collect(), file.components().collect());
    let shared = here.iter().zip(&there).take_while(|(a, b)| a == b).count();
    let mut way = PathBuf::new();
    for _ in shared..here.len() {
        way.push("..");
    }
    way.extend(&there[shared..]);
    // Links are written with forward slashes on every system.
    way.to_string_lossy().replace('\\', "/")
}

/// What is at the end of a link, said in a few words: for the card that
/// shows when the pointer rests on it.
#[derive(Clone, Debug, PartialEq)]
pub struct Glimpse {
    /// The file, as the panel names it. Empty for a heading of the document
    /// the link is in.
    pub file: String,
    /// The heading the link leads to, or the title of the file.
    pub title: String,
    /// The first sentences found there.
    pub about: String,
}

/// How much of a note a glimpse of it reads, and how much it says.
const GLIMPSE_READS: u64 = 512 * 1024;
const GLIMPSE_SAYS: usize = 220;

/// A glimpse of where `url` leads, when that is a Markdown file or a heading:
/// `folder` is that of the document the link is in, and `own` its text.
pub fn glimpse(url: &str, folder: Option<&Path>, own: &str) -> Option<Glimpse> {
    let (path, anchor) = url.split_once('#').unwrap_or((url, ""));
    let anchor = crate::open::percent_decode(anchor).to_lowercase();
    let (file, text) = match path.is_empty() {
        true => (String::new(), std::borrow::Cow::Borrowed(own)),
        false => {
            // Something with a scheme is not a file of ours.
            if path.contains("://") || path.starts_with("mailto:") {
                return None;
            }
            let path = PathBuf::from(crate::open::percent_decode(path));
            let path = match (path.is_absolute(), folder) {
                (false, Some(folder)) => folder.join(path),
                _ => path,
            };
            if !files::is_markdown(&path) {
                return None;
            }
            let size = std::fs::metadata(&path).ok()?.len();
            if size > GLIMPSE_READS {
                return None;
            }
            let text = std::fs::read_to_string(&path).ok()?;
            (files::display_name(&path), std::borrow::Cow::Owned(text))
        }
    };
    let headings = doc::outline(&text);
    let (title, from, to) = match anchor.is_empty() {
        // The whole file: its title is its first heading, or its name.
        true => {
            let title = headings.first().map(|heading| heading.title.to_string());
            (title.unwrap_or_else(|| file.clone()), 0, None)
        }
        false => {
            let at = headings
                .iter()
                .position(|heading| doc::anchor(&heading.title) == anchor)?;
            let next = headings.get(at + 1).map(|heading| heading.line);
            let heading = &headings[at];
            (heading.title.to_string(), heading.line + 1, next)
        }
    };
    let about = doc::excerpt(&text, from, to, GLIMPSE_SAYS);
    Some(Glimpse { file, title, about })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(text: &str) -> Option<String> {
        let leads = |name: &str| match name {
            "Missing" => None,
            name => Some(format!("{name}.md")),
        };
        linked(text, leads).map(|shown| shown.text)
    }

    #[test]
    fn wiki_links_become_ordinary_links() {
        assert_eq!(
            shown("See [[Roadmap]] and [[Road map|the plan]]."),
            Some("See [Roadmap](<Roadmap.md>) and [the plan](<Road map.md>).".into())
        );
        assert_eq!(
            shown("[[Roadmap#Next year]] [[#A heading]] [[plans/Roadmap]]"),
            Some(
                "[Roadmap › Next year](<Roadmap.md#next-year>) [A heading](<#a-heading>) \
                 [Roadmap](<plans/Roadmap.md>)"
                    .into()
            )
        );
    }

    #[test]
    fn what_leads_nowhere_and_what_is_code_stay_as_written() {
        assert_eq!(shown("Nothing here, [not one], [[Missing]]."), None);
        assert_eq!(
            shown("An embed ![[Roadmap]] and an escape \\[[Roadmap]]"),
            None
        );
        assert_eq!(shown("`[[Roadmap]]` and ``a ` [[Roadmap]]``"), None);
        assert_eq!(
            shown("```\n[[Roadmap]]\n```\n[[Roadmap]]\n~~~\n[[Roadmap]]\n~~~\n"),
            Some("```\n[[Roadmap]]\n```\n[Roadmap](<Roadmap.md>)\n~~~\n[[Roadmap]]\n~~~\n".into())
        );
        // Brackets that are never closed, and empty ones.
        assert_eq!(shown("[[Roadmap and [[]] and [[|x]]"), None);
    }

    #[test]
    fn places_after_a_link_are_found_again_in_the_source() {
        let source = "- [ ] [[Roadmap]] first\n- [x] [[Missing]] then [[A|b]] last\n";
        let shown = linked(source, |name| {
            (name != "Missing").then(|| format!("{name}.md"))
        })
        .unwrap();
        // The same lines, and what is not a link is found where it was.
        assert_eq!(shown.text.lines().count(), source.lines().count());
        for words in ["- [ ]", "first", "- [x]", "then", "last"] {
            let at = shown.text.find(words).unwrap();
            assert_eq!(shown.in_source(at), source.find(words).unwrap(), "{words}");
        }
    }

    #[test]
    fn a_name_finds_its_file() {
        let files = [
            "/n/Roadmap.md",
            "/n/plans/Roadmap.md",
            "/n/plans/Ideas.md",
            "/n/a/b/Deep.md",
        ]
        .map(PathBuf::from);
        let known = Known::new(files.iter().map(PathBuf::as_path));
        let find = |name: &str, from: &str| {
            known
                .find(name, Some(Path::new(from)))
                .map(|file| file.to_string_lossy().into_owned())
        };
        // Whatever its case, with or without `.md`.
        assert_eq!(find("ideas", "/n"), Some("/n/plans/Ideas.md".into()));
        assert_eq!(find("Ideas.md", "/n"), Some("/n/plans/Ideas.md".into()));
        // Of two, the one beside the note; a folder tells them apart.
        assert_eq!(find("Roadmap", "/n"), Some("/n/Roadmap.md".into()));
        assert_eq!(
            find("Roadmap", "/n/plans"),
            Some("/n/plans/Roadmap.md".into())
        );
        assert_eq!(
            find("plans/Roadmap", "/n"),
            Some("/n/plans/Roadmap.md".into())
        );
        assert_eq!(find("b/Deep", "/n"), Some("/n/a/b/Deep.md".into()));
        assert_eq!(find("c/Deep", "/n"), None);
        assert_eq!(find("Nowhere", "/n"), None);
    }

    #[test]
    fn the_way_to_a_file_is_said_from_the_note() {
        let way = |from: &str, file: &str| way(Some(Path::new(from)), Path::new(file));
        assert_eq!(way("/n", "/n/Roadmap.md"), "Roadmap.md");
        assert_eq!(way("/n", "/n/plans/Road map.md"), "plans/Road map.md");
        assert_eq!(way("/n/plans", "/n/Roadmap.md"), "../Roadmap.md");
        assert_eq!(way("/n/a/b", "/n/plans/Ideas.md"), "../../plans/Ideas.md");
    }

    #[test]
    fn a_glimpse_says_what_is_at_the_end_of_a_link() {
        let folder = std::env::temp_dir().join(format!("zinmd-glimpse-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let note = "# The plan\n\nShip the reader first.\n\n## Next year\n\nThen the rest of it.\n";
        std::fs::write(folder.join("Road map.md"), note).unwrap();
        let own = "# Here\n\n## A heading\n\nWords under it.\n";
        let see = |url: &str| glimpse(url, Some(&folder), own);

        let whole = see("Road%20map.md").unwrap();
        assert_eq!(
            (
                whole.file.as_str(),
                whole.title.as_str(),
                whole.about.as_str()
            ),
            // A short opening is read on, into what follows.
            (
                "Road map",
                "The plan",
                "Ship the reader first. Then the rest of it."
            )
        );
        let part = see("Road map.md#next-year").unwrap();
        assert_eq!(
            (part.title.as_str(), part.about.as_str()),
            ("Next year", "Then the rest of it.")
        );
        let here = see("#a-heading").unwrap();
        assert_eq!(
            (here.file.as_str(), here.title.as_str(), here.about.as_str()),
            ("", "A heading", "Words under it.")
        );
        // Not ours to look into.
        assert_eq!(see("https://example.com/page.md"), None);
        assert_eq!(see("picture.png"), None);
        assert_eq!(see("missing.md"), None);
        assert_eq!(see("#no-such-heading"), None);
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
