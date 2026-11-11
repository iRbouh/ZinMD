//! Mermaid diagrams: fenced code blocks tagged `mermaid`.
//!
//! Drawing a diagram takes tens of milliseconds, so it never happens while the
//! window is being drawn. The first time a diagram is shown it is sent to a
//! background thread; until the picture is ready the block shows the previous
//! picture drawn at that spot, so a diagram being typed updates without a flash.
//! Pictures are cached by source, appearance and accent colour.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use gpui_kit::assets::IconName;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::h_flex;
use gpui_kit::component::text::{
    MarkdownNode, MarkdownParseContext, MarkdownPlugin, markdown_ast as mdast,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{Subject, look_at, tool};
use crate::theme::{self, Accent, ActivePalette as _, Palette, size};

/// Pixels drawn per point, so diagrams stay sharp on Retina displays.
const SCALE: f32 = 2.;
/// How tall the words in a diagram are when it is shown at its natural size.
const DRAWN_TEXT: f32 = 16.;
/// The tallest a diagram is shown on the page, in lines of its own words.
const TALLEST: f32 = 34.;

/// A drawn diagram.
#[derive(Clone)]
struct Picture {
    image: Arc<Image>,
    /// Size in points.
    size: Size<Pixels>,
}

enum Entry {
    Drawing,
    /// The picture, and when it arrived if it is the first one drawn at its
    /// spot: a first picture fades in, an edited one simply replaces the last.
    Ready(Picture, Option<Instant>),
    Failed(SharedString),
}

/// Diagram source hash, whether it was drawn for the dark appearance, and in
/// which accent colour.
type Key = (u64, bool, Accent);

#[derive(Default)]
struct Diagrams {
    entries: HashMap<Key, Entry>,
    /// The last picture drawn for the block starting at a source offset.
    latest: HashMap<usize, Picture>,
}

impl Global for Diagrams {}

/// What `parse` keeps about a diagram block.
struct Source {
    code: SharedString,
    hash: u64,
    /// Where the block starts in the document: its identity while it is edited.
    start: usize,
}

pub struct Mermaid;

impl MarkdownPlugin for Mermaid {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        "mermaid"
    }

    fn parse(&self, node: &mdast::Node, cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let mdast::Node::Code(code) = node else {
            return None;
        };
        if !code
            .lang
            .as_deref()
            .is_some_and(|lang| lang.eq_ignore_ascii_case("mermaid"))
        {
            return None;
        }
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        code.value.hash(&mut hasher);
        let start = cx.offset()
            + code
                .position
                .as_ref()
                .map_or(0, |position| position.start.offset);
        let source = Source {
            code: code.value.clone().into(),
            hash: hasher.finish(),
            start,
        };
        Some(
            MarkdownNode::new("mermaid", source)
                .text(code.value.clone())
                .markdown(cx.node_source(node).unwrap_or_default().to_string()),
        )
    }

    fn render(&self, node: &MarkdownNode, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = *cx.palette();
        let frame = div()
            .relative()
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(8.))
            .p(px(16.))
            .rounded(size::RADIUS_MD)
            .border_1()
            .border_color(p.border_subtle);
        let Some(source) = node.data::<Source>() else {
            return frame.into_any_element();
        };

        let key = (source.hash, cx.theme().is_dark(), theme::accent(cx));
        if !cx.default_global::<Diagrams>().entries.contains_key(&key) {
            // The colours the app is settling on, not ones caught partway through a fade.
            draw_in_background(source, key, &theme::settled(cx), cx);
        }
        // A diagram is drawn with words 16 px tall. Shown at the size of the
        // words around it, its labels read as part of the page.
        let body = window.text_style().font_size.to_pixels(window.rem_size());
        let scale = f32::from(body) / DRAWN_TEXT;
        let diagrams = cx.global::<Diagrams>();
        let previous = diagrams.latest.get(&source.start).cloned();
        match diagrams.entries.get(&key) {
            Some(Entry::Ready(picture, arrived)) => {
                let shown = arrived.map_or(1., |arrived| {
                    crate::motion::progress(arrived, crate::motion::PANEL, cx)
                });
                if shown < 1. {
                    window.request_animation_frame();
                }
                // A click opens the diagram large over the page; while the
                // pointer is over it, small buttons offer that and a copy.
                let subject = Subject::Diagram {
                    code: source.code.clone(),
                    picture: picture.image.clone(),
                    size: picture.size,
                };
                let (open, expand, copy, code) = (
                    subject.clone(),
                    subject.clone(),
                    subject,
                    source.code.clone(),
                );
                let tools = h_flex()
                    .absolute()
                    .top(px(8.))
                    .right(px(8.))
                    .gap(px(2.))
                    .p(px(2.))
                    .rounded(size::RADIUS_MD)
                    .border_1()
                    .border_color(p.border_subtle)
                    .bg(p.surface_raised)
                    .shadow_sm()
                    .opacity(0.)
                    .group_hover("diagram", |style| style.opacity(1.))
                    .child(
                        tool(("diagram-open", source.start), IconName::Expand, &p).on_click(
                            move |_, window, cx| {
                                cx.stop_propagation();
                                look_at(expand.clone(), window, cx);
                            },
                        ),
                    )
                    .child(
                        tool(("diagram-copy", source.start), IconName::Copy, &p).on_click(
                            move |_, _, cx| {
                                cx.stop_propagation();
                                copy.copy(cx);
                            },
                        ),
                    )
                    .child(
                        tool(("diagram-code", source.start), IconName::Code, &p).on_click(
                            move |_, _, cx| {
                                cx.stop_propagation();
                                cx.write_to_clipboard(ClipboardItem::new_string(code.to_string()));
                            },
                        ),
                    );
                return frame
                    .id(("diagram", source.start))
                    .group("diagram")
                    .cursor_pointer()
                    .child(picture.element(scale, previous).opacity(shown))
                    .child(tools)
                    // Spreading two fingers on it opens it too, and the
                    // same spread then goes on to zoom it.
                    .on_pinch({
                        let open = open.clone();
                        move |event, window, cx| {
                            if event.delta > 0. {
                                look_at(open.clone(), window, cx);
                            }
                        }
                    })
                    .on_click(move |_, window, cx| look_at(open.clone(), window, cx))
                    .into_any_element();
            }
            Some(Entry::Failed(error)) => frame
                // Keep the last good picture in view while the source is mid-edit.
                .children(previous.map(|picture| picture.element(scale, None).opacity(0.45)))
                .child(
                    div()
                        .w_full()
                        .text_size(px(12.))
                        .text_color(p.status_danger)
                        .child(error.clone()),
                ),
            _ => match previous {
                Some(picture) => frame.child(picture.element(scale, None)),
                None => frame.child(
                    div()
                        .py(px(28.))
                        .text_size(px(12.))
                        .text_color(p.text_tertiary)
                        .child("Drawing diagram…"),
                ),
            },
        }
        .into_any_element()
    }
}

