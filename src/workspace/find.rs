//! Finding words on the page.
//!
//! The command key and F put a small field at the top of the page. What is
//! typed is looked for in the text as it is read (not in the Markdown behind
//! it), every place it is found is marked, and Return goes from one to the
//! next. Escape puts the field away.
//!
//! The looking is forgiving (see [`crate::matching`]): case and accents do
//! not count, several words are found in any order in a paragraph, and a slip
//! of the finger still finds the word that was meant.

use std::ops::Range;

use gpui_kit::assets::IconName;
use gpui_kit::base::text::RangeHighlight;
use gpui_kit::base::transition;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, Sizable as _, h_flex};
use gpui_kit::*;

use super::{Mode, Workspace};
use crate::matching::{self, Answer, Folded, Wanted};
use crate::motion;
use crate::theme::{ActivePalette as _, size};
use crate::ui::icon_button;

/// What the button that takes the words to every file says.
const EVERYWHERE: &str = match cfg!(target_os = "macos") {
    true => "Look in all files  ⇧⌘F",
    false => "Look in all files  Ctrl+Shift+F",
};

pub(super) struct Find {
    input: Entity<InputState>,
    /// Where the words were found, in the text of the page.
    found: Vec<Range<usize>>,
    /// Whether they are what was typed, or the nearest there is to it.
    answer: Answer,
    /// Which of those places is the one shown.
    at: usize,
    /// The words that were last looked for.
    sought: SharedString,
    _events: Subscription,
}

