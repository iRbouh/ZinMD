//! ZinMD: a fast native Markdown reader and editor.
//!
//! Usage: `zinmd [file.md | folder]`

// The theme is one large `json!` literal, which expands recursively.
#![recursion_limit = "512"]
// On Windows a shipped build is a window and nothing else: no console behind it.
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod assets;
mod doc;
mod export;
mod files;
mod matching;
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
mod system;
mod theme;
mod ui;
mod update;
mod watch;
mod workspace;

use std::path::PathBuf;

use gpui_kit::*;

use crate::theme::size;
use crate::workspace::Workspace;

fn main() {
    // `zinmd --version` says which ZinMD this is and leaves, opening nothing.
    if std::env::args_os()
        .nth(1)
        .is_some_and(|argument| argument == "--version" || argument == "-V")
    {
        println!("ZinMD {}", system::VERSION);
        return;
    }
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
        // The window opens as a page: the shape of a sheet of paper.
        #[cfg(not(feature = "shot"))]
        let window_bounds = remembered_bounds(&session, cx);
        #[cfg(feature = "shot")]
        let window_bounds = Bounds::centered(None, shot::window_size(cx), cx);

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(window_bounds)),
            window_min_size: Some(gpui_kit::size(px(380.), px(420.))),
            titlebar: Some(TitlebarOptions {
                title: None,
                appears_transparent: true,
                // Centres the window buttons in our taller title bar.
                traffic_light_position: Some(point(px(14.), (size::TITLEBAR - px(16.)) / 2.)),
            }),
            // Our title bar moves the window itself, so clicks on it are never delayed.
            app_owns_titlebar_drag: true,
            // On Linux the bar at the top is ours too, not a second one from
            // the desktop, and the window answers to the name of its launcher.
            window_decorations: Some(WindowDecorations::Client),
            app_id: Some(system::APP_ID.into()),
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

/// Where the page was last time, if that place still exists: a display may
/// have been unplugged since. Otherwise a sheet as tall as the display has
/// room for, in the centre.
#[cfg(not(feature = "shot"))]
fn remembered_bounds(session: &session::Session, cx: &App) -> Bounds<Pixels> {
    let fallback = || {
        let room = cx
            .primary_display()
            .map_or(gpui_kit::size(px(1440.), px(900.)), |display| {
                display.visible_bounds().size
            });
        Bounds::centered(None, workspace::page_size(room), cx)
    };
    let Some([x, y, width, height]) = session.page else {
        return fallback();
    };
    // A page has the shape of a sheet with its margins, whatever shape it was
    // left in: it comes back as tall as it was, about the same centre line.
    let sheet: f32 = workspace::page_width(px(height)).into();
    let (x, width) = (x + (width - sheet) / 2., sheet);
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