impl Picture {
    /// The picture at `scale` times its natural size, shrinking to fit a
    /// narrower column. `while_decoding` is shown for the frame or two the
    /// new picture needs to load.
    fn element(&self, scale: f32, while_decoding: Option<Picture>) -> Div {
        // A very tall diagram is shown smaller, so the page still flows past
        // it; it can be opened large to be read closely.
        let tallest = px(TALLEST * DRAWN_TEXT * scale);
        let scale = scale.min(tallest / self.size.height);
        // Two boxes. The outer is as wide as the picture, or as the column
        // when that is narrower. The inner takes its shape from the picture
        // at that width, so the layout is settled before the picture loads.
        // (One box doing both took its height from the column's width, and
        // left any diagram narrower than the column with a blank under it.)
        div().w_full().max_w(self.size.width * scale).child(
            div()
                .w_full()
                .aspect_ratio(self.size.width / self.size.height)
                .child(img(self.image.clone()).size_full().when_some(
                    while_decoding,
                    |image, previous| {
                        image.with_loading(move || {
                            img(previous.image.clone()).size_full().into_any_element()
                        })
                    },
                )),
        )
    }
}

fn draw_in_background(source: &Source, key: Key, p: &Palette, cx: &mut App) {
    let diagrams = cx.global_mut::<Diagrams>();
    // Old pictures are cheap to draw again; do not let them pile up.
    if diagrams.entries.len() >= 256 {
        diagrams
            .entries
            .retain(|_, entry| matches!(entry, Entry::Drawing));
    }
    diagrams.entries.insert(key, Entry::Drawing);

    let start = source.start;
    let code = themed(&source.code, p);
    cx.spawn(async move |cx| {
        let drawn = cx
            .background_executor()
            .spawn(async move { draw(&code, SCALE) })
            .await;
        cx.update(|cx| {
            let diagrams = cx.global_mut::<Diagrams>();
            let entry = match drawn {
                Ok(picture) => {
                    let first = diagrams.latest.insert(start, picture.clone()).is_none();
                    Entry::Ready(picture, first.then(Instant::now))
                }
                Err(error) => Entry::Failed(error.into()),
            };
            diagrams.entries.insert(key, entry);
            cx.refresh_windows();
        });
    })
    .detach();
}

