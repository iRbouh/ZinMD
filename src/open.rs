//! Files the system asks us to open: a double-click in Finder, "Open With", or
//! a document dropped on the Dock icon.
//!
//! macOS delivers these as events rather than command-line arguments, and may
//! do so before the app has finished starting or at any time while it runs.
//! They are queued here, and the app takes them when it is ready.

use std::path::PathBuf;

use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedReceiver, unbounded};
use gpui_kit::Application;

pub struct OpenRequests(UnboundedReceiver<Vec<PathBuf>>);

/// Start collecting the system's open requests. Call before the app runs.
pub fn listen(app: &Application) -> OpenRequests {
    let (sender, receiver) = unbounded();
    app.on_open_urls(move |urls| {
        let paths: Vec<PathBuf> = urls.iter().filter_map(|url| path_of(url)).collect();
        crate::startup::trace(|| format!("asked to open {paths:?}"));
        if !paths.is_empty() {
            sender.unbounded_send(paths).ok();
        }
    });
    OpenRequests(receiver)
}

impl OpenRequests {
    /// The files asked for so far, without waiting: those that started the app.
    pub fn take_ready(&mut self) -> Vec<PathBuf> {
        let mut ready = Vec::new();
        while let Ok(paths) = self.0.try_recv() {
            ready.extend(paths);
        }
        ready
    }

    /// Wait for the next request.
    pub async fn next(&mut self) -> Option<Vec<PathBuf>> {
        self.0.next().await
    }
}

/// The file a `file://` URL points at.
fn path_of(url: &str) -> Option<PathBuf> {
    let path = url.strip_prefix("file://")?;
    // A host may precede the path (`file://localhost/Users/…`); the path starts at the slash.
    let path = &path[path.find('/')?..];
    Some(PathBuf::from(percent_decode(path)))
}

/// Turn `%20` and friends back into the characters they stand for.
pub fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut ix = 0;
    while ix < bytes.len() {
        let hex = (bytes[ix] == b'%')
            .then(|| text.get(ix + 1..ix + 3))
            .flatten();
        match hex.and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
            Some(byte) => {
                decoded.push(byte);
                ix += 3;
            }
            None => {
                decoded.push(bytes[ix]);
                ix += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

#[cfg(test)]
mod tests {
    use super::{path_of, percent_decode};
    use std::path::PathBuf;

    #[test]
    fn reads_file_urls() {
        assert_eq!(
            path_of("file:///Users/me/My%20Notes/caf%C3%A9.md"),
            Some(PathBuf::from("/Users/me/My Notes/café.md"))
        );
        assert_eq!(
            path_of("file://localhost/tmp/a.md"),
            Some(PathBuf::from("/tmp/a.md"))
        );
        assert_eq!(path_of("https://example.com/a.md"), None);
    }

    #[test]
    fn decodes_percent_escapes() {
        assert_eq!(
            percent_decode("my%20notes/caf%C3%A9.md"),
            "my notes/café.md"
        );
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("a%zzb"), "a%zzb");
    }
}
