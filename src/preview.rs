//! The rendered side of a document.
//!
//! The component kit parses and draws the Markdown as a virtualized list with
//! one row per top-level block, so only the blocks on screen are laid out.
//! `BlockMap` records which source lines each of those rows came from; that is
//! what lets the editor and the preview scroll together.

mod callout;
pub mod figure;
pub mod frontmatter;
pub mod math;
pub mod mermaid;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

use gpui_kit::base::text::{MarkdownExtensions, TextViewStyle};
use gpui_kit::*;

use crate::theme::{Palette, size};

/// Markdown syntax beyond the core that the preview understands: front matter,
/// callouts, math and diagrams. `parse_options` must describe the same syntax,
/// or rows and lines drift apart.
///
/// Built once and shared: the kit parses a document again whenever it is handed
/// a registry it has not seen before.
pub fn extensions() -> MarkdownExtensions {
    static EXTENSIONS: LazyLock<MarkdownExtensions> = LazyLock::new(|| {
        callout::register(
            MarkdownExtensions::default()
                .frontmatter()
                .plugin(frontmatter::FrontMatter)
                .plugin(math::InlineMath)
                .plugin(math::BlockMath)
                .plugin(mermaid::Mermaid)
                .plugin(figure::Figure),
        )
    });
    EXTENSIONS.clone()
}

/// How the kit's parser is configured, given `extensions`.
pub fn parse_options() -> markdown::ParseOptions {
    let mut options = markdown::ParseOptions::gfm();
    options.constructs.frontmatter = true;
    options.constructs.math_text = true;
    options.constructs.math_flow = true;
    options
}

/// The document on show: what its pictures are named relative to.
#[derive(Default, PartialEq)]
pub struct Shown {
    pub folder: Option<PathBuf>,
}

impl Global for Shown {}

/// Something on the page that can be looked at closely: opened large over
/// the page, zoomed, copied, saved.
#[derive(Clone)]
pub enum Subject {
    /// A picture from the document. `file` is where it is on disk, if it is.
    Picture {
        source: ImageSource,
        name: SharedString,
        file: Option<PathBuf>,
    },
    /// A diagram: its source, the picture drawn of it, and how big that
    /// picture is at its natural size.
    Diagram {
        code: SharedString,
        picture: Arc<Image>,
        size: Size<Pixels>,
    },
}

impl Subject {
    /// The picture as something the clipboard can hold. `None` for a picture
    /// that lives on the web: only its address is at hand.
    pub fn image(&self) -> Option<Image> {
        match self {
            Subject::Diagram { picture, .. } => Some(picture.as_ref().clone()),
            Subject::Picture {
                source: ImageSource::Image(image),
                ..
            } => Some(image.as_ref().clone()),
            Subject::Picture {
                file: Some(file), ..
            } => {
                let kind = file.extension()?.to_str()?.to_ascii_lowercase();
                let format = match kind.as_str() {
                    "png" => ImageFormat::Png,
                    "jpg" | "jpeg" => ImageFormat::Jpeg,
                    "gif" => ImageFormat::Gif,
                    "webp" => ImageFormat::Webp,
                    "svg" => ImageFormat::Svg,
                    "bmp" => ImageFormat::Bmp,
                    "tif" | "tiff" => ImageFormat::Tiff,
                    _ => return None,
                };
                Some(Image::from_bytes(format, std::fs::read(file).ok()?))
            }
            Subject::Picture { .. } => None,
        }
    }

    /// Put the picture on the clipboard. Says whether it could.
    pub fn copy(&self, cx: &mut App) -> bool {
        match self.image() {
            Some(image) => {
                cx.write_to_clipboard(ClipboardItem::new_image(&image));
                true
            }
            None => false,
        }
    }
}

/// What was last asked to be looked at closely, until the window takes it.
#[derive(Default)]
pub struct Looking(pub Option<Subject>);

impl Global for Looking {}

/// Ask for `subject` to be opened large over the page.
pub fn look_at(subject: Subject, window: &mut Window, cx: &mut App) {
    cx.set_global(Looking(Some(subject)));
    window.dispatch_action(Box::new(crate::workspace::LookCloser), cx);
}

