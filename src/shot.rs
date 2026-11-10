//! Developer tool, only built with `--features shot`.
//!
//! `ZWINMD_SHOT=out.png` saves what the window draws to a PNG, then quits. This
//! needs no screen-recording permission and works with the window hidden.
//! `ZWINMD_SIZE=1420x888` sets the window size, `ZWINMD_THEME=dark|light` the appearance.
//! `ZWINMD_SCRIPT="type:Hello;save;editor:600"` drives the app first; the steps
//! are listed at `Workspace::debug_step`.

use std::time::Duration;

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::*;

use crate::workspace::Workspace;

pub fn window_size(cx: &mut App) -> Size<Pixels> {
    match std::env::var("ZWINMD_THEME").as_deref() {
        Ok("dark") => Theme::change(ThemeMode::Dark, None, cx),
        Ok("light") => Theme::change(ThemeMode::Light, None, cx),
        _ => {}
    }
    std::env::var("ZWINMD_SIZE")
        .ok()
        .and_then(|value| {
            let (w, h) = value.split_once('x')?;
            Some(size(px(w.parse().ok()?), px(h.parse().ok()?)))
        })
        .unwrap_or(size(px(1280.), px(820.)))
}

pub fn after_first_frame(window: AnyWindowHandle, workspace: Entity<Workspace>, cx: &mut App) {
    let Ok(path) = std::env::var("ZWINMD_SHOT") else {
        return;
    };
    cx.spawn(async move |cx| {
        // Let background work (file tree, line map) land first.
        cx.background_executor()
            .timer(Duration::from_millis(400))
            .await;
        // Run the script one step at a time, giving each a few frames to settle.
        for step in std::env::var("ZWINMD_SCRIPT")
            .unwrap_or_default()
            .split(';')
            .filter(|step| !step.is_empty())
        {
            let step = step.to_string();
            cx.update_window(window, |_, window, cx| {
                workspace.update(cx, |workspace, cx| workspace.debug_step(&step, window, cx))
            })
            .ok();
            cx.background_executor()
                .timer(Duration::from_millis(150))
                .await;
        }
        cx.background_executor()
            .timer(Duration::from_millis(600))
            .await;
        cx.update_window(window, |_, window, cx| {
            window.refresh();
            match window.render_to_image() {
                Ok(image) => match image.save(&path) {
                    Ok(()) => eprintln!("saved {path} ({}x{})", image.width(), image.height()),
                    Err(error) => eprintln!("could not save {path}: {error}"),
                },
                Err(error) => eprintln!("could not render the window: {error}"),
            }
            cx.quit();
        })
        .ok();
    })
    .detach();
}
