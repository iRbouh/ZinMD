//! The rendered side of a document.
//!
//! The component kit parses and draws the Markdown as a virtualized list with
//! one row per top-level block, so only the blocks on screen are laid out.
//! `BlockMap` records which source lines each of those rows came from; that is
//! what lets the editor and the preview scroll together.

mod callout;
pub mod frontmatter;
pub mod math;
pub mod mermaid;

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex};

use gpui_kit::component::text::{MarkdownExtensions, TextViewStyle};
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
                .plugin(mermaid::Mermaid),
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

/// Type sizes and spacing of rendered Markdown.
pub fn style(p: &Palette) -> TextViewStyle {
    // Table rows sit on the page itself; only the header row is tinted.
    let table = StyleRefinement::default().bg(p.surface_preview);
    TextViewStyle::default()
        .paragraph_gap(rems(1.))
        .table(table)
        .heading_font_size(|level, _| match level {
            1 => size::FONT_H1,
            2 => size::FONT_H2,
            3 => size::FONT_H3,
            _ => size::FONT_BODY,
        })
}

/// The source lines each top-level block of a document starts on, in order.
///
/// Block `ix` covers the lines from its start up to the start of the next
/// block, so the blank lines after a block belong to it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BlockMap {
    starts: Vec<u32>,
    line_count: u32,
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
        Self {
            starts,
            line_count: text.lines().count() as u32,
        }
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