impl Workspace {
    /// Bring up the field, or go back to it if it is already there.
    pub(super) fn find_on_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_find(true, window, cx);
    }

    /// Bring up the field. With `typing` it takes the keyboard; without, the
    /// keyboard stays with the page. `false` when there is no page to find
    /// words on.
    fn show_find(&mut self, typing: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.mode != Mode::Read || self.active_doc().is_none() {
            return false;
        }
        if self.find.is_none() {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Find on the page"));
            let events = cx.subscribe_in(
                &input,
                window,
                |this, input, event: &InputEvent, _, cx| match event {
                    // Words put in the field for the reader were looked for
                    // as they were put there.
                    InputEvent::Change => {
                        let typed = input.read(cx).value();
                        if this.find.as_ref().is_some_and(|find| find.sought != typed) {
                            this.find_again(0, cx)
                        }
                    }
                    // Return goes on to the next place; with Shift, back to the last.
                    InputEvent::PressEnter { shift, .. } => {
                        this.find_step(if *shift { -1 } else { 1 }, cx)
                    }
                    _ => {}
                },
            );
            self.find = Some(Find {
                input,
                found: Vec::new(),
                answer: Answer::Exact,
                at: 0,
                sought: "".into(),
                _events: events,
            });
        }
        if typing && let Some(find) = &self.find {
            find.input.update(cx, |input, cx| {
                input.focus(window, cx);
                input.select_all(window, cx);
            });
        }
        self.find_open = true;
        cx.notify();
        true
    }

    /// Mark `words` on the page, and make the place nearest to line `line`
    /// of the source the one being shown: how what the search of all files
    /// found is shown. With `reveal` the page is moved to that place; without,
    /// moving it is left to the caller. The keyboard stays with the page.
    /// `false` when the words are not on the page as it is read.
    pub(super) fn find_from_search(
        &mut self,
        words: &str,
        line: u32,
        reveal: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(preview) = self.active_doc().map(|doc| doc.preview.clone()) else {
            return false;
        };
        let text = preview.read(cx).rendered_text();
        let (found, _) = matching::search(&Folded::new(text.as_str()), &Wanted::new(words));
        if found.is_empty() || !self.show_find(false, window, cx) {
            return false;
        }
        // Where the line begins on the page. A line the page does not show
        // is taken to be as far down the page as it is down the source.
        let source = text.source();
        let start: usize = source
            .split_inclusive('\n')
            .take(line as usize)
            .map(str::len)
            .sum();
        let end = start + source[start..].lines().next().map_or(0, str::len);
        let near = match text.range_for_source(start..end) {
            Some(shown) => shown.start,
            None => (start as f64 / source.len().max(1) as f64 * text.len() as f64) as usize,
        };
        if let Some(find) = &mut self.find {
            find.sought = words.to_string().into();
            let words = words.to_string();
            find.input
                .update(cx, |input, cx| input.set_value(words, window, cx));
        }
        self.mark_found(
            |found| found.partition_point(|place| place.end <= near),
            reveal,
            cx,
        );
        true
    }

    /// The words being looked for on the page, if any are.
    pub(super) fn words_being_found(&self, cx: &App) -> Option<SharedString> {
        let find = self.find.as_ref().filter(|_| self.find_open)?;
        Some(find.input.read(cx).value()).filter(|words| !words.trim().is_empty())
    }

    /// Look for `words` on the page, for scripted runs.
    #[cfg(feature = "shot")]
    pub(super) fn debug_find(&mut self, words: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.find_on_page(window, cx);
        if let Some(find) = &self.find {
            let words = words.to_string();
            find.input
                .update(cx, |input, cx| input.set_value(words, window, cx));
        }
        self.find_again(0, cx);
    }

    /// Put the field away, and take the marks off the page.
    pub(super) fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.find_open {
            return;
        }
        self.find_open = false;
        // From every page: the marks may be on one that is no longer on show.
        for doc in &self.docs {
            doc.preview
                .update(cx, |page, cx| page.clear_range_highlights(cx));
        }
        self.focus_document(window, cx);
        cx.notify();
    }

    /// Whether the field has the keyboard.
    pub(super) fn finding(&self) -> bool {
        self.find_open
    }

    /// Go to the next place the words were found, or with `-1` the one before.
    pub(super) fn find_step(&mut self, by: isize, cx: &mut Context<Self>) {
        let Some(find) = &mut self.find else { return };
        if find.found.is_empty() {
            return;
        }
        let count = find.found.len() as isize;
        let at = (find.at as isize + by).rem_euclid(count) as usize;
        self.find_again(at, cx);
    }

    /// Look for what is in the field, mark every place it is found, and show
    /// the one numbered `at`.
    fn find_again(&mut self, at: usize, cx: &mut Context<Self>) {
        self.mark_found(move |_| at, true, cx);
    }

    /// Look for what is in the field, mark every place it is found, and make
    /// the one `choose` picks from them the one being shown: with `reveal`,
    /// by moving the page to it.
    fn mark_found(
        &mut self,
        choose: impl FnOnce(&[Range<usize>]) -> usize,
        reveal: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(preview) = self.active_doc().map(|doc| doc.preview.clone()) else {
            return;
        };
        let p = *cx.palette();
        let Some(find) = &mut self.find else { return };
        let words = find.input.read(cx).value();
        let text = preview.read(cx).rendered_text();
        (find.found, find.answer) =
            matching::search(&Folded::new(text.as_str()), &Wanted::new(&words));
        find.sought = words;
        find.at = choose(&find.found).min(find.found.len().saturating_sub(1));

        // Every place is tinted; the one being shown is tinted strongly.
        let marks = find.found.iter().enumerate().map(|(ix, place)| {
            let strength = if ix == find.at { 0.6 } else { 0.22 };
            RangeHighlight::new(place.clone(), p.accent.opacity(strength))
        });
        let shown = find.found.get(find.at).filter(|_| reveal).cloned();
        preview.update(cx, |page, cx| {
            page.set_range_highlights(marks, cx).ok();
            if let Some(shown) = shown {
                page.reveal_range(shown, cx).ok();
            }
        });
        cx.notify();
    }

    /// The field, at the top of the page, while it is up or on its way out.
    pub(super) fn render_find(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let presence = transition(
            "find",
            if self.find_open { 1. } else { 0. },
            motion::glide(motion::QUICK, cx),
            window,
            cx,
        );
        if presence <= 0.001 {
            if !self.find_open {
                self.find = None;
            }
            return None;
        }
        let p = *cx.palette();
        let find = self.find.as_ref()?;
        let typed = !find.input.read(cx).value().is_empty();
        let count: SharedString = match (typed, find.found.len()) {
            (false, _) => "".into(),
            (true, 0) => "Not found".into(),
            // Said when the words as typed are not there, so that what is
            // marked is not taken for them.
            (true, all) if find.answer == Answer::Close => {
                format!("Close match · {} of {all}", find.at + 1).into()
            }
            (true, all) => format!("{} of {all}", find.at + 1).into(),
        };
        let field = Input::new(&find.input)
            .appearance(false)
            .bordered(false)
            .small()
            .px_0()
            .text_size(size::FONT_UI);

        let card = h_flex()
            .id("find")
            .w(px(340.))
            .max_w_full()
            .h(px(38.))
            .pl(px(11.))
            .pr(px(4.))
            .gap(px(6.))
            .rounded(size::RADIUS_LG)
            .border_1()
            .border_color(p.border_strong)
            .bg(p.surface_raised)
            .shadow_lg()
            .occlude()
            .text_size(size::FONT_UI)
            .child(
                Icon::new(IconName::Search)
                    .size(px(14.))
                    .text_color(p.text_tertiary),
            )
            .child(div().flex_1().min_w_0().child(field))
            .child(
                div()
                    .flex_none()
                    .text_size(size::FONT_UI_SMALL)
                    .text_color(match typed && find.found.is_empty() {
                        true => p.status_danger,
                        false => p.text_tertiary,
                    })
                    .child(count),
            )
            .child(
                icon_button("find-before", IconName::ChevronUp, &p)
                    .on_click(cx.listener(|this, _, _, cx| this.find_step(-1, cx))),
            )
            .child(
                icon_button("find-next", IconName::ChevronDown, &p)
                    .on_click(cx.listener(|this, _, _, cx| this.find_step(1, cx))),
            )
            .child(
                icon_button("find-everywhere", IconName::TextSearch, &p)
                    .tooltip(|window, cx| Tooltip::new(EVERYWHERE).build(window, cx))
                    .on_click(cx.listener(|this, _, window, cx| this.open_search(window, cx))),
            )
            .child(
                icon_button("find-close", IconName::X, &p)
                    .on_click(cx.listener(|this, _, window, cx| this.close_find(window, cx))),
            );

        // Under the bar at the top, at the right, clear of the rail.
        Some(
            div()
                .absolute()
                .top(size::TITLEBAR + px(2.) - px(8.) * (1. - presence))
                .right(px(56.))
                .left(px(16.))
                .flex()
                .justify_end()
                .opacity(presence)
                .child(card),
        )
    }
}
