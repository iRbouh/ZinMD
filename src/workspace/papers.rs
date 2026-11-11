//! Papers: a file can have a window of its own, the shape of a sheet.
//!
//! Opening a file from outside the app, or with the command key held on a
//! file, a link or what a search found, makes a new paper. A plain click
//! still turns the page of the window it is made in. The papers are the
//! user's to place: side by side, overlapping, on another screen.
//!
//! The app keeps the papers that are open in the order they were last used,
//! which is the order the switcher shows them in (see [`super::switcher`]),
//! and says in the session where each one was, so that they come back.

use std::path::{Path, PathBuf};
use std::time::Instant;

use gpui_kit::*;

use super::Workspace;
use super::frame::{self, Frame};
use super::switcher::Switcher;
use crate::session::Session;
use crate::startup;
use crate::system;
use crate::theme::size;

/// One paper: a window, and what is in it.
pub(super) struct Paper {
    pub window: AnyWindowHandle,
    pub workspace: WeakEntity<Workspace>,
    /// The file it shows and where its window is, as the paper last said.
    shows: Option<PathBuf>,
    frame: [f32; 4],
}

/// Every paper that is open, the one used last first.
#[derive(Default)]
pub(super) struct Papers {
    open: Vec<Paper>,
    /// How many papers have been opened beside another: each is put a step
    /// further down and to the right, so that they do not hide each other.
    steps: usize,
    /// The switcher, while it is up.
    switcher: Option<(AnyWindowHandle, WeakEntity<Switcher>)>,
    /// Where each paper was before they were laid side by side: what
    /// putting them back goes by.
    before_row: Option<Vec<(AnyWindowHandle, Frame)>>,
}

impl Global for Papers {}

/// How far down and to the right of the paper it is opened from a new paper
/// is put.
const STEP: f32 = 32.;

/// The window of a paper: no bar of the system's across its top, and the
/// buttons of the window set in the app's own bar.
pub fn window_options(bounds: Bounds<Pixels>) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(gpui_kit::size(px(380.), px(420.))),
        titlebar: Some(TitlebarOptions {
            title: None,
            appears_transparent: true,
            // Centres the window buttons in our taller title bar.
            traffic_light_position: Some(point(px(14.), (size::TITLEBAR - px(16.)) / 2.)),
        }),
        // Our title bar moves the window itself, so clicks on it are never delayed.
        app_owns_titlebar_drag: true,
        // On Linux the bar at the top is ours too, not a second one from
        // the desktop, and the window answers to the name of its launcher.
        window_decorations: Some(WindowDecorations::Client),
        app_id: Some(system::APP_ID.into()),
        focus: cfg!(not(feature = "shot")),
        // Scripted runs stay in the background, where frames are normally
        // held to 30 a second; lift that so they measure real smoothness.
        #[cfg(feature = "shot")]
        inactive_frame_interval: None,
        ..Default::default()
    }
}

/// Where a page that was at `frame` (x, y, width, height) goes, if that
/// place still exists: a display may have been unplugged since. Otherwise a
/// sheet as tall as the display has room for, in the centre.
pub fn page_bounds(frame: Option<[f32; 4]>, cx: &App) -> Bounds<Pixels> {
    let fallback = || {
        let room = cx
            .primary_display()
            .map_or(gpui_kit::size(px(1440.), px(900.)), |display| {
                display.visible_bounds().size
            });
        Bounds::centered(None, super::page_size(room), cx)
    };
    let Some([x, y, width, height]) = frame else {
        return fallback();
    };
    // A page has the shape of a sheet with its margins, whatever shape it was
    // left in: it comes back as tall as it was, about the same centre line.
    let sheet: f32 = super::page_width(px(height)).into();
    let (x, width) = (x + (width - sheet) / 2., sheet);
    let bounds = Bounds::new(point(px(x), px(y)), gpui_kit::size(px(width), px(height)));
    // Enough of the title bar must be on some display to grab the window by.
    let grip = Bounds::new(
        bounds.origin,
        gpui_kit::size(bounds.size.width, size::TITLEBAR),
    );
    let reachable = cx.displays().iter().any(|display| {
        let overlap = display.bounds().intersect(&grip);
        overlap.size.width >= px(120.) && overlap.size.height >= px(20.)
    });
    if reachable { bounds } else { fallback() }
}

