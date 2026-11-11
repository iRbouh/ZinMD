//! ZinMD: a fast native Markdown editor and viewer.
//!
//! Usage: `zinmd [file.md | folder]`

// The theme is one large `json!` literal, which expands recursively.
#![recursion_limit = "512"]

mod assets;
mod doc;
mod export;
mod files;
mod motion;
mod open;
mod preview;
mod search;
mod session;
mod settings;
#[cfg(feature = "shot")]
mod shot;
mod startup;
mod sync;
mod theme;
mod ui;
mod watch;
mod workspace;

use std::path::PathBuf;

use gpui_kit::*;

use crate::theme::size;
use crate::workspace::Workspace;

fn main() {
    let started = startup::begin();
    startup::warm_up_graphics();
    let path = std::env::args_os().nth(1).map(PathBuf::from);

    let app = gpui_kit::application().with_assets(assets::AppAssets);
    let mut requests = open::listen(&app);
    startup::mark("application created");
    app.run(move |cx| {
        startup::mark("event loop running");
        let settings = settings::Settings::load();
        theme::install(&settings, cx);
        cx.set_global(settings);
        gpui_kit::init(cx);
        startup::mark("component kit ready");
        theme::follow_system(cx);
        workspace::init(cx);
        startup::mark("theme and keys ready");
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        // Scripted runs stay in the background, away from the user's typing.
        #[cfg(not(feature = "shot"))]
        cx.activate(true);
        startup::mark("app activated");

        let session = session::Session::load();
        #[cfg(not(feature = "shot"))]
        let window_bounds = remembered_bounds(&session, cx);
        #[cfg(feature = "shot")]
        let window_bounds = Bounds::centered(None, shot::window_size(cx), cx);

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(window_bounds)),
            window_min_size: Some(gpui_kit::size(px(560.), px(360.))),
            titlebar: Some(TitlebarOptions {
                title: None,
                appears_transparent: true,
                // Centres the window buttons in our taller title bar.
                traffic_light_position: Some(point(px(14.), (size::TITLEBAR - px(16.)) / 2.)),
            }),
            // Our title bar moves the window itself, so clicks on it are never delayed.
            app_owns_titlebar_drag: true,
            focus: cfg!(not(feature = "shot")),
            // Scripted runs stay in the background, where frames are normally
            // held to 30 a second; lift that so they measure real smoothness.
            #[cfg(feature = "shot")]
            inactive_frame_interval: None,
            ..Default::default()
        };
        // A file double-clicked in Finder arrives as a request, not an argument.
        // If it is already here, open straight into it.
        let mut asked = requests.take_ready().into_iter();
        let path = path.or_else(|| asked.next());
        let (window, workspace) = gpui_kit::open_window(options, cx, |window, cx| {
            startup::mark("native window created");
            cx.new(|cx| Workspace::new(path, session, started, window, cx))
        })
        .expect("failed to open the main window");
        startup::mark("window opened");

        // Everything else that was asked for, and whatever is asked for later.
        let rest: Vec<PathBuf> = asked.collect();
        let opener = workspace.downgrade();
        cx.spawn(async move |cx| {
            let mut paths = Some(rest);
            while let Some(batch) = paths.take() {
                let opened = cx.update_window(window, |_, window, cx| {
                    opener
                        .update(cx, |workspace, cx| {
                            workspace.open_requested(batch, window, cx)
                        })
                        .is_ok()
                });
                if !matches!(opened, Ok(true)) {
                    break;
                }
                paths = requests.next().await;
            }
        })
        .detach();

        #[cfg(feature = "shot")]
        shot::after_first_frame(window, workspace, cx);
    });
}

/// Where the window was last time, if that place still exists: a display may
/// have been unplugged since. Otherwise a comfortable size in the centre.
#[cfg(not(feature = "shot"))]
fn remembered_bounds(session: &session::Session, cx: &App) -> Bounds<Pixels> {
    let fallback = || Bounds::centered(None, gpui_kit::size(px(1280.), px(820.)), cx);
    let Some([x, y, width, height]) = session.window else {
        return fallback();
    };
    let bounds = Bounds::new(point(px(x), px(y)), gpui_kit::size(px(width), px(height)));
    // Enough of the title bar must be on some display to grab the window by.
    let grip = Bounds::new(
        bounds.origin,
        gpui_kit::size(bounds.size.width, size::TITLEBAR),
    );
    let reachable = cx.displays().iter().any(|display| {
        let overlap = display.bounds().intersect(&grip);
        overlap.size.width >= px(120.) && overlap.size.height >= px(20.)
    });
    if reachable { bounds } else { fallback() }
}
