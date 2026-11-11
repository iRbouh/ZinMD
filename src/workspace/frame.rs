//! The window's two shapes: a sheet of paper to read from, and a wide desk to
//! edit at. Switching between them changes the window itself, which glides
//! from one shape to the other on the same curve as everything else.
//!
//! The framework can set a window's size but not move it, nor hold it to a
//! proportion, so on macOS the window is asked directly.

use std::time::{Duration, Instant};

use gpui_kit::*;

use super::{Mode, Workspace};
use crate::motion;

/// A4: the long side of the sheet is √2 times the short one.
pub const SHEET: f32 = std::f32::consts::SQRT_2;
/// The margin at each side of a page, as a share of the sheet's width, and
/// the least it is: room for the rail of marks down the page's edge. The
/// sheet is all text: the margins are added around it, not taken out of it.
const MARGIN: f32 = 0.07;
const MARGIN_LEAST: f32 = 44.;

/// How wide a page's window is for a sheet this wide.
fn with_margins(sheet: f64) -> f64 {
    (sheet + 2. * (sheet * MARGIN as f64).max(MARGIN_LEAST as f64)).round()
}
/// How big the editor's window is before the user has given it a size.
const DESK: (f32, f32) = (1280., 820.);
/// The tallest a page opens, however tall the display: beyond this the text
/// column would be wider than is comfortable to read.
const PAGE_TALLEST: f32 = 1120.;
const PAGE_SHORTEST: f32 = 480.;
/// How long the window takes to change shape.
const RESHAPE: Duration = Duration::from_millis(340);

/// What the window opens as. A file is opened to be read.
pub(super) fn starting_mode() -> Mode {
    #[cfg(feature = "shot")]
    if std::env::var("ZINMD_MODE").as_deref() == Ok("edit") {
        return Mode::Edit;
    }
    Mode::Read
}

/// How wide a page's window is when it is this tall: a sheet of that height,
/// and a margin at each side of it.
#[cfg_attr(feature = "shot", allow(dead_code))]
pub fn page_width(height: Pixels) -> Pixels {
    px(with_margins((f32::from(height) / SHEET) as f64) as f32)
}

/// How wide the sheet is in a page's window of this size: as wide as a sheet
/// of the window's height, or as the window has room for.
pub(super) fn sheet_width(window: Size<Pixels>) -> Pixels {
    (window.height / SHEET)
        .min(window.width - px(2. * MARGIN_LEAST))
        .max(px(240.))
}

/// The size of a page on a display with this much room.
#[cfg_attr(feature = "shot", allow(dead_code))]
pub fn page_size(room: Size<Pixels>) -> Size<Pixels> {
    let height = (room.height - px(40.))
        .min(px(PAGE_TALLEST))
        .max(px(PAGE_SHORTEST));
    size(page_width(height), height)
}

/// A rectangle on screen, in the system's terms: `y` counts up from the
/// bottom of the main display.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Frame {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl Frame {
    /// The frame `progress` of the way from this one to `other`.
    fn towards(self, other: Frame, progress: f64) -> Frame {
        let mix = |from: f64, to: f64| from + (to - from) * progress;
        Frame {
            x: mix(self.x, other.x).round(),
            y: mix(self.y, other.y).round(),
            width: mix(self.width, other.width).round(),
            height: mix(self.height, other.height).round(),
        }
    }
}

/// Where the window goes when it becomes `mode`: the size that mode had last
/// time, or its usual one, hanging from the same top edge about the same
/// centre line, and brought back onto the display if that put it over an edge.
fn destination(
    mode: Mode,
    from: Frame,
    display: Frame,
    page: Option<[f32; 4]>,
    desk: Option<[f32; 4]>,
) -> Frame {
    let (width, height) = match mode {
        Mode::Read => {
            let height = page
                .map_or(display.height - 40., |page| page[3] as f64)
                .min(display.height)
                .min(PAGE_TALLEST as f64)
                .max(PAGE_SHORTEST as f64);
            (with_margins(height / SHEET as f64), height)
        }
        Mode::Edit => {
            let (width, height) = desk.map_or(DESK, |desk| (desk[2], desk[3]));
            (
                (width as f64).min(display.width),
                (height as f64).min(display.height),
            )
        }
    };
    let top = from.y + from.height;
    let centre = from.x + from.width / 2.;
    let x = (centre - width / 2.).clamp(
        display.x,
        (display.x + display.width - width).max(display.x),
    );
    let y = (top - height).clamp(
        display.y,
        (display.y + display.height - height).max(display.y),
    );
    Frame {
        x: x.round(),
        y: y.round(),
        width: width.round(),
        height: height.round(),
    }
}