/// Open a paper: a window showing `path`, or an empty page. `session` is
/// what it starts from: the folders of its side panel, and its ways. The
/// first paper of a run is `primary`: it brings back what was open last
/// time and looks for updates.
pub fn open_paper(
    path: Option<PathBuf>,
    session: Session,
    bounds: Bounds<Pixels>,
    started: Instant,
    primary: bool,
    cx: &mut App,
) -> Option<(AnyWindowHandle, Entity<Workspace>)> {
    let frame = [
        bounds.origin.x.into(),
        bounds.origin.y.into(),
        bounds.size.width.into(),
        bounds.size.height.into(),
    ];
    let shows = path.clone();
    let (window, workspace) = gpui_kit::open_window(window_options(bounds), cx, |window, cx| {
        startup::mark("native window created");
        cx.new(|cx| Workspace::new(path, session, started, primary, window, cx))
    })
    .ok()?;
    cx.default_global::<Papers>().open.insert(
        0,
        Paper {
            window,
            workspace: workspace.downgrade(),
            shows,
            frame,
        },
    );
    Some((window, workspace))
}

/// The papers that are open, the one used last first. Those whose window
/// has been closed are forgotten on the way.
pub(super) fn open_papers(cx: &mut App) -> Vec<(AnyWindowHandle, Entity<Workspace>)> {
    let papers = cx.default_global::<Papers>();
    papers
        .open
        .retain(|paper| paper.workspace.upgrade().is_some());
    papers
        .open
        .iter()
        .filter_map(|paper| Some((paper.window, paper.workspace.upgrade()?)))
        .collect()
}

/// How many papers are open.
pub(super) fn count(cx: &App) -> usize {
    cx.try_global::<Papers>().map_or(0, |papers| {
        papers
            .open
            .iter()
            .filter(|paper| paper.workspace.upgrade().is_some())
            .count()
    })
}

/// A paper was brought to the front: it is the one used last.
pub(super) fn came_forward(window: AnyWindowHandle, cx: &mut App) {
    let papers = cx.default_global::<Papers>();
    if let Some(at) = papers.open.iter().position(|paper| paper.window == window)
        && at > 0
    {
        let paper = papers.open.remove(at);
        papers.open.insert(0, paper);
    }
}

/// A paper says what it shows and where its window is.
pub(super) fn note(window: AnyWindowHandle, shows: Option<PathBuf>, frame: [f32; 4], cx: &mut App) {
    let known = cx.try_global::<Papers>().and_then(|papers| {
        let paper = papers.open.iter().find(|paper| paper.window == window)?;
        Some(paper.shows == shows && paper.frame == frame)
    });
    // Said every frame: nothing is touched unless something changed.
    if known != Some(false) {
        return;
    }
    let papers = cx.default_global::<Papers>();
    if let Some(paper) = papers.open.iter_mut().find(|paper| paper.window == window) {
        (paper.shows, paper.frame) = (shows, frame);
    }
}

/// The other papers than the one in `window`: the file each shows and where
/// its window is. What the session keeps of them.
pub(super) fn others(window: AnyWindowHandle, cx: &App) -> Vec<(PathBuf, [f32; 4])> {
    let Some(papers) = cx.try_global::<Papers>() else {
        return Vec::new();
    };
    papers
        .open
        .iter()
        .filter(|paper| paper.window != window && paper.workspace.upgrade().is_some())
        .filter_map(|paper| Some((paper.shows.clone()?, paper.frame)))
        .collect()
}

/// Where the window of each paper is, as the papers last said.
pub(super) fn frames(cx: &App) -> Vec<(AnyWindowHandle, [f32; 4])> {
    cx.try_global::<Papers>().map_or(Vec::new(), |papers| {
        papers
            .open
            .iter()
            .map(|paper| (paper.window, paper.frame))
            .collect()
    })
}

