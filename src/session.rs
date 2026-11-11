//! Where the user left off: the windows, the folders in the side panel, the
//! open files and the layout. Written as things change and read back at the
//! next launch, so the app opens the way it was closed.

use std::path::PathBuf;

use serde_json::{Value, json};

/// A folder of the side panel, and how it was left.
#[derive(Clone, Debug, PartialEq)]
pub struct FolderState {
    pub path: PathBuf,
    pub pinned: bool,
    /// Whether its files were listed under its name.
    pub open: bool,
    /// The subfolders that were open.
    pub open_dirs: Vec<PathBuf>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Session {
    /// The window's place and size on screen while reading: x, y, width, height.
    pub page: Option<[f32; 4]>,
    /// The same for the editor, which is a wider window.
    pub desk: Option<[f32; 4]>,
    /// The folders of the side panel, in the order they were shown.
    pub folders: Vec<FolderState>,
    /// The files that were open on their own, outside any of the folders.
    pub files: Vec<PathBuf>,
    /// The file that was on show.
    pub active: Option<PathBuf>,
    /// "editor", "split" or "preview".
    pub view: String,
    pub sidebar_open: bool,
    pub sidebar_width: f32,
    /// Whether the outline at the foot of the side panel was showing its headings.
    pub outline_open: bool,
    /// The editor's share of the document area in split view.
    pub split: f32,
    pub scroll_sync: bool,
    /// How far down the side panel was scrolled.
    pub panel_scroll: f32,
    /// How far each file has been read, 0 to 1: the furthest point reached.
    pub progress: Vec<(PathBuf, f32)>,
    /// The other papers that were open, each a window of its own: the file
    /// it showed, and where its window was (x, y, width, height).
    pub papers: Vec<(PathBuf, [f32; 4])>,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            page: None,
            desk: None,
            folders: Vec::new(),
            files: Vec::new(),
            active: None,
            view: "split".into(),
            sidebar_open: true,
            sidebar_width: 264.,
            outline_open: true,
            split: 0.5,
            scroll_sync: true,
            panel_scroll: 0.,
            progress: Vec::new(),
            papers: Vec::new(),
        }
    }
}

impl Session {
    /// What another paper starts from: the same folders and ways, and no
    /// files of its own yet.
    pub fn for_another_paper(&self) -> Self {
        Self {
            files: Vec::new(),
            active: None,
            papers: Vec::new(),
            ..self.clone()
        }
    }
}

/// How many files' reading progress is kept. The most recently read stay.
pub const PROGRESS_KEPT: usize = 300;

impl Session {
    pub fn load() -> Self {
        let text = std::fs::read_to_string(data_dir().join("session.json")).unwrap_or_default();
        Self::parse(&text)
    }

    fn parse(text: &str) -> Self {
        let mut session = Self::default();
        let Ok(json) = serde_json::from_str::<Value>(text) else {
            return session;
        };
        let number = |key: &str| json[key].as_f64().map(|value| value as f32);
        let path = |value: &Value| value.as_str().map(PathBuf::from);
        let paths = |value: &Value| -> Vec<PathBuf> {
            value
                .as_array()
                .map(|list| list.iter().filter_map(path).collect())
                .unwrap_or_default()
        };
        let window = |key: &str| {
            let window: Vec<f32> = json[key]
                .as_array()?
                .iter()
                .filter_map(|value| value.as_f64())
                .map(|value| value as f32)
                .collect();
            // A window too small to use is not worth coming back to.
            (window.len() == 4 && window[2] >= 400. && window[3] >= 300.)
                .then(|| [window[0], window[1], window[2], window[3]])
        };

        session.page = window("page");
        // Before there was a page to read, the one window was the editor's.
        session.desk = window("desk").or_else(|| window("window"));
        session.folders = json["folders"]
            .as_array()
            .map(|folders| {
                folders
                    .iter()
                    .filter_map(|folder| {
                        Some(FolderState {
                            path: path(&folder["path"])?,
                            pinned: folder["pinned"].as_bool().unwrap_or(false),
                            open: folder["open"].as_bool().unwrap_or(true),
                            open_dirs: paths(&folder["open_dirs"]),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        // Sessions written when the panel showed a single folder.
        if let Some(root) = path(&json["root"]).filter(|_| json["folders"].is_null()) {
            session.folders.push(FolderState {
                path: root,
                pinned: false,
                open: true,
                open_dirs: Vec::new(),
            });
        }
        session.files = match json["files"].is_null() {
            true => paths(&json["tabs"]),
            false => paths(&json["files"]),
        };
        session.active = path(&json["active"]).or_else(|| {
            let shown = json["active"].as_u64()? as usize;
            session.files.get(shown).cloned()
        });
        if let Some(view) = json["view"]
            .as_str()
            .filter(|view| ["editor", "split", "preview"].contains(view))
        {
            session.view = view.to_string();
        }
        session.sidebar_open = json["sidebar_open"]
            .as_bool()
            .unwrap_or(session.sidebar_open);
        session.sidebar_width = number("sidebar_width")
            .unwrap_or(session.sidebar_width)
            .clamp(200., 480.);
        session.outline_open = json["outline_open"].as_bool().unwrap_or(true);
        session.split = number("split").unwrap_or(session.split).clamp(0.2, 0.8);
        session.scroll_sync = json["scroll_sync"].as_bool().unwrap_or(session.scroll_sync);
        session.panel_scroll = number("panel_scroll").unwrap_or(0.).max(0.);
        session.progress = json["progress"]
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|entry| {
                        let read = entry["read"].as_f64()? as f32;
                        Some((path(&entry["path"])?, read.clamp(0., 1.)))
                    })
                    .take(PROGRESS_KEPT)
                    .collect()
            })
            .unwrap_or_default();
        session.papers = json["papers"]
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|paper| {
                        let frame: Vec<f32> = paper["frame"]
                            .as_array()?
                            .iter()
                            .filter_map(|value| value.as_f64().map(|value| value as f32))
                            .collect();
                        let frame = <[f32; 4]>::try_from(frame).ok()?;
                        Some((path(&paper["path"])?, frame))
                    })
                    .collect()
            })
            .unwrap_or_default();
        session
    }

