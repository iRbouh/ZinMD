//! The folders in the side panel.
//!
//! A folder is scanned once for its Markdown files, off the main thread. What
//! the panel lists is then worked out from that list alone, which is what lets
//! it leave out folders with nothing to read in them, count the files in a
//! folder that is shut, and show a chain of folders that hold only each other
//! as one row.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use gpui_kit::SharedString;

/// One row under a folder's name: a subfolder, or a file.
#[derive(Clone, Debug)]
pub struct Row {
    pub path: PathBuf,
    /// What is shown. A file loses its `.md`; a chain of folders that hold
    /// only each other reads `guides / advanced`. Empty for a row that is
    /// still being named.
    pub name: SharedString,
    /// For a file deeper than the panel indents: the way down to it, shown
    /// faintly after its name (`auth / sessions`). The name comes first, so
    /// it is still there to read when the way is too long for the row.
    pub way: SharedString,
    pub is_dir: bool,
    /// 0 for what sits directly in the folder, 1 for what is inside one of its subfolders.
    pub depth: usize,
    /// For subfolders: whether its files are listed below it.
    pub open: bool,
    /// For subfolders: how many Markdown files are inside.
    pub count: usize,
}

/// A folder the user added to the panel.
#[derive(Clone, Debug)]
pub struct Folder {
    pub root: PathBuf,
    pub name: SharedString,
    /// Pinned folders stay above the others.
    pub pinned: bool,
    /// Whether its files are listed under its name.
    pub open: bool,
    /// Every Markdown file inside, sorted. Empty until the folder has been scanned.
    pub files: Vec<PathBuf>,
    /// The subfolders whose files are listed.
    pub open_dirs: HashSet<PathBuf>,
    /// What is listed under the name right now.
    pub rows: Vec<Row>,
}

impl Folder {
    pub fn new(root: PathBuf) -> Self {
        let name = root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.to_string_lossy().into_owned());
        Self {
            root,
            name: name.into(),
            pinned: false,
            open: true,
            files: Vec::new(),
            open_dirs: HashSet::new(),
            rows: Vec::new(),
        }
    }

    pub fn contains(&self, path: &Path) -> bool {
        path.starts_with(&self.root)
    }

    /// Take the result of a scan.
    pub fn set_files(&mut self, files: Vec<PathBuf>) {
        self.files = files;
        self.list();
    }

    /// Work out the rows again, after the files or the open subfolders changed.
    pub fn list(&mut self) {
        self.rows = list(&self.root, &self.files, &self.open_dirs);
    }

    /// Open or shut the subfolder at `path`.
    pub fn toggle_dir(&mut self, path: &Path) {
        if !self.open_dirs.remove(path) {
            self.open_dirs.insert(path.to_path_buf());
        }
        self.list();
    }

    /// Open whatever is shut on the way to `path`, so its row is listed.
    /// Returns whether anything had to be opened.
    pub fn reveal(&mut self, path: &Path) -> bool {
        let mut opened = !self.open;
        self.open = true;
        let dir = self
            .rows
            .iter()
            .find(|row| row.is_dir && path.starts_with(&row.path))
            .map(|row| row.path.clone());
        if let Some(dir) = dir {
            opened |= self.open_dirs.insert(dir);
        }
        if opened {
            self.list();
        }
        opened
    }
}

/// The rows of a folder: subfolders first, then its own files, both by name.
fn list(root: &Path, files: &[PathBuf], open_dirs: &HashSet<PathBuf>) -> Vec<Row> {
    let key = |name: &std::ffi::OsStr| name.to_string_lossy().to_lowercase();
    let mut own: Vec<&PathBuf> = Vec::new();
    // Subfolders by their first step down, in the order they are shown.
    let mut inside: BTreeMap<String, (PathBuf, Vec<&PathBuf>)> = BTreeMap::new();
    for file in files {
        let Ok(rest) = file.strip_prefix(root) else {
            continue;
        };
        let mut steps = rest.components();
        let Some(first) = steps.next() else { continue };
        if steps.next().is_none() {
            own.push(file);
        } else {
            inside
                .entry(key(first.as_os_str()))
                .or_insert_with(|| (root.join(first), Vec::new()))
                .1
                .push(file);
        }
    }

    let mut rows = Vec::new();
    for (mut dir, held) in inside.into_values() {
        let mut name = dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        // A folder that holds nothing but one folder is shown together with it.
        while let Some(only) = only_step(&dir, &held) {
            name = format!("{name} / {}", only.to_string_lossy());
            dir.push(only);
        }
        let open = open_dirs.contains(&dir);
        rows.push(Row {
            path: dir.clone(),
            name: name.into(),
            way: SharedString::default(),
            is_dir: true,
            depth: 0,
            open,
            count: held.len(),
        });
        if !open {
            continue;
        }
        // Everything inside is listed one step in, however deep it lies: a
        // deeper file says where it is instead of being indented further.
        let mut files: Vec<(bool, String, Row)> = held
            .iter()
            .filter_map(|file| {
                let rest = file.strip_prefix(&dir).ok()?;
                let way: Vec<String> = rest
                    .parent()
                    .into_iter()
                    .flat_map(Path::components)
                    .map(|step| step.as_os_str().to_string_lossy().into_owned())
                    .collect();
                let nested = !way.is_empty();
                let way = way.join(" / ");
                Some((
                    nested,
                    rest.to_string_lossy().to_lowercase(),
                    Row {
                        path: (*file).clone(),
                        name: display_name(file).into(),
                        way: way.into(),
                        is_dir: false,
                        depth: 1,
                        open: false,
                        count: 0,
                    },
                ))
            })
            .collect();
        files.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
        rows.extend(files.into_iter().map(|(.., row)| row));
    }

    own.sort_by_cached_key(|file| file.file_name().map(key));
    rows.extend(own.into_iter().map(|file| Row {
        path: file.clone(),
        name: display_name(file).into(),
        way: SharedString::default(),
        is_dir: false,
        depth: 0,
        open: false,
        count: 0,
    }));
    rows
}

