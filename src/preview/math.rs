//! LaTeX math: `$inline$` and `$$display$$`.
//!
//! RaTeX parses and lays a formula out; we draw the result as vector outlines
//! tinted with the text colour, so math is sharp at any size and follows the
//! theme. Layout is cached by source, so a formula is only laid out once.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::text::{
    InlineElement, InlineRenderContext, MarkdownNode, MarkdownParseContext, MarkdownPlugin,
    markdown_ast as mdast,
};
use gpui_kit::*;

use crate::theme::{ActivePalette as _, size};

/// A formula laid out at one unit per em.
pub struct Formula {
    svg: Vec<u8>,
    width: f32,
    /// Extent above the baseline.
    height: f32,
    /// Extent below the baseline.
    depth: f32,
}

/// A laid-out formula, or why it could not be laid out.
type Laid = Arc<Result<Formula, String>>;

/// SVG units per em in `Formula::svg`.
const UNITS_PER_EM: f64 = 40.;

fn lay_out(source: &str, display: bool) -> Laid {
    static CACHE: LazyLock<Mutex<HashMap<(bool, String), Laid>>> = LazyLock::new(Default::default);
    let key = (display, source.to_string());
    if let Some(laid) = CACHE.lock().unwrap().get(&key) {
        return laid.clone();
    }
    // The math engine is young: a formula it trips over must not take the app down.
    let source = key.1.clone();
    let laid = std::panic::catch_unwind(move || build(&source, display))
        .unwrap_or_else(|_| Err("this formula could not be laid out".to_string()));
    let laid = Arc::new(laid);
    let mut cache = CACHE.lock().unwrap();
    if cache.len() >= 4096 {
        cache.clear();
    }
    cache.insert(key, laid.clone());
    laid
}

fn build(source: &str, display: bool) -> Result<Formula, String> {
    use ratex_layout::{LayoutOptions, layout, to_display_list, validate_layout};
    use ratex_types::math_style::MathStyle;

    let ast = ratex_parser::parse(source).map_err(|error| error.to_string())?;
    let style = if display {
        MathStyle::Display
    } else {
        MathStyle::Text
    };
    let boxes = layout(&ast, &LayoutOptions::default().with_style(style));
    validate_layout(&boxes).map_err(|error| error.to_string())?;
    let list = to_display_list(&boxes);
    // No padding: the image is exactly width by (height + depth) em.
    let options = ratex_svg::SvgOptions {
        font_size: UNITS_PER_EM,
        padding: 0.,
        stroke_width: 1.5,
        embed_glyphs: true,
        font_dir: String::new(),
    };
    Ok(Formula {
        svg: ratex_svg::render_to_svg(&list, &options).into_bytes(),
        width: list.width as f32,
        height: list.height as f32,
        depth: list.depth as f32,
    })
}

impl Formula {
    /// The formula drawn for text of `font_size`, in `color`.
    fn element(&self, font_size: Pixels, color: Hsla) -> Svg {
        svg()
            .data(&self.svg)
            .flex_none()
            .w(font_size * self.width)
            .h(font_size * (self.height + self.depth))
            .text_color(color)
    }

    /// The formula as part of a line of text, its baseline on the line's.
    fn inline(&self, font_size: Pixels, color: Hsla) -> InlineElement {
        let ascent = font_size * self.height;
        let height = font_size * (self.height + self.depth);
        // The kit only accepts a baseline inside the element, and falls back to
        // showing the source otherwise. A formula with nothing below its baseline
        // (`E = mc^2`) has the two equal, where rounding decides; the extra
        // pixel keeps the baseline safely inside.
        let frame = div()
            .flex_none()
            .h(height + px(1.))
            .child(self.element(font_size, color));
        InlineElement::new(frame).with_baseline(ascent)
    }
}

