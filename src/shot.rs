//! Developer tool, only built with `--features shot`.
//!
//! `ZWINMD_SHOT=out.png` saves what the window draws to a PNG, then quits. This
//! needs no screen-recording permission and works with the window hidden.
//! `ZWINMD_SIZE=1420x888` sets the window size, `ZWINMD_THEME=dark|light` the appearance.

use std::time::Duration;

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::*;

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

pub fn after_first_frame(window: AnyWindowHandle, cx: &mut App) {
    let Ok(path) = std::env::var("ZWINMD_SHOT") else { return };
    cx.spawn(async move |cx| {
        // Let background work (file tree, image decoding) land first.
        cx.background_executor().timer(Duration::from_millis(700)).await;
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
