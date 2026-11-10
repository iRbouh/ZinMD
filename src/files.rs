//! The folder tree in the sidebar.
//!
//! The tree is kept as a flat list of the rows that are visible right now.
//! Opening a folder inserts its children after it, closing one removes them, so
//! drawing is a plain list and a folder is only read from disk when it is opened.

use std::path::{Path, PathBuf};

use gpui_kit::SharedString;

#[derive(Clone, Debug)]
pub struct Row {
    pub path: PathBuf,
    pub name: SharedString,
    pub is_dir: bool,
    /// How deep the row sits below the root; 0 is a direct child.
    pub depth: usize,
    /// For folders: whether its children are currently listed below it.
    pub open: bool,
}

#[derive(Default)]
pub struct FileTree {
    pub root: Option<PathBuf>,
    pub rows: Vec<Row>,
}

impl FileTree {
    /// Open or close the folder at `ix`.
    pub fn toggle(&mut self, ix: usize) {
        let Some(row) = self.rows.get_mut(ix) else {
            return;
        };
        if !row.is_dir {
            return;
        }
        row.open = !row.open;
        let depth = row.depth;
        if row.open {
            let children = read_dir(&row.path, depth + 1);
            self.rows.splice(ix + 1..ix + 1, children);
        } else {
            let end = self.rows[ix + 1..]
                .iter()
                .position(|r| r.depth <= depth)
                .map_or(self.rows.len(), |n| ix + 1 + n);
            self.rows.drain(ix + 1..end);
        }
    }

    /// The rows listed under the folder at `ix`. Empty for a closed folder or a file.
    pub fn descendants(&self, ix: usize) -> std::ops::Range<usize> {
        let Some(row) = self.rows.get(ix) else {
            return 0..0;
        };
        let depth = row.depth;
        let after = &self.rows[ix + 1..];
        ix + 1
            ..ix + 1
                + after
                    .iter()
                    .position(|row| row.depth <= depth)
                    .unwrap_or(after.len())
    }

    /// Read the folder again, keeping open the folders that were open.
    pub fn refresh(&mut self) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let open: Vec<PathBuf> = self
            .rows
            .iter()
            .filter(|row| row.is_dir && row.open)
            .map(|row| row.path.clone())
            .collect();
        self.rows = read_dir(&root, 0);
        let mut ix = 0;
        while ix < self.rows.len() {
            if self.rows[ix].is_dir && open.contains(&self.rows[ix].path) {
                self.toggle(ix);
            }
            ix += 1;
        }
    }

    /// Open every folder on the way to `path`, so its row is visible.
    pub fn reveal(&mut self, path: &Path) {
        let mut ix = 0;
        while ix < self.rows.len() {
            let row = &self.rows[ix];
            if row.is_dir && !row.open && path.starts_with(&row.path) {
                self.toggle(ix);
            }
            ix += 1;
        }
    }
}

/// The Markdown files and folders directly inside `dir`: folders first, then by name.
pub fn read_dir(dir: &Path, depth: usize) -> Vec<Row> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut rows: Vec<Row> = entries
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
            keep.then(|| Row {
                path: entry.path(),
                name: name.into(),
                is_dir,
                depth,
                open: false,
            })
        })
        .collect();
    rows.sort_by_cached_key(|row| (!row.is_dir, row.name.to_lowercase()));
    rows
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

    #[test]
    fn opens_and_closes_folders() {
        let root = std::env::temp_dir().join(format!("zwinmd-files-{}", std::process::id()));
        std::fs::create_dir_all(root.join("docs/deep")).unwrap();
        std::fs::create_dir_all(root.join("node_modules")).unwrap();
        for file in [
            "b.md",
            "A.markdown",
            "notes.txt",
            ".hidden.md",
            "docs/one.md",
            "docs/deep/two.MD",
        ] {
            std::fs::write(root.join(file), "x").unwrap();
        }

        let mut tree = FileTree {
            root: Some(root.clone()),
            rows: read_dir(&root, 0),
        };
        let names = |tree: &FileTree| {
            tree.rows
                .iter()
                .map(|r| format!("{}{}", "  ".repeat(r.depth), r.name))
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&tree), ["docs", "A.markdown", "b.md"]);

        tree.reveal(&root.join("docs/deep/two.MD"));
        assert_eq!(
            names(&tree),
            [
                "docs",
                "  deep",
                "    two.MD",
                "  one.md",
                "A.markdown",
                "b.md"
            ]
        );

        // A new file shows up on refresh, and open folders stay open.
        std::fs::write(root.join("docs/zed.md"), "x").unwrap();
        tree.refresh();
        assert_eq!(
            names(&tree),
            [
                "docs",
                "  deep",
                "    two.MD",
                "  one.md",
                "  zed.md",
                "A.markdown",
                "b.md"
            ]
        );

        // "docs" holds three rows, "deep" one, and a file none.
        assert_eq!(tree.descendants(0), 1..5);
        assert_eq!(tree.descendants(1), 2..3);
        assert_eq!(tree.descendants(2), 3..3);

        tree.toggle(0);
        assert_eq!(tree.descendants(0), 1..1);
        assert_eq!(names(&tree), ["docs", "A.markdown", "b.md"]);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