/// What the card over a link says of the note, or the heading, it leads to:
/// the file, what it is called, and its first words.
pub fn glimpse_card(glimpse: &crate::links::Glimpse, p: &Palette) -> Div {
    let file = (!glimpse.file.is_empty() && glimpse.file != glimpse.title).then(|| {
        div()
            .text_size(px(10.5))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(p.text_tertiary)
            .child(glimpse.file.clone())
    });
    let about = (!glimpse.about.is_empty()).then(|| {
        div()
            .text_size(px(12.5))
            .line_height(px(19.))
            .text_color(p.text_secondary)
            .child(glimpse.about.clone())
    });
    div()
        .flex()
        .flex_col()
        .w(px(300.))
        .py(px(6.))
        .px(px(4.))
        .gap(px(4.))
        .children(file)
        .child(
            div()
                .text_size(px(14.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(p.text_primary)
                .child(glimpse.title.clone()),
        )
        .children(about)
}

/// A small round button that sits on a picture or a diagram while the
/// pointer is over it.
pub fn tool(
    id: impl Into<ElementId>,
    icon: gpui_kit::assets::IconName,
    p: &Palette,
) -> Stateful<Div> {
    let (strong, hover) = (p.text_primary, p.surface_hover);
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(32.))
        .rounded(size::RADIUS_SM)
        .text_color(p.text_secondary)
        .hover(move |style| style.bg(hover).text_color(strong))
        .active(|style| style.opacity(0.7))
        .cursor_pointer()
        .child(gpui_kit::component::Icon::new(icon).size(px(16.)))
}

/// Where the picture behind an image link comes from.
///
/// Paths are read from disk, relative to the folder of the document. Pictures
/// embedded as `data:` links are decoded once and remembered. Anything else is
/// left to the kit to fetch.
pub fn image_source(uri: &SharedUri, folder: Option<&Path>) -> ImageSource {
    static EMBEDDED: LazyLock<Mutex<HashMap<SharedUri, Option<Arc<Image>>>>> =
        LazyLock::new(Default::default);

    let link: &str = uri.as_ref();
    if link.starts_with("data:") {
        let mut embedded = EMBEDDED.lock().unwrap();
        if embedded.len() >= 64 && !embedded.contains_key(uri) {
            embedded.clear();
        }
        let image = embedded
            .entry(uri.clone())
            .or_insert_with(|| decode_data_url(link).map(Arc::new));
        return match image {
            Some(image) => ImageSource::Image(image.clone()),
            None => ImageSource::Resource(Resource::Uri(uri.clone())),
        };
    }
    let is_remote = link
        .split_once("://")
        .is_some_and(|(scheme, _)| scheme != "file");
    if is_remote {
        return ImageSource::Resource(Resource::Uri(uri.clone()));
    }

    let path = Path::new(link.strip_prefix("file://").unwrap_or(link));
    let path = match (path.is_absolute(), folder) {
        (false, Some(folder)) => folder.join(path),
        _ => path.to_path_buf(),
    };
    ImageSource::Resource(Resource::Path(path.into()))
}

fn decode_data_url(link: &str) -> Option<Image> {
    let url = data_url::DataUrl::process(link).ok()?;
    let format = match url.mime_type().subtype.as_str() {
        "png" => ImageFormat::Png,
        "jpeg" | "jpg" => ImageFormat::Jpeg,
        "gif" => ImageFormat::Gif,
        "webp" => ImageFormat::Webp,
        "svg+xml" => ImageFormat::Svg,
        "bmp" => ImageFormat::Bmp,
        "tiff" => ImageFormat::Tiff,
        _ => return None,
    };
    let (bytes, _) = url.decode_to_vec().ok()?;
    Some(Image::from_bytes(format, bytes))
}

/// The size of the words of a page whose text column is `sheet` wide.
///
/// A line is easy to read at 60 to 85 characters. The column is the shape of
/// a sheet of paper and grows with the window, so past a certain width the
/// words grow with it, the way a page held closer looks bigger, instead of
/// the lines growing ever longer. `scale` is the reader's own choice on top.
pub fn body_size(sheet: Option<Pixels>, scale: f32) -> Pixels {
    // A character of the body font is about half as wide as the font is tall.
    const CHARACTERS: f32 = 84.;
    let fitted = sheet.map_or(size::FONT_BODY, |sheet| {
        (sheet / (CHARACTERS * 0.49)).clamp(size::FONT_BODY, size::FONT_BODY_LARGEST)
    });
    (fitted * scale).round()
}

/// Type sizes and spacing of rendered Markdown, for words `body` tall.
///
/// Everything is a multiple of the body size, so the page keeps its
/// proportions at any size: a heading has more room above it than below, the
/// more the higher its level, and paragraphs sit a little under a line apart.
pub fn style(p: &Palette, body: Pixels, dark: bool) -> TextViewStyle {
    let radius = |mut style: StyleRefinement| {
        let corner: AbsoluteLength = size::RADIUS_MD.into();
        style.corner_radii.top_left = Some(corner);
        style.corner_radii.top_right = Some(corner);
        style.corner_radii.bottom_left = Some(corner);
        style.corner_radii.bottom_right = Some(corner);
        style
    };
    let inline_code = HighlightStyle {
        background_color: Some(p.surface_hover),
        ..Default::default()
    };
    // The kit measures gaps in rems, which are 16 px whatever the text size.
    let lines = |of_body: f32| rems(f32::from(body) * of_body / 16.);
    TextViewStyle::default()
        .with_foreground(p.text_primary)
        .with_muted_foreground(p.text_secondary)
        .with_link(p.accent_text)
        .with_selection(p.accent.opacity(0.28))
        .with_code_background(p.surface_sunken)
        .with_border(p.border_subtle)
        .with_dark(dark)
        .with_inline_code(inline_code)
        .with_paragraph_gap(lines(1.))
        // The kit already gives a heading a gap that depends on what comes
        // before it; this adds to that gap, more for the higher levels, so a
        // new section is seen coming. The room is an unseen border: the
        // kit's own gap is padding, which a second padding would replace, and
        // a margin is left out when the page works out how tall a block is.
        .with_heading(move |level| {
            let (size, above) = match level {
                1 => (2., 0.),
                2 => (1.4, 0.7),
                3 => (1.16, 0.6),
                _ => (1., 0.5),
            };
            StyleRefinement::default()
                .text_size(body * size)
                .border_t(body * above)
                .border_color(transparent_black())
        })
        // Code is set a little smaller than the words around it, and grows
        // with them.
        .with_code_block(radius(
            StyleRefinement::default()
                .text_size(body * 0.87)
                .px(body * 1.1)
                .pb(body * 0.9),
        ))
        // Table rows sit on the page itself; only the header row is tinted.
        // A table too wide for the page is not squeezed until its words
        // break in two: its columns give way down to a width they can be
        // read at, and past that the table scrolls sideways.
        .with_table(radius({
            let mut table = StyleRefinement::default().bg(p.surface_preview);
            table.overflow.x = Some(Overflow::Scroll);
            table
        }))
        .with_table_head(
            StyleRefinement::default()
                .bg(p.surface_sunken)
                .text_color(p.text_secondary),
        )
        .with_table_row_hover(p.surface_hover.opacity(0.5))
}

/// The source lines each top-level block of a document starts on, in order.
///
/// Block `ix` covers the lines from its start up to the start of the next
/// block, so the blank lines after a block belong to it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BlockMap {
    starts: Vec<u32>,
    line_count: u32,
    /// How much text comes before each block, in bytes, and how much there
    /// is in all: what "half way through" is measured in.
    before: Vec<u32>,
    size: u32,
}

