//! Small building blocks shared by the strips of the window.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
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

/// How wide `text` is when drawn at `size` in the interface font.
pub fn text_width(
    text: &SharedString,
    size: Pixels,
    weight: FontWeight,
    window: &Window,
) -> Pixels {
    let mut font = window.text_style().font();
    font.weight = weight;
    let run = TextRun {
        len: text.len(),
        font,
        color: Hsla::default(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window
        .text_system()
        .shape_line(text.clone(), size, &[run], None)
        .width
}