    fn to_json(&self) -> String {
        let folders: Vec<Value> = self
            .folders
            .iter()
            .map(|folder| {
                json!({
                    "path": folder.path,
                    "pinned": folder.pinned,
                    "open": folder.open,
                    "open_dirs": folder.open_dirs,
                })
            })
            .collect();
        let progress: Vec<Value> = self
            .progress
            .iter()
            .map(|(path, read)| json!({ "path": path, "read": read }))
            .collect();
        let papers: Vec<Value> = self
            .papers
            .iter()
            .map(|(path, frame)| json!({ "path": path, "frame": frame }))
            .collect();
        let json = json!({
            "page": self.page,
            "desk": self.desk,
            "folders": folders,
            "files": self.files,
            "active": self.active,
            "view": self.view,
            "sidebar_open": self.sidebar_open,
            "sidebar_width": self.sidebar_width,
            "outline_open": self.outline_open,
            "split": self.split,
            "scroll_sync": self.scroll_sync,
            "panel_scroll": self.panel_scroll,
            "progress": progress,
            "papers": papers,
        });
        serde_json::to_string_pretty(&json).unwrap_or_default()
    }

    /// Write the session to disk. Failing to is not worth interrupting the user for.
    pub fn save(&self) {
        let folder = data_dir();
        std::fs::create_dir_all(&folder).ok();
        std::fs::write(folder.join("session.json"), self.to_json()).ok();
    }
}

/// Where the app keeps what it remembers. `ZINMD_DATA` points it elsewhere,
/// so tests and scripted runs leave the real settings alone.
pub fn data_dir() -> PathBuf {
    if let Some(folder) = std::env::var_os("ZINMD_DATA") {
        return PathBuf::from(folder);
    }
    crate::system::data_dir()
}

#[cfg(test)]
mod tests {
    use super::{FolderState, Session};
    use std::path::PathBuf;

    #[test]
    fn round_trips() {
        let session = Session {
            page: Some([400., 40., 640., 905.]),
            desk: Some([120., 80., 1200., 800.]),
            folders: vec![
                FolderState {
                    path: PathBuf::from("/notes"),
                    pinned: true,
                    open: true,
                    open_dirs: vec![PathBuf::from("/notes/api")],
                },
                FolderState {
                    path: PathBuf::from("/blog posts"),
                    pinned: false,
                    open: false,
                    open_dirs: Vec::new(),
                },
            ],
            files: vec![PathBuf::from("/tmp/a.md"), PathBuf::from("/tmp/b c.md")],
            active: Some(PathBuf::from("/notes/api/overview.md")),
            view: "preview".into(),
            sidebar_open: false,
            sidebar_width: 300.,
            outline_open: false,
            split: 0.4,
            scroll_sync: false,
            panel_scroll: 96.,
            progress: vec![(PathBuf::from("/notes/a.md"), 0.5)],
            papers: vec![(PathBuf::from("/notes/b.md"), [40., 60., 620., 880.])],
        };
        assert_eq!(Session::parse(&session.to_json()), session);
    }

    #[test]
    fn falls_back_to_defaults() {
        assert_eq!(Session::parse(""), Session::default());
        assert_eq!(Session::parse("[1, 2"), Session::default());
        // Values out of range are brought back in; an unusable window is dropped.
        let odd = Session::parse(
            r#"{"view": "sideways", "split": 7, "sidebar_width": 5, "page": [0, 0, 10, 10]}"#,
        );
        assert_eq!(
            (odd.view.as_str(), odd.split, odd.sidebar_width, odd.page),
            ("split", 0.8, 200., None)
        );
    }

    #[test]
    fn reads_a_session_from_before_folders() {
        let old = Session::parse(
            r#"{"window": [10, 20, 1280, 820], "root": "/notes",
                "tabs": ["/notes/a.md", "/notes/b.md"], "active": 1}"#,
        );
        assert_eq!(old.desk, Some([10., 20., 1280., 820.]));
        assert_eq!(old.folders.len(), 1);
        assert_eq!(old.folders[0].path, PathBuf::from("/notes"));
        assert_eq!(old.active, Some(PathBuf::from("/notes/b.md")));
    }
}