impl BlockMap {
    pub fn parse(text: &str) -> Self {
        let Ok(markdown::mdast::Node::Root(root)) = markdown::to_mdast(text, &parse_options())
        else {
            return Self::default();
        };
        let mut starts = Vec::with_capacity(root.children.len());
        let mut previous = 0;
        for node in &root.children {
            // Positions are one-based. A node without one sits where the last one did.
            let start = node.position().map_or(previous, |position| {
                position.start.line.saturating_sub(1) as u32
            });
            previous = start.max(previous);
            starts.push(previous);
        }
        // Where each line begins, to say how much text is before a block.
        let mut lines = vec![0];
        lines.extend(text.match_indices('\n').map(|(at, _)| at as u32 + 1));
        let before = starts
            .iter()
            .map(|line| {
                lines
                    .get(*line as usize)
                    .copied()
                    .unwrap_or(text.len() as u32)
            })
            .collect();
        Self {
            starts,
            line_count: text.lines().count() as u32,
            before,
            size: text.len() as u32,
        }
    }

    /// How much of the text comes before the place `through` (0 to 1) block
    /// `ix`, from 0 to 1. It goes by the amount of text, not by the number
    /// of blocks: a long paragraph is more of a document than a short one.
    pub fn share_before(&self, ix: usize, through: f32) -> Option<f32> {
        let start = *self.before.get(ix)?;
        let end = self.before.get(ix + 1).copied().unwrap_or(self.size);
        let at = start as f32 + end.saturating_sub(start) as f32 * through.clamp(0., 1.);
        (self.size > 0).then(|| (at / self.size as f32).clamp(0., 1.))
    }

