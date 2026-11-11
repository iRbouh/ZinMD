//! Small building blocks shared by the strips of the window.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::theme::{Palette, size};

/// A square, borderless button that holds one icon.
pub fn icon_button(id: impl Into<ElementId>, icon: IconName, p: &Palette) -> Stateful<Div> {
    let strong = p.text_primary;
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(28.))
        .rounded(size::RADIUS_SM)
        .text_color(p.text_secondary)
        .hover(move |style| style.text_color(strong))
        .child(Icon::new(icon).size(px(16.)))
        // The title bar drags the window from any press that reaches it.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

/// The small-caps label above a sidebar section.
pub fn section_label(label: &'static str, p: &Palette) -> Div {
    div()
        .text_size(size::FONT_UI_SMALL)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(p.text_tertiary)
        .child(label)
}

/// How long a copy button shows its tick.
const COPIED: std::time::Duration = std::time::Duration::from_millis(1400);

/// A button that copies `value`, big enough to hit without aiming, which
/// shows a tick for a moment to say it has. With `label` it says in words
/// what it copies; without, it is the icon alone.
///
/// `id` must tell this button from every other on the page: it is also what
/// the button remembers having been pressed by.
pub fn copy_button(
    id: impl Into<ElementId>,
    label: Option<&'static str>,
    value: impl Fn() -> String + 'static,
    p: &Palette,
    window: &mut Window,
    cx: &mut App,
) -> Stateful<Div> {
    let id: ElementId = id.into();
    let pressed = window.use_keyed_state(id.clone(), cx, |_, _| None::<std::time::Instant>);
    let copied = pressed.read(cx).is_some_and(|at| at.elapsed() < COPIED);
    let (strong, hover, done) = (p.text_primary, p.surface_hover, p.status_success);
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .gap(px(7.))
        .h(px(32.))
        .min_w(px(32.))
        .when(label.is_some(), |button| button.px(px(11.)))
        .rounded(size::RADIUS_SM)
        .text_size(size::FONT_UI)
        .text_color(if copied { done } else { p.text_secondary })
        .hover(move |style| {
            style
                .bg(hover)
                .text_color(if copied { done } else { strong })
        })
        .active(|style| style.opacity(0.7))
        .cursor_pointer()
        .child(
            Icon::new(if copied {
                IconName::Check
            } else {
                IconName::Copy
            })
            .size(px(16.)),
        )
        .children(label.map(|label| if copied { "Copied" } else { label }))
        // A press here is not the start of a selection, nor of a drag.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            cx.write_to_clipboard(ClipboardItem::new_string(value()));
            pressed.update(cx, |pressed, cx| {
                *pressed = Some(std::time::Instant::now());
                cx.notify();
            });
            // Take the tick away again once it has been seen.
            let pressed = pressed.clone();
            cx.spawn(async move |cx| {
                cx.background_executor().timer(COPIED).await;
                pressed.update(cx, |_, cx| cx.notify());
            })
            .detach();
        })
}