/// The one folder that everything in `held` lies in, directly under `dir`:
/// `None` when `dir` holds a file of its own, or more than one folder.
fn only_step<'a>(dir: &Path, held: &[&'a PathBuf]) -> Option<&'a std::ffi::OsStr> {
    let mut only = None;
    for file in held {
        let mut steps = file.strip_prefix(dir).ok()?.components();
        let first = steps.next()?.as_os_str();
        steps.next()?;
        match only {
            None => only = Some(first),
            Some(seen) if seen == first => {}
            Some(_) => return None,
        }
    }
    only
}

/// A file's name as the panel shows it: without `.md`, which every file has.
/// Any other extension is kept, since it tells the file apart.
pub fn display_name(path: &Path) -> String {
    let plain = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"));
    let name = if plain {
        path.file_stem()
    } else {
        path.file_name()
    };
    name.map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Every Markdown file under `root`, sorted. Stops at `limit` files, so a
/// huge folder cannot stall the background thread that runs this.
pub fn scan(root: &Path, limit: usize) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut folders = vec![root.to_path_buf()];
    'walk: while let Some(folder) = folders.pop() {
        for (path, is_dir) in read_dir(&folder) {
            if is_dir {
                folders.push(path);
            } else {
                found.push(path);
                if found.len() >= limit {
                    break 'walk;
                }
            }
        }
    }
    found.sort();
    found
}

/// The Markdown files and the folders directly inside `dir`, hidden and build
/// folders left out. The flag says which are folders.
fn read_dir(dir: &Path) -> Vec<(PathBuf, bool)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                return None;
            }
            // `file_type` does not follow symlinks; ask again for those so linked folders work.
            let file_type = entry.file_type().ok()?;
            let is_dir = if file_type.is_symlink() {
                entry.path().is_dir()
            } else {
                file_type.is_dir()
            };
            let keep = if is_dir {
                !IGNORED_DIRS.contains(&name.as_str())
            } else {
                is_markdown(Path::new(&name))
            };
            keep.then(|| (entry.path(), is_dir))
        })
        .collect()
}

/// Build folders that are never worth walking into.
pub const IGNORED_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    "vendor",
    "__pycache__",
];

pub fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            ["md", "markdown", "mdx", "mdown", "mkd"]
                .iter()
                .any(|known| ext.eq_ignore_ascii_case(known))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(folder: &Folder) -> Vec<String> {
        folder
            .rows
            .iter()
            .map(|row| {
                let count = match row.is_dir && !row.open {
                    true => format!(" ({})", row.count),
                    false => String::new(),
                };
                let way = match row.way.is_empty() {
                    true => String::new(),
                    false => format!(" in {}", row.way),
                };
                format!("{}{}{way}{count}", "  ".repeat(row.depth), row.name)
            })
            .collect()
    }

    #[test]
    fn lists_only_what_there_is_to_read() {
        let root = std::env::temp_dir().join(format!("zinmd-files-{}", std::process::id()));
        for dir in [
            "docs/deep/er",
            "guides/advanced",
            "empty/also",
            "node_modules",
        ] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        for file in [
            "b.md",
            "A.markdown",
            "notes.txt",
            ".hidden.md",
            "docs/one.md",
            "docs/deep/two.MD",
            "docs/deep/er/three.md",
            "guides/advanced/tips.md",
            "node_modules/readme.md",
        ] {
            std::fs::write(root.join(file), "x").unwrap();
        }

        let mut folder = Folder::new(root.clone());
        folder.set_files(scan(&root, 5000));
        // Folders first, with how many files they hold; a folder with nothing
        // to read is not there; a chain is one row; `.md` is not shown.
        assert_eq!(
            names(&folder),
            ["docs (3)", "guides / advanced (1)", "A.markdown", "b"]
        );

        // Opening a subfolder lists everything in it one step in: its own
        // files first, deeper ones after, saying where they are.
        folder.toggle_dir(&root.join("docs"));
        assert_eq!(
            names(&folder),
            [
                "docs",
                "  one",
                "  three in deep / er",
                "  two in deep",
                "guides / advanced (1)",
                "A.markdown",
                "b"
            ]
        );

        // Revealing a file opens what is shut on the way, once.
        folder.open = false;
        let tips = root.join("guides/advanced/tips.md");
        assert!(folder.reveal(&tips));
        assert!(folder.open);
        assert!(names(&folder).contains(&"  tips".to_string()));
        assert!(!folder.reveal(&tips));

        // A new file shows up on the next scan, and open subfolders stay open.
        std::fs::write(root.join("docs/zed.md"), "x").unwrap();
        folder.set_files(scan(&root, 5000));
        assert!(names(&folder).contains(&"  zed".to_string()));

        folder.toggle_dir(&root.join("docs"));
        assert!(names(&folder).contains(&"docs (4)".to_string()));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn keeps_an_extension_that_is_not_md() {
        assert_eq!(display_name(Path::new("/a/notes.md")), "notes");
        assert_eq!(display_name(Path::new("/a/Notes.MD")), "Notes");
        assert_eq!(display_name(Path::new("/a/notes.mdx")), "notes.mdx");
        assert_eq!(display_name(Path::new("/a/v1.2 plan.md")), "v1.2 plan");
    }
}