/// Put our colours in front of a diagram, unless it brings its own configuration.
fn themed(code: &str, p: &Palette) -> String {
    let head = code.trim_start();
    if head.starts_with("%%{") || head.starts_with("---") {
        return code.to_string();
    }
    // Mermaid wants plain opaque colours.
    let hex = |color: Hsla| {
        let Rgba { r, g, b, .. } = p.surface_preview.blend(color).into();
        let byte = |v: f32| (v.clamp(0., 1.) * 255.).round() as u8;
        format!("#{:02x}{:02x}{:02x}", byte(r), byte(g), byte(b))
    };
    let config = serde_json::json!({
        "theme": "base",
        "themeVariables": {
            "background": hex(p.surface_preview),
            "primaryColor": hex(p.accent_soft),
            "primaryTextColor": hex(p.text_primary),
            "primaryBorderColor": hex(p.accent_hover),
            "secondaryColor": hex(p.surface_sunken),
            "secondaryTextColor": hex(p.text_primary),
            "secondaryBorderColor": hex(p.border_strong),
            "tertiaryColor": hex(p.surface_preview),
            "tertiaryTextColor": hex(p.text_primary),
            "tertiaryBorderColor": hex(p.border_strong),
            "lineColor": hex(p.text_tertiary),
            "textColor": hex(p.text_primary),
            "titleColor": hex(p.text_primary),
            "mainBkg": hex(p.accent_soft),
            "nodeBorder": hex(p.accent_hover),
            "edgeLabelBackground": hex(p.surface_preview),
            "clusterBkg": hex(p.surface_sunken),
            "clusterBorder": hex(p.border_strong),
            "noteBkgColor": hex(p.accent_muted),
            "noteTextColor": hex(p.text_primary),
            "noteBorderColor": hex(p.accent_border),
            "actorBkg": hex(p.accent_soft),
            "actorBorder": hex(p.accent_hover),
            "actorTextColor": hex(p.text_primary),
            "actorLineColor": hex(p.text_tertiary),
            "signalColor": hex(p.text_secondary),
            "signalTextColor": hex(p.text_primary),
            "labelBoxBkgColor": hex(p.accent_soft),
            "labelBoxBorderColor": hex(p.accent_hover),
            "labelTextColor": hex(p.text_primary),
            "loopTextColor": hex(p.text_primary),
            "activationBkgColor": hex(p.surface_sunken),
            "activationBorderColor": hex(p.border_strong),
            "pie1": hex(p.accent),
            "pie2": hex(p.status_info),
            "pie3": hex(p.status_success),
            "pie4": hex(p.syntax_code),
            "pie5": hex(p.syntax_number),
            "pie6": hex(p.status_danger),
            "pie7": hex(p.border_strong),
        },
    });
    format!("%%{{init: {config}}}%%\n{code}")
}

/// A diagram as an SVG picture for export, in the colours of `p`.
pub fn svg(code: &str, p: &Palette) -> Result<String, String> {
    use merman::{OperationControl, RenderOutput, RenderRequest, Renderer, SvgRequest};

    let code = themed(code, p);
    let request = SvgRequest {
        environment: environment()?.clone(),
        ..Default::default()
    };
    let render =
        move || Renderer::new().render(RenderRequest::svg(&code, OperationControl::new(), request));
    let output = std::panic::catch_unwind(std::panic::AssertUnwindSafe(render))
        .map_err(|_| "this diagram could not be drawn".to_string())?
        .map_err(|error| error.to_string())?;
    match output {
        RenderOutput::Svg(Some(svg)) => Ok(svg.svg().to_string()),
        _ => Err("no diagram found in this block".to_string()),
    }
}

