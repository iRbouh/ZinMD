//! ZwinMD: a fast native Markdown editor and viewer.
//!
//! Usage: `zwinmd [file.md | folder]`

// The theme is one large `json!` literal, which expands recursively.
#![recursion_limit = "512"]

mod assets;
mod doc;
mod export;
mod files;
mod motion;
mod open;
mod preview;
mod settings;
#[cfg(feature = "shot")]
mod shot;
mod startup;
mod sync;
mod theme;
mod ui;
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

        #[cfg(not(feature = "shot"))]
        let window_size = size(px(1280.), px(820.));
        #[cfg(feature = "shot")]
        let window_size = shot::window_size(cx);

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                window_size,
                cx,
            ))),
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
            cx.new(|cx| Workspace::new(path, started, window, cx))
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