/// Room left around a row of papers, and between two of them.
const ROW_GAP: f64 = 16.;

/// The narrowest a paper is made to stand in a row: a window is no narrower.
const ROW_NARROWEST: f64 = 380.;

/// Where `count` papers go to stand side by side on `display`: sheets of
/// one size, as tall as the display has room for, in a row in the middle,
/// from left to right. When that many would be too narrow in one row, they
/// stand in two rows, or three.
pub(super) fn side_by_side(display: Frame, count: usize) -> Vec<Frame> {
    if count == 0 {
        return Vec::new();
    }
    let sheet_in =
        |width: f64| (width / (1. + 2. * MARGIN as f64)).min(width - 2. * MARGIN_LEAST as f64);
    // How many rows, how many to a row, and how wide and tall each is.
    let mut rows = 1;
    let (across, width, height) = loop {
        let across = count.div_ceil(rows);
        let room = (display.width - (across as f64 + 1.) * ROW_GAP) / across as f64;
        let tallest = ((display.height - (rows as f64 + 1.) * ROW_GAP) / rows as f64)
            .min(PAGE_TALLEST as f64);
        let width = with_margins(tallest / SHEET as f64).min(room).floor();
        if width >= ROW_NARROWEST || rows == 3 {
            // As tall as a sheet that wide, when there is the room.
            let height = (sheet_in(width) * SHEET as f64).min(tallest).round();
            break (across, width, height);
        }
        rows += 1;
    };
    let all = rows as f64 * height + (rows as f64 - 1.) * ROW_GAP;
    // The system counts up from the foot of the display: the first row is
    // the highest.
    let top = display.y + (display.height + all) / 2.;
    (0..count)
        .map(|nth| {
            let (row, place) = (nth / across, nth % across);
            let here = (count - row * across).min(across) as f64;
            let wide = here * width + (here - 1.) * ROW_GAP;
            let left = display.x + ((display.width - wide) / 2.).max(ROW_GAP);
            Frame {
                x: (left + place as f64 * (width + ROW_GAP)).round(),
                y: (top - (row as f64 + 1.) * height - row as f64 * ROW_GAP).round(),
                width,
                height,
            }
        })
        .collect()
}

/// Where a window is, and the part of its display that windows may use, in
/// the system's terms. `None` where the system is not asked.
pub(super) fn frames_of(window: &Window) -> Option<(Frame, Frame)> {
    let native = native::Native::of(window)?;
    Some((native.frame()?, native.display()?))
}

impl Frame {
    /// How far to the right it is: what papers are put in order by.
    pub(super) fn left(&self) -> f64 {
        self.x
    }
}

impl Workspace {
    /// Glide the window to `to`: one paper of several being laid side by
    /// side, or put back where it was.
    pub(super) fn glide_window(&mut self, to: Frame, window: &mut Window, cx: &mut Context<Self>) {
        if window.is_fullscreen() {
            return;
        }
        let Some(native) = native::Native::of(window) else {
            return;
        };
        let Some(from) = native.frame() else { return };
        native.hold_shape(None);
        self.reshaping = true;
        let still = cx.reduce_motion();
        self._reshape = cx.spawn(async move |this, cx| {
            let started = Instant::now();
            loop {
                let progress = match still {
                    true => 1.,
                    false => (started.elapsed().as_secs_f32() / RESHAPE.as_secs_f32()).min(1.),
                };
                native.set_frame(from.towards(to, motion::ease(progress) as f64));
                if progress >= 1. {
                    break;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(7))
                    .await;
                if this.update(cx, |_, _| ()).is_err() {
                    return;
                }
            }
            this.update(cx, |this, cx| {
                this.reshaping = false;
                if this.mode == Mode::Read {
                    native.hold_shape(Some(to.height / to.width));
                }
                cx.notify();
            })
            .ok();
        });
    }