/// The paper that shows `path`, if one does.
pub(super) fn showing(path: &Path, cx: &App) -> Option<AnyWindowHandle> {
    let papers = cx.try_global::<Papers>()?;
    papers
        .open
        .iter()
        .find(|paper| paper.shows.as_deref() == Some(path) && paper.workspace.upgrade().is_some())
        .map(|paper| paper.window)
}

/// The switcher came up.
pub(super) fn switcher_opened(
    window: AnyWindowHandle,
    switcher: WeakEntity<Switcher>,
    cx: &mut App,
) {
    cx.default_global::<Papers>().switcher = Some((window, switcher));
}

/// The switcher, if it is up.
pub(super) fn switcher(cx: &mut App) -> Option<(AnyWindowHandle, Entity<Switcher>)> {
    let papers = cx.default_global::<Papers>();
    let (window, switcher) = papers.switcher.clone()?;
    match switcher.upgrade() {
        Some(switcher) => Some((window, switcher)),
        None => {
            papers.switcher = None;
            None
        }
    }
}

/// Lay the papers side by side on the display of the one in front, in the
/// order they are in from left to right; or, when they were laid so by the
/// last call, put each back where it was. They glide there.
pub(super) fn lay_side_by_side(cx: &mut App) {
    let open = open_papers(cx);
    let glide = |paper: AnyWindowHandle, workspace: &Entity<Workspace>, to: Frame, cx: &mut App| {
        paper
            .update(cx, |_, window, cx| {
                workspace.update(cx, |workspace, cx| workspace.glide_window(to, window, cx));
            })
            .ok();
    };
    // Back where they were.
    if let Some(before) = cx.default_global::<Papers>().before_row.take() {
        for (paper, to) in before {
            if let Some((_, workspace)) = open.iter().find(|(window, _)| *window == paper) {
                glide(paper, workspace, to, cx);
            }
        }
        return;
    }
    // Where each is now, and the display of the one in front.
    let mut placed: Vec<(AnyWindowHandle, Entity<Workspace>, Frame, Frame)> = open
        .into_iter()
        .filter_map(|(paper, workspace)| {
            let (at, display) = paper
                .update(cx, |_, window, _| frame::frames_of(window))
                .ok()??;
            Some((paper, workspace, at, display))
        })
        .collect();
    let Some(display) = placed.first().map(|(.., display)| *display) else {
        return;
    };
    placed.sort_by(|a, b| a.2.left().total_cmp(&b.2.left()));
    let row = frame::side_by_side(display, placed.len());
    startup::trace(|| format!("papers side by side on {display:?}: {row:?}"));
    let before = placed
        .iter()
        .map(|(paper, _, at, _)| (*paper, *at))
        .collect();
    cx.default_global::<Papers>().before_row = Some(before);
    for ((paper, workspace, ..), to) in placed.iter().zip(row) {
        glide(*paper, workspace, to, cx);
    }
}

/// How far from the paper it is opened beside the next new paper goes: each
/// a step further than the last, then round again.
pub(super) fn next_step(cx: &mut App) -> Pixels {
    let papers = cx.default_global::<Papers>();
    papers.steps += 1;
    px(STEP * ((papers.steps - 1) % 6 + 1) as f32)
}

/// Open what the system asked for: files double-clicked in its file
/// manager, or dropped on the app. A folder joins the side panel of the
/// paper in front. A file is a new paper, unless the paper in front is an
/// empty page waiting for one.
pub fn open_requested(paths: Vec<PathBuf>, cx: &mut App) {
    for path in paths {
        let Some((window, workspace)) = open_papers(cx).into_iter().next() else {
            continue;
        };
        window
            .update(cx, |_, window, cx| {
                workspace.update(cx, |workspace, cx| workspace.open_asked(path, window, cx));
            })
            .ok();
    }
    cx.activate(true);
}
