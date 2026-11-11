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
mod links;
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

#[cfg(feature = "shot")]
use gpui_kit::Bounds;

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
        let window_bounds = workspace::page_bounds(session.page, cx);
        #[cfg(feature = "shot")]
        let window_bounds = Bounds::centered(None, shot::window_size(cx), cx);

        // A file double-clicked in Finder arrives as a request, not an argument.
        // If it is already here, open straight into it.
        let mut asked = requests.take_ready().into_iter();
        let path = path.or_else(|| asked.next());
        // With nothing asked for, the other papers of last time come back
        // too, each where it was.
        let papers = match path {
            Some(_) => Vec::new(),
            None => session.papers.clone(),
        };
        let beside = session.for_another_paper();
        let (window, _workspace) =
            workspace::open_paper(path, session, window_bounds, started, true, cx)
                .expect("failed to open the main window");
        startup::mark("window opened");
        for (file, frame) in papers.into_iter().filter(|(file, _)| file.is_file()) {
            let bounds = workspace::page_bounds(Some(frame), cx);
            workspace::open_paper(Some(file), beside.clone(), bounds, started, false, cx);
        }
        // The paper that was in front last time is in front again.
        window
            .update(cx, |_, window, _| window.activate_window())
            .ok();

        // Everything else that was asked for, and whatever is asked for later.
        let rest: Vec<PathBuf> = asked.collect();
        cx.spawn(async move |cx| {
            let mut paths = Some(rest);
            while let Some(batch) = paths.take() {
                cx.update(|cx| workspace::open_requested(batch, cx));
                paths = requests.next().await;
            }
        })
        .detach();

        #[cfg(feature = "shot")]
        shot::after_first_frame(window, _workspace, cx);
    });
}