    /// Give the window the shape of the mode it is now in, gliding there.
    pub(super) fn reshape_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.is_fullscreen() {
            return;
        }
        let Some(native) = native::Native::of(window) else {
            return;
        };
        let (Some(from), Some(display)) = (native.frame(), native.display()) else {
            return;
        };
        let mode = self.mode;
        let to = destination(mode, from, display, self.page, self.desk);
        // The editor's window is any shape the user likes.
        native.hold_shape(None);
        self.reshaping = true;
        let still = cx.reduce_motion();
        self._reshape = cx.spawn(async move |this, cx| {
            let started = Instant::now();
            loop {
                let progress = match still {
                    true => 1.,
                    false => (started.elapsed().as_secs_f32() / RESHAPE.as_secs_f32()).min(1.),
                };
                // Outside any update of the app: moving the window makes the
                // system call straight back into it to lay the window out again.
                native.set_frame(from.towards(to, motion::ease(progress) as f64));
                if progress >= 1. {
                    break;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(7))
                    .await;
                // The window may have been closed on the way.
                if this.update(cx, |_, _| ()).is_err() {
                    return;
                }
            }
            this.update(cx, |this, cx| {
                this.reshaping = false;
                if this.mode == Mode::Read {
                    native.hold_shape(Some(to.height / to.width));
                }
                cx.notify();
            })
            .ok();
        });
    }

    /// Hold a page to the proportions of a sheet of paper, so that dragging
    /// its edge makes a bigger or smaller sheet, not a different shape.
    pub(super) fn shape_window(&mut self, window: &mut Window) {
        if self.mode == Mode::Read
            && !window.is_fullscreen()
            && let Some(native) = native::Native::of(window)
        {
            // The shape it has now: a sheet with its margins.
            if let Some(frame) = native.frame() {
                native.hold_shape(Some(frame.height / frame.width));
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod native {
    use objc2::encode::{Encode, Encoding};
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    use super::Frame;

    const POINT: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
    const SIZE: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);

    // SAFETY: four doubles laid out as AppKit's own rectangle: origin, then size.
    unsafe impl Encode for Frame {
        const ENCODING: Encoding = Encoding::Struct("CGRect", &[POINT, SIZE]);
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Extent {
        width: f64,
        height: f64,
    }

    // SAFETY: two doubles laid out as AppKit's own size.
    unsafe impl Encode for Extent {
        const ENCODING: Encoding = SIZE;
    }

    /// The system's own window behind one of ours.
    #[derive(Clone, Copy)]
    pub struct Native(*mut AnyObject);

    impl Native {
        pub fn of(window: &gpui_kit::Window) -> Option<Self> {
            let handle = HasWindowHandle::window_handle(window).ok()?;
            let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
                return None;
            };
            let view = handle.ns_view.as_ptr() as *mut AnyObject;
            // SAFETY: the view is alive for as long as the window we were handed.
            let window: *mut AnyObject = unsafe { msg_send![view, window] };
            (!window.is_null()).then_some(Self(window))
        }

        pub fn frame(self) -> Option<Frame> {
            // SAFETY: `frame` takes nothing and returns a rectangle.
            Some(unsafe { msg_send![self.0, frame] })
        }

        /// The part of the window's display that windows may use: without the
        /// menu bar and the Dock.
        pub fn display(self) -> Option<Frame> {
            // SAFETY: both messages take nothing; a window off every display has no screen.
            unsafe {
                let screen: *mut AnyObject = msg_send![self.0, screen];
                (!screen.is_null()).then(|| msg_send![screen, visibleFrame])
            }
        }

        pub fn set_frame(self, frame: Frame) {
            // SAFETY: a rectangle and a flag, as the message declares.
            unsafe {
                let _: () = msg_send![self.0, setFrame: frame, display: true];
            }
        }

        /// Hold the window to `height / width`, or let it be any shape.
        pub fn hold_shape(self, tall: Option<f64>) {
            // SAFETY: each message takes one size. Setting the increments is
            // how AppKit is told to stop holding a proportion.
            unsafe {
                match tall {
                    Some(tall) => {
                        let shape = Extent {
                            width: 1.,
                            height: tall,
                        };
                        let _: () = msg_send![self.0, setContentAspectRatio: shape];
                    }
                    None => {
                        let free = Extent {
                            width: 1.,
                            height: 1.,
                        };
                        let _: () = msg_send![self.0, setResizeIncrements: free];
                    }
                }
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod native {
    use super::Frame;

    /// Elsewhere the window keeps the shape it was opened with.
    #[derive(Clone, Copy)]
    pub struct Native;

    impl Native {
        pub fn of(_: &gpui_kit::Window) -> Option<Self> {
            None
        }
        pub fn frame(self) -> Option<Frame> {
            None
        }
        pub fn display(self) -> Option<Frame> {
            None
        }
        pub fn set_frame(self, _: Frame) {}
        pub fn hold_shape(self, _: Option<f64>) {}
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: that would pull in the kit's own `#[test]` macro through
    // the parent's glob import and shadow the standard one.
    use super::{
        Frame, MARGIN_LEAST, Mode, PAGE_TALLEST, SHEET, destination, page_size, sheet_width,
        side_by_side, with_margins,
    };
    use gpui_kit::{px, size};

    const DISPLAY: Frame = Frame {
        x: 0.,
        y: 80.,
        width: 1512.,
        height: 900.,
    };

    #[test]
    fn papers_side_by_side_are_one_size_in_rows_that_fit() {
        let sheet_in = |width: f64| (width / 1.14).min(width - 2. * MARGIN_LEAST as f64);
        // Two fit at their full height, in the middle.
        let two = side_by_side(DISPLAY, 2);
        assert_eq!(two.len(), 2);
        assert_eq!((two[0].width, two[0].height), (two[1].width, two[1].height));
        assert_eq!(two[0].height, 868.);
        assert_eq!(two[0].width, with_margins(868. / SHEET as f64));
        assert_eq!((two[0].y, two[1].y), (96., 96.));
        assert_eq!(two[1].x, two[0].x + two[0].width + 16.);
        let (left, right) = (two[0].x, 1512. - (two[1].x + two[1].width));
        assert!((left - right).abs() <= 1., "{left} and {right}");
        // Four would be too narrow in a row: two rows of two, the first above.
        let four = side_by_side(DISPLAY, 4);
        assert_eq!((four[0].y, four[1].y), (four[1].y, four[0].y));
        assert_eq!(four[2].y, four[3].y);
        assert!(four[0].y > four[2].y + four[2].height);
        assert_eq!(four[0].x, four[2].x);
        // Five: three, then two in the middle under them.
        let five = side_by_side(DISPLAY, 5);
        assert!(five[3].x > five[0].x && five[4].x + five[4].width < five[2].x + five[2].width);
        for paper in two.iter().chain(&four).chain(&five) {
            // Each is a sheet, no narrower than a window is, and on the display.
            assert!(paper.width >= 380.);
            let sheet = sheet_in(paper.width) * SHEET as f64;
            assert!((paper.height - sheet).abs() <= 1., "{paper:?}");
            assert!(paper.x >= 16. && paper.x + paper.width <= 1512. - 16.);
            assert!(paper.y >= DISPLAY.y && paper.y + paper.height <= DISPLAY.y + DISPLAY.height);
        }
        assert!(side_by_side(DISPLAY, 0).is_empty());
    }

    #[test]
    fn a_page_is_a_sheet_hanging_from_the_same_top_edge() {
        let desk = Frame {
            x: 100.,
            y: 120.,
            width: 1280.,
            height: 820.,
        };
        let page = destination(Mode::Read, desk, DISPLAY, None, None);
        assert_eq!(page.height, 860.);
        // The sheet inside is A4; the window is that and a margin at each side.
        let sheet = page.height / SHEET as f64;
        assert_eq!(page.width, with_margins(sheet));
        // To within the point the width is rounded by.
        assert!(page.width - sheet >= 2. * MARGIN_LEAST as f64 - 1.);
        // Same top edge, same centre line.
        assert_eq!(page.y + page.height, desk.y + desk.height);
        assert!((page.x + page.width / 2. - 740.).abs() <= 1.);
    }

    #[test]
    fn the_desk_comes_back_at_its_own_size_and_stays_on_the_display() {
        let page = Frame {
            x: 1100.,
            y: 100.,
            width: 600.,
            height: 848.,
        };
        let desk = destination(Mode::Edit, page, DISPLAY, None, Some([0., 0., 1400., 700.]));
        assert_eq!((desk.width, desk.height), (1400., 700.));
        // Centred on the page it would hang off the right edge: it is brought back.
        assert_eq!(desk.x + desk.width, DISPLAY.x + DISPLAY.width);
        assert_eq!(desk.y + desk.height, page.y + page.height);
    }

    #[test]
    fn a_page_fits_the_display_it_opens_on() {
        let small = page_size(size(px(1280.), px(700.)));
        assert_eq!(small.height, px(660.));
        let tall = page_size(size(px(3000.), px(2000.)));
        assert_eq!(tall.height, px(PAGE_TALLEST));
        // The text gets the whole sheet: the margins are not taken out of it.
        let text = sheet_width(small);
        assert!((small.height / text - SHEET).abs() < 0.01);
        // On a small display the margins are still wide enough for the rail.
        assert!(small.width >= text + px(2. * MARGIN_LEAST - 1.));
    }
}
