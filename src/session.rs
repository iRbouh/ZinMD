//! Where the user left off: the window, the folder, the open tabs and the
//! layout. Written as things change and read back at the next launch, so the
//! app opens the way it was closed.

use std::path::PathBuf;

use serde_json::{Value, json};

#[derive(Clone, Debug, PartialEq)]
pub struct Session {
    /// The window's place and size on screen: x, y, width, height.
    pub window: Option<[f32; 4]>,
    pub root: Option<PathBuf>,
    pub tabs: Vec<PathBuf>,
    /// Which of `tabs` was on show.
    pub active: usize,
    /// "editor", "split" or "preview".
    pub view: String,
    pub sidebar_open: bool,
    pub sidebar_width: f32,
    /// The editor's share of the document area in split view.
    pub split: f32,
    pub scroll_sync: bool,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            window: None,
            root: None,
            tabs: Vec::new(),
            active: 0,
            view: "split".into(),
            sidebar_open: true,
            sidebar_width: 248.,
            split: 0.5,
            scroll_sync: true,
        }
    }
}

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

        if let Some(window) = json["window"].as_array().filter(|window| window.len() == 4) {
            let window: Vec<f32> = window
                .iter()
                .filter_map(|value| value.as_f64())
                .map(|value| value as f32)
                .collect();
            // A window too small to use is not worth coming back to.
            if window.len() == 4 && window[2] >= 400. && window[3] >= 300. {
                session.window = Some([window[0], window[1], window[2], window[3]]);
            }
        }
        session.root = path(&json["root"]);
        session.tabs = json["tabs"]
            .as_array()
            .map(|tabs| tabs.iter().filter_map(path).collect())
            .unwrap_or_default();
        session.active = json["active"].as_u64().unwrap_or(0) as usize;
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
            .clamp(180., 480.);
        session.split = number("split").unwrap_or(session.split).clamp(0.2, 0.8);
        session.scroll_sync = json["scroll_sync"].as_bool().unwrap_or(session.scroll_sync);
        session
    }

    fn to_json(&self) -> String {
        let json = json!({
            "window": self.window,
            "root": self.root,
            "tabs": self.tabs,
            "active": self.active,
            "view": self.view,
            "sidebar_open": self.sidebar_open,
            "sidebar_width": self.sidebar_width,
            "split": self.split,
            "scroll_sync": self.scroll_sync,
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
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    home.join("Library/Application Support/ZinMD")
}

#[cfg(test)]
mod tests {
    use super::Session;
    use std::path::PathBuf;

    #[test]
    fn round_trips() {
        let session = Session {
            window: Some([120., 80., 1200., 800.]),
            root: Some(PathBuf::from("/notes")),
            tabs: vec![PathBuf::from("/notes/a.md"), PathBuf::from("/notes/b c.md")],
            active: 1,
            view: "preview".into(),
            sidebar_open: false,
            sidebar_width: 300.,
            split: 0.4,
            scroll_sync: false,
        };
        assert_eq!(Session::parse(&session.to_json()), session);
    }

    #[test]
    fn falls_back_to_defaults() {
        assert_eq!(Session::parse(""), Session::default());
        assert_eq!(Session::parse("[1, 2"), Session::default());
        // Values out of range are brought back in; an unusable window is dropped.
        let odd = Session::parse(
            r#"{"view": "sideways", "split": 7, "sidebar_width": 5, "window": [0, 0, 10, 10]}"#,
        );
        assert_eq!(
            (odd.view.as_str(), odd.split, odd.sidebar_width, odd.window),
            ("split", 0.8, 180., None)
        );
    }
}
