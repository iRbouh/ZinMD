//! Noticing when files change on disk, so an open document can follow an edit
//! made elsewhere (git, another editor) and the sidebar can show new files.
//!
//! The system reports changes on its own thread; they are passed along as
//! plain paths and the workspace decides what, if anything, they mean.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use futures::channel::mpsc::{UnboundedReceiver, unbounded};
use notify::{RecursiveMode, Watcher as _};

pub struct Watcher {
    system: notify::RecommendedWatcher,
    /// The folder shown in the sidebar, watched with everything under it.
    root: Option<PathBuf>,
    /// Folders of open documents that are outside the root.
    folders: HashSet<PathBuf>,
}

/// Start watching. Returns the watcher, to say what to watch, and the stream
/// of paths that changed.
pub fn start() -> Option<(Watcher, UnboundedReceiver<Vec<PathBuf>>)> {
    let (sender, receiver) = unbounded();
    let system = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if let Ok(event) = event {
            // Reading a file is not a change.
            if !event.kind.is_access() {
                sender.unbounded_send(event.paths).ok();
            }
        }
    })
    .ok()?;
    Some((
        Watcher {
            system,
            root: None,
            folders: HashSet::new(),
        },
        receiver,
    ))
}

impl Watcher {
    /// Watch `root` and everything under it, instead of the previous root.
    pub fn set_root(&mut self, root: Option<&Path>) {
        if self.root.as_deref() == root {
            return;
        }
        if let Some(old) = self.root.take() {
            self.system.unwatch(&old).ok();
        }
        if let Some(root) = root {
            if self.system.watch(root, RecursiveMode::Recursive).is_ok() {
                self.root = Some(root.to_path_buf());
            }
        }
    }

    /// Make sure changes to `file` are noticed, wherever it lives.
    pub fn include(&mut self, file: &Path) {
        let Some(folder) = file.parent() else { return };
        let covered = self
            .root
            .as_deref()
            .is_some_and(|root| folder.starts_with(root));
        if !covered
            && !self.folders.contains(folder)
            && self
                .system
                .watch(folder, RecursiveMode::NonRecursive)
                .is_ok()
        {
            self.folders.insert(folder.to_path_buf());
        }
    }
}

/// What a file looks like on disk, to tell whether it changed: when it was
/// last written and how long it is. `None` if it is not there.
pub fn stamp(file: &Path) -> Option<(SystemTime, u64)> {
    let metadata = std::fs::metadata(file).ok()?;
    Some((metadata.modified().ok()?, metadata.len()))
}

/// Whether a changed path could matter to the sidebar: not inside a hidden or
/// build folder, which can churn constantly and are never listed.
pub fn is_listed(path: &Path, root: &Path) -> bool {
    let Ok(inside) = path.strip_prefix(root) else {
        return false;
    };
    inside.components().all(|part| {
        let name = part.as_os_str().to_string_lossy();
        !name.starts_with('.') && !crate::files::IGNORED_DIRS.contains(&name.as_ref())
    })
}

#[cfg(test)]
mod tests {
    use super::is_listed;
    use std::path::Path;

    #[test]
    fn ignores_hidden_and_build_folders() {
        let root = Path::new("/notes");
        assert!(is_listed(Path::new("/notes/docs/a.md"), root));
        assert!(!is_listed(Path::new("/notes/.git/index"), root));
        assert!(!is_listed(Path::new("/notes/target/debug/x"), root));
        assert!(!is_listed(Path::new("/elsewhere/a.md"), root));
    }
}
