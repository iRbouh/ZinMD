//! Developer tool, only built with `--features shot`.
//!
//! `ZWINMD_SHOT=out.png` saves what the window draws to a PNG, then quits. This
//! needs no screen-recording permission and works with the window hidden.
//! `ZWINMD_SIZE=1420x888` sets the window size, `ZWINMD_THEME=dark|light` the appearance.
//! `ZWINMD_SCRIPT="type:Hello;save;editor:600"` drives the app first; the steps
//! are listed at `Workspace::debug_step`.

use std::time::Duration;

use gpui_kit::component::ThemeMode;
use gpui_kit::*;

use crate::workspace::Workspace;

pub fn window_size(cx: &mut App) -> Size<Pixels> {
    match std::env::var("ZWINMD_THEME").as_deref() {
        Ok("dark") => crate::theme::set_mode(ThemeMode::Dark, false, cx),
        Ok("light") => crate::theme::set_mode(ThemeMode::Light, false, cx),
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

/// The steps of a script that stand for the user's hands: `move:x,y`,
/// `click:x,y`, `rclick:x,y` and `press:x,y` (button down, not yet up), in
/// window points, and `key:<keystroke>`.
fn pointer_step(step: &str) -> Option<Vec<PlatformInput>> {
    let (command, at) = step.split_once(':')?;
    // `key:escape`, `key:enter`, `key:cmd-s`: a key press, as the keyboard sends it.
    if command == "key" {
        // The parser wants text that lives for good; a script step may as well.
        let keystroke = Keystroke::parse(at.to_string().leak()).ok()?;
        return Some(vec![PlatformInput::KeyDown(KeyDownEvent {
            keystroke,
            is_held: false,
            prefer_character_input: false,
        })]);
    }
    let button = match command {
        "move" => None,
        "click" | "press" => Some(MouseButton::Left),
        "rclick" => Some(MouseButton::Right),
        _ => return None,
    };
    let (x, y) = at.split_once(',')?;
    let position = point(px(x.parse().ok()?), px(y.parse().ok()?));
    let modifiers = Modifiers::default();
    let mut events = vec![PlatformInput::MouseMove(MouseMoveEvent {
        position,
        pressed_button: None,
        modifiers,
    })];
    if let Some(button) = button {
        events.push(PlatformInput::MouseDown(MouseDownEvent {
            button,
            position,
            modifiers,
            click_count: 1,
            first_mouse: false,
        }));
        if command != "press" {
            events.push(PlatformInput::MouseUp(MouseUpEvent {
                button,
                position,
                modifiers,
                click_count: 1,
            }));
        }
    }
    Some(events)
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
        // Run the script one step at a time, giving each time to settle.
        // `ZWINMD_PAUSE` sets how long; `ZWINMD_SETTLE` how long to wait before
        // the picture, so a short one catches an animation midway.
        let millis = |name: &str, default: u64| {
            Duration::from_millis(
                std::env::var(name)
                    .ok()
                    .and_then(|ms| ms.parse().ok())
                    .unwrap_or(default),
            )
        };
        let script = std::env::var("ZWINMD_SCRIPT").unwrap_or_default();
        for step in script.split(';').filter(|step| !step.is_empty()) {
            let step = step.to_string();
            eprintln!("step {step}");
            cx.update_window(window, |_, window, cx| match pointer_step(&step) {
                // The pointer acts on the window as a whole, as the real one does.
                Some(events) => {
                    for event in events {
                        window.dispatch_event(event, cx);
                    }
                    window.refresh();
                }
                None => {
                    workspace.update(cx, |workspace, cx| workspace.debug_step(&step, window, cx))
                }
            })
            .ok();
            cx.background_executor()
                .timer(millis("ZWINMD_PAUSE", 150))
                .await;
        }
        cx.background_executor()
            .timer(millis("ZWINMD_SETTLE", 600))
            .await;
        cx.update_window(window, |_, _, cx| {
            workspace.update(cx, |workspace, _| workspace.report_frames())
        })
        .ok();
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