    pub fn len(&self) -> usize {
        self.starts.len()
    }

    /// First line of block `ix`, and the line just past its end.
    fn lines(&self, ix: usize) -> (f32, f32) {
        let start = self.starts[ix];
        let end = self
            .starts
            .get(ix + 1)
            .copied()
            .unwrap_or(self.line_count)
            .max(start + 1);
        (start as f32, end as f32)
    }

    /// The block that holds source line `line`, and how far through the block it is (0 to 1).
    /// `line` may be fractional: 4.5 is halfway down line 4.
    pub fn locate(&self, line: f32) -> Option<(usize, f32)> {
        let ix = self
            .starts
            .partition_point(|start| *start as f32 <= line)
            .checked_sub(1)
            .or((!self.starts.is_empty()).then_some(0))?;
        let (start, end) = self.lines(ix);
        Some((ix, ((line - start) / (end - start)).clamp(0., 1.)))
    }

    /// The source line `fraction` (0 to 1) of the way through block `ix`.
    pub fn line_at(&self, ix: usize, fraction: f32) -> Option<f32> {
        (ix < self.starts.len()).then(|| {
            let (start, end) = self.lines(ix);
            start + (end - start) * fraction.clamp(0., 1.)
        })
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: that would pull in the kit's own `#[test]` macro through
    // the parent's glob import and shadow the standard one.
    use super::BlockMap;

    const DOC: &str = "# Title\n\nFirst paragraph\nstill first.\n\n```rust\nfn main() {}\n\n```\n\n- one\n- two\n\n| a | b |\n| - | - |\n| 1 | 2 |\n";

    #[test]
    fn how_far_is_measured_in_text_not_in_blocks() {
        // Three blocks: a short heading, a long paragraph, a short one.
        let long = "word ".repeat(200);
        let text = format!("# Title\n\n{long}\n\nThe end.\n");
        let map = BlockMap::parse(&text);
        assert_eq!(map.len(), 3);
        assert_eq!(map.share_before(0, 0.), Some(0.));
        // The top of the second block of three is not a third of the way.
        assert!(map.share_before(1, 0.).unwrap() < 0.02);
        // Half the long paragraph is about half of the text.
        let half = map.share_before(1, 0.5).unwrap();
        assert!((0.45..0.55).contains(&half), "{half}");
        // The last block begins near the end, and ends at it.
        assert!(map.share_before(2, 0.).unwrap() > 0.97);
        assert_eq!(map.share_before(2, 1.), Some(1.));
        assert_eq!(map.share_before(3, 0.), None);
        assert_eq!(BlockMap::default().share_before(0, 0.), None);
    }

    #[test]
    fn maps_blocks_to_lines() {
        let map = BlockMap::parse(DOC);
        // heading, paragraph, code, list, table
        assert_eq!(map.starts, [0, 2, 5, 10, 13]);
        assert_eq!(map.line_count, 16);
    }

    #[test]
    fn locates_lines_in_blocks() {
        let map = BlockMap::parse(DOC);
        assert_eq!(map.locate(0.), Some((0, 0.)));
        assert_eq!(map.locate(1.), Some((0, 0.5)));
        assert_eq!(map.locate(2.), Some((1, 0.)));
        // The code block covers lines 5 to 9; line 7.5 is halfway.
        assert_eq!(map.locate(7.5), Some((2, 0.5)));
        assert_eq!(map.locate(99.), Some((4, 1.)));
        assert_eq!(BlockMap::default().locate(3.), None);
    }

    #[test]
    fn maps_blocks_back_to_lines() {
        let map = BlockMap::parse(DOC);
        assert_eq!(map.line_at(2, 0.5), Some(7.5));
        assert_eq!(map.line_at(4, 1.), Some(16.));
        assert_eq!(map.line_at(5, 0.), None);
        // Going there and back lands on the same line.
        for line in [0., 3.25, 6., 11.5, 15.] {
            let (ix, fraction) = map.locate(line).unwrap();
            assert!((map.line_at(ix, fraction).unwrap() - line).abs() < 1e-4);
        }
    }

    #[test]
    fn leading_blank_lines_belong_to_the_first_block() {
        let map = BlockMap::parse("\n\n# Late\n\ntext\n");
        assert_eq!(map.starts, [2, 4]);
        assert_eq!(map.locate(0.), Some((0, 0.)));
    }
}
