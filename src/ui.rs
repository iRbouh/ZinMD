//! Small building blocks shared by the strips of the window.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::*;

use crate::theme::{Palette, size};

/// A square, borderless button that holds one icon.
pub fn icon_button(id: impl Into<ElementId>, icon: IconName, p: &Palette) -> Stateful<Div> {
    let (hover, pressed, strong) = (p.surface_hover, p.border_strong, p.text_primary);
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(28.))
        .rounded(size::RADIUS_SM)
        .text_color(p.text_secondary)
        .hover(move |style| style.bg(hover).text_color(strong))
        .active(move |style| style.bg(pressed))
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
