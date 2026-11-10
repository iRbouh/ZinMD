//! The rendered side of a document.
//!
//! The component kit parses and draws the Markdown as a virtualized list with
//! one row per top-level block, so only the blocks on screen are laid out.
//! `BlockMap` records which source lines each of those rows came from; that is
//! what lets the editor and the preview scroll together.

use gpui_kit::component::text::{MarkdownExtensions, TextViewStyle};
use gpui_kit::*;

use crate::theme::size;

/// Markdown syntax beyond the core that the preview understands.
/// `parse_options` must describe the same syntax, or rows and lines drift apart.
pub fn extensions() -> MarkdownExtensions {
    MarkdownExtensions::default()
}

/// How the kit's parser is configured, given `extensions`.
fn parse_options() -> markdown::ParseOptions {
    let mut options = markdown::ParseOptions::gfm();
    options.constructs.math_text = true;
    options.constructs.math_flow = true;
    options
}

/// Type sizes and spacing of rendered Markdown.
pub fn style() -> TextViewStyle {
    TextViewStyle::default()
        .paragraph_gap(rems(1.))
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