/// A formula as HTML for export: the same outlines, sized in ems so they scale
/// with the text and coloured like it. `None` if the formula cannot be laid out.
pub fn html(source: &str, display: bool) -> Option<String> {
    let laid = lay_out(source, display);
    let formula = laid.as_ref().as_ref().ok()?;
    let svg = String::from_utf8_lossy(&formula.svg);
    // Our own size and colour replace the ones the outlines were drawn with.
    let body = svg
        .split_once('>')
        .map(|(_, rest)| rest)?
        .replace(['\n', '\r'], " ")
        .replace("rgba(0,0,0,1)", "currentColor");
    let view_box = format!(
        "0 0 {} {}",
        formula.width as f64 * UNITS_PER_EM,
        (formula.height + formula.depth) as f64 * UNITS_PER_EM
    );
    let style = format!(
        "width:{:.3}em;height:{:.3}em;vertical-align:{:.3}em",
        formula.width,
        formula.height + formula.depth,
        -formula.depth
    );
    Some(format!(
        r#"<svg class="math" xmlns="http://www.w3.org/2000/svg" viewBox="{view_box}" style="{style}" fill="currentColor">{body}"#
    ))
}

/// The source of a formula that failed, shown where the formula would be.
fn broken(source: &str, cx: &App) -> Div {
    div()
        .font_family(cx.theme().mono_font_family.clone())
        .text_size(size::FONT_EDITOR)
        .text_color(cx.palette().status_danger)
        .child(source.to_string())
}

pub struct InlineMath;

impl MarkdownPlugin for InlineMath {
    fn name(&self) -> &str {
        "math-inline"
    }

    fn parse(&self, node: &mdast::Node, _: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let mdast::Node::InlineMath(math) = node else {
            return None;
        };
        Some(MarkdownNode::new("math-inline", lay_out(&math.value, false)).text(math.value.clone()))
    }

    fn render_inline(
        &self,
        node: &MarkdownNode,
        context: &InlineRenderContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<InlineElement> {
        let laid = node.data::<Laid>()?;
        let font_size = context.font_size();
        Some(match laid.as_ref() {
            Ok(formula) => formula.inline(font_size, window.text_style().color),
            Err(_) => InlineElement::new(broken(&format!("${}$", node.as_text()), cx)),
        })
    }
}

pub struct BlockMath;

impl MarkdownPlugin for BlockMath {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        "math-block"
    }

    fn parse(&self, node: &mdast::Node, _: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let mdast::Node::Math(math) = node else {
            return None;
        };
        Some(MarkdownNode::new("math-block", lay_out(&math.value, true)).text(math.value.clone()))
    }

    fn render(&self, node: &MarkdownNode, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let style = window.text_style();
        // Display math reads best a touch larger than the text around it.
        let font_size = style.font_size.to_pixels(window.rem_size()) * 1.15;
        let row = div().w_full().flex().justify_center().py(px(4.));
        match node.data::<Laid>().map(|laid| laid.as_ref()) {
            Some(Ok(formula)) => row.child(formula.element(font_size, style.color)),
            Some(Err(error)) => row.justify_start().child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(broken(node.as_text(), cx))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(cx.palette().text_tertiary)
                            .child(error.clone()),
                    ),
            ),
            None => row,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::lay_out;

    #[test]
    fn lays_out_common_formulas() {
        for source in [
            "E = mc^2",
            r"\frac{a}{b}",
            r"\sqrt{x^2 + y^2}",
            r"\sum_{i=1}^{n} i",
            r"t_f = \frac{1000}{120}",
            r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}",
            r"\int_{-\infty}^{\infty} e^{-x^2}\,dx = \sqrt{\pi}",
        ] {
            let laid = lay_out(source, false);
            let formula = laid
                .as_ref()
                .as_ref()
                .unwrap_or_else(|error| panic!("{source}: {error}"));
            assert!(
                formula.width > 0. && formula.height > 0.,
                "{source} has no size"
            );
            assert!(formula.svg.starts_with(b"<svg"), "{source} is not an image");
        }
    }

    #[test]
    fn reports_broken_formulas() {
        assert!(lay_out(r"\frac{1}{", false).is_err());
        // The same answer from the cache.
        assert!(lay_out(r"\frac{1}{", false).is_err());
    }
}