/// Measuring and drawing text must use the same font or labels overflow their
/// boxes, so the font is loaded once and handed to both.
fn environment() -> Result<&'static merman::SvgEnvironment, String> {
    static ENVIRONMENT: OnceLock<Result<merman::SvgEnvironment, String>> = OnceLock::new();
    ENVIRONMENT
        .get_or_init(|| {
            use merman::svg::export::{NativeFontContext, NativeFontLimits};
            // The system's plain font, or the editor's own where there is none.
            let font = crate::system::sans_font().unwrap_or_else(|| {
                crate::settings::EditorFont::JetBrainsMono
                    .regular()
                    .to_vec()
            });
            let fonts = NativeFontContext::from_font_bytes(
                [font],
                NativeFontLimits::default(),
                &merman::OperationControl::new(),
            )
            .map_err(|error| error.to_string())?;
            merman::SvgEnvironment::deterministic()
                .with_native_font_context(fonts)
                .map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// A diagram drawn with characters, for pasting where pictures cannot go.
pub fn as_text(code: &str) -> Result<String, String> {
    use merman::{AsciiRequest, OperationControl, RenderOutput, RenderRequest, Renderer};

    let code = code.to_string();
    let render = move || {
        Renderer::new().render(RenderRequest::ascii(
            &code,
            OperationControl::new(),
            AsciiRequest::default(),
        ))
    };
    let output = std::panic::catch_unwind(std::panic::AssertUnwindSafe(render))
        .map_err(|_| "this diagram could not be drawn".to_string())?
        .map_err(|error| error.to_string())?;
    match output {
        RenderOutput::Ascii(Some(drawn)) => Ok(drawn.text),
        _ => Err("this kind of diagram cannot be drawn with text".to_string()),
    }
}

/// A diagram drawn again with `scale` pixels to the point, in the colours of
/// `p`: sharper than the one on the page, for looking at closely or saving.
/// Slow: call from a background thread.
pub fn sharper(code: &str, p: &Palette, scale: f32) -> Result<(Arc<Image>, Size<Pixels>), String> {
    draw(&themed(code, p), scale).map(|picture| (picture.image, picture.size))
}

fn draw(code: &str, scale: f32) -> Result<Picture, String> {
    use merman::svg::export::RasterOptions;
    use merman::{OperationControl, PngRequest, RenderOutput, RenderRequest, Renderer, SvgRequest};

    let request = PngRequest {
        svg: SvgRequest {
            environment: environment()?.clone(),
            ..Default::default()
        },
        options: RasterOptions::default().with_scale(scale),
    };
    // The diagram engine is young: a diagram it trips over must not take the app down.
    let code = code.to_string();
    let render =
        move || Renderer::new().render(RenderRequest::png(&code, OperationControl::new(), request));
    let output = std::panic::catch_unwind(std::panic::AssertUnwindSafe(render))
        .map_err(|_| "this diagram could not be drawn".to_string())?
        .map_err(|error| error.to_string())?;
    let RenderOutput::Png(Some(png)) = output else {
        return Err("no diagram found in this block".to_string());
    };
    // A PNG states its width and height as big-endian numbers at bytes 16 to 24.
    let dimension = |at: usize| {
        png.bytes
            .get(at..at + 4)
            .map(|bytes| u32::from_be_bytes(bytes.try_into().unwrap()) as f32 / scale)
    };
    let (Some(width), Some(height)) = (dimension(16), dimension(20)) else {
        return Err("the diagram came out empty".to_string());
    };
    if width <= 0. || height <= 0. {
        return Err("the diagram came out empty".to_string());
    }
    Ok(Picture {
        image: Arc::new(Image::from_bytes(ImageFormat::Png, png.bytes)),
        size: size(px(width), px(height)),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_diagram_can_be_drawn_with_text() {
        let text = super::as_text("flowchart LR\n  Source --> Parser --> GPU").unwrap();
        for word in ["Source", "Parser", "GPU"] {
            assert!(text.contains(word), "{word} is missing from:\n{text}");
        }
        // More than one line: boxes, not a list of names.
        assert!(text.lines().count() >= 3, "{text}");
    }

    use super::{draw, themed};
    use crate::theme::{Accent, Palette};

    /// Draws real diagrams. Set `ZINMD_DIAGRAMS=<folder>` to keep the pictures.
    #[test]
    fn draws_diagrams() {
        let diagrams = [
            (
                "flowchart",
                "flowchart LR\n  Source --> Parser --> Blocks --> Layout --> GPU\n",
            ),
            (
                "sequence",
                "sequenceDiagram\n  participant U as User\n  participant E as Editor\n  U->>E: type\n  E-->>U: rendered\n  Note over U,E: under 8 ms\n",
            ),
            (
                "decision",
                "flowchart TD\n  A[Start] --> B{Changed?}\n  B -- yes --> C[Layout]\n  B -- no --> D[(Cache)]\n  C --> E([Done])\n  D --> E\n",
            ),
        ];
        for (name, code) in diagrams {
            let accent = Accent::Mustard;
            for (mode, palette) in [
                ("light", Palette::light(accent)),
                ("dark", Palette::dark(accent)),
            ] {
                let picture = draw(&themed(code, &palette), 2.)
                    .unwrap_or_else(|error| panic!("{name}: {error}"));
                let (width, height) = (
                    f32::from(picture.size.width),
                    f32::from(picture.size.height),
                );
                println!("{name} {mode}: {width} x {height}");
                assert!(width > 50. && height > 20., "{name} is too small");
                if let Ok(folder) = std::env::var("ZINMD_DIAGRAMS") {
                    std::fs::write(format!("{folder}/{name}-{mode}.png"), picture.image.bytes())
                        .unwrap();
                }
            }
        }
        assert!(draw("not a diagram at all", 2.).is_err());
    }
}
