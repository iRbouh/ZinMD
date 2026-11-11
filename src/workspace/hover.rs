//! Hover that fades in and out instead of switching.
//!
//! The pointer is over one thing at a time, so the workspace only remembers
//! which. Each hoverable thing then eases its own highlight towards "under the
//! pointer" or "not", and keeps easing after the pointer has left.

use std::hash::{Hash, Hasher};

use gpui_kit::base::{Transition, transition};
use gpui_kit::*;

use super::Workspace;
use crate::motion;
use crate::theme::Accent;

/// Something the pointer can be over.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Hot {
    Button(&'static str),
    /// A file in the side panel, by its key there.
    File(u64),
    /// A folder's name, or a subfolder's, in the side panel.
    Folder(u64),
    Heading(usize),
    /// A colour in the menu of accent colours.
    Colour(Accent),
}

impl Workspace {
    /// How much `key` is under the pointer right now: 0 to 1, easing both ways.
    pub(super) fn warmth(&self, key: Hot, window: &mut Window, cx: &mut App) -> f32 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        key.hash(&mut hasher);
        let id = ElementId::from(("warmth", hasher.finish() as usize));
        let target = if self.hot == Some(key) { 1. } else { 0. };
        transition(
            id,
            target,
            Transition::new(motion::HOVER).ease(motion::ease),
            window,
            cx,
        )
    }

    /// Give `element` a fill of `color` that fades in under the pointer, and a
    /// slight dip while it is pressed.
    pub(super) fn hoverable(
        &self,
        element: Stateful<Div>,
        key: Hot,
        color: Hsla,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let warmth = self.warmth(key, window, cx);
        element
            .bg(color.opacity(warmth))
            .active(|style| style.opacity(0.75))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.hot = Some(key);
                } else if this.hot == Some(key) {
                    this.hot = None;
                }
                cx.notify();
            }))
    }
}
