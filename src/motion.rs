//! Motion: how things move, kept in one place so the whole app moves alike.
//!
//! Everything eases out on one curve: quick off the mark, soft on arrival.
//! Movement is short, so it reads as response rather than as animation, and
//! the kit drops it altogether when the system asks for reduced motion.

use std::time::{Duration, Instant};

use gpui_kit::base::Transition;
use gpui_kit::base::animation::cubic_bezier;
use gpui_kit::*;

use crate::theme::Palette;

/// Small things: the view switch, the palette appearing.
pub const QUICK: Duration = Duration::from_millis(160);
/// Whole panels: the sidebar, the panes.
pub const PANEL: Duration = Duration::from_millis(240);

/// Hover colour fading in and out.
pub const HOVER: Duration = Duration::from_millis(120);

/// How far along something is that began at `started` and takes `duration`:
/// 0 to 1, eased. Already 1 when the system asks for reduced motion.
pub fn progress(started: Instant, duration: Duration, cx: &App) -> f32 {
    if cx.reduce_motion() {
        return 1.;
    }
    ease((started.elapsed().as_secs_f32() / duration.as_secs_f32()).min(1.))
}

/// The curve everything moves on.
pub fn ease(t: f32) -> f32 {
    cubic_bezier(0.22, 1., 0.36, 1.)(t)
}

/// A transition of `duration`. While the user is dragging there is none: a
/// divider has to stay under the pointer, not catch up with it.
pub fn glide(duration: Duration, cx: &App) -> Transition {
    let duration = if cx.has_active_drag() {
        Duration::ZERO
    } else {
        duration
    };
    Transition::new(duration).ease(ease)
}

/// Shown under the pointer while a divider is dragged: nothing.
pub struct DragGhost;

impl Render for DragGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

/// A vertical divider that can be dragged, centred on `x` in its parent.
/// Dragging it carries `payload`; whoever lays the panes out listens for that.
pub fn resize_handle<T: 'static>(
    id: &'static str,
    x: Pixels,
    payload: T,
    p: &Palette,
) -> Stateful<Div> {
    div()
        .id(id)
        .group(id)
        .absolute()
        .top_0()
        .bottom_0()
        .left(x - px(4.))
        .w(px(8.))
        .flex()
        .justify_center()
        .cursor_col_resize()
        // A hairline lights up under the pointer, to show the divider can be taken.
        .child(
            div()
                .w(px(2.))
                .h_full()
                .bg(p.accent)
                .opacity(0.)
                .group_hover(id, |style| style.opacity(0.6)),
        )
        .on_drag(payload, |_, _, _, cx| cx.new(|_| DragGhost))
}
