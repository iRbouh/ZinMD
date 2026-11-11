//! Looking closely at a picture or a diagram.
//!
//! A click on either opens it large over the page. There it can be zoomed
//! (the wheel, a pinch, or the buttons), moved by dragging, copied, and
//! saved. A diagram is drawn again at a finer grain once it is open, so it
//! stays sharp when zoomed. Escape, or a click beside the picture, goes back
//! to the page.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::assets::IconName;
use gpui_kit::base::Transition;
use gpui_kit::base::transition;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::Workspace;
use crate::preview::{Looking, Subject, mermaid};
use crate::theme::{self, ActivePalette as _, Palette, size};
use crate::{motion, system};

/// Room kept clear above the picture, for its name and the way out.
const TOP: Pixels = px(52.);
/// Room kept clear under it, for the buttons and the line above them.
const FOOT: Pixels = px(104.);
/// Room kept clear at each side.
const SIDE: Pixels = px(28.);
/// How close a picture can be brought: this many times the size it fits at.
const CLOSEST: f32 = 8.;
/// What one press of a zoom button, or one step of a wheel, multiplies by.
const STEP: f32 = 1.25;
/// How many pixels to the point a diagram is drawn again at once opened.
const FINE: f32 = 5.;
/// How long a button says "copied" or "saved".
const SAID: Duration = Duration::from_millis(1400);

/// What a button under the picture has just done.
#[derive(Clone, Copy, PartialEq)]
enum Done {
    Copied,
    CopiedSource,
    CopiedText,
    Saved,
}

pub(super) struct Viewer {
    subject: Subject,
    /// 1 fits the window; more is closer.
    zoom: f32,
    /// How far the middle of the picture is from the middle of its room.
    offset: Point<Pixels>,
    /// Whether the last change glides (a button, a key, a double click) or
    /// follows the hand as it is (the wheel, a pinch, a drag).
    glides: bool,
    /// Where the pointer was when it last moved with the button down, and
    /// whether it has moved since the button went down.
    held: Option<(Point<Pixels>, bool)>,
    /// How big the whole picture is when it fits its room, once that is known.
    whole: Option<Size<Pixels>>,
    /// A diagram drawn again at a finer grain than the page needs.
    fine: Option<Arc<Image>>,
    done: Option<(Done, Instant)>,
    _work: Task<()>,
    _said: Task<()>,
}

impl Workspace {
    /// Open what the page last asked to have looked at closely.
    pub(super) fn look_closer(&mut self, cx: &mut Context<Self>) {
        let Some(subject) = cx.default_global::<Looking>().0.take() else {
            return;
        };
        // A diagram is drawn again, finely, while it glides open.
        let work = match &subject {
            Subject::Diagram { code, .. } => {
                let (code, palette) = (code.clone(), theme::settled(cx));
                cx.spawn(async move |this, cx| {
                    let drawn = cx
                        .background_executor()
                        .spawn(async move { mermaid::sharper(&code, &palette, FINE) })
                        .await;
                    if let Ok((fine, _)) = drawn {
                        this.update(cx, |this, cx| {
                            if let Some(viewer) = &mut this.viewer {
                                viewer.fine = Some(fine);
                                cx.notify();
                            }
                        })
                        .ok();
                    }
                })
            }
            Subject::Picture { .. } => Task::ready(()),
        };
        self.viewer = Some(Viewer {
            subject,
            zoom: 1.,
            offset: Point::default(),
            glides: false,
            held: None,
            whole: None,
            fine: None,
            done: None,
            _work: work,
            _said: Task::ready(()),
        });
        self.viewer_open = true;
        cx.notify();
    }

    /// Open a picture or a diagram over the page, for scripted runs.
    #[cfg(feature = "shot")]
    pub(super) fn debug_look(&mut self, what: &str, window: &mut Window, cx: &mut Context<Self>) {
        let subject = match what {
            "diagram" => {
                let code = "flowchart TD\n  A[Open file] --> B{Markdown?}\n  B -- yes --> C[Parse]\n  B -- no --> D[Show as text]\n  C --> E[Blocks]\n  E --> F[Paint]\n  D --> F";
                let Ok((picture, size)) = mermaid::sharper(code, &theme::settled(cx), 2.) else {
                    return;
                };
                Subject::Diagram {
                    code: code.into(),
                    picture,
                    size,
                }
            }
            file => Subject::Picture {
                source: ImageSource::Resource(Resource::Path(std::path::Path::new(file).into())),
                name: "A picture, opened from a script".into(),
                file: Some(PathBuf::from(file)),
            },
        };
        crate::preview::look_at(subject, window, cx);
    }

    pub(super) fn close_viewer(&mut self, cx: &mut Context<Self>) {
        self.viewer_open = false;
        cx.notify();
    }

    /// The keys that act on the open picture. Says whether `key` was one.
    pub(super) fn viewer_key(
        &mut self,
        key: &Keystroke,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.viewer_open {
            return false;
        }
        let middle = stage(window).1;
        match key.key.as_str() {
            "escape" => self.close_viewer(cx),
            "=" | "+" => self.zoom_viewer(STEP, middle, true, window, cx),
            "-" => self.zoom_viewer(1. / STEP, middle, true, window, cx),
            "0" => self.fit_viewer(cx),
            "c" => self.copy_viewed(cx),
            _ => return false,
        }
        true
    }

    /// Bring the picture `by` times closer, keeping what is at `about` where
    /// it is.
    fn zoom_viewer(
        &mut self,
        by: f32,
        about: Point<Pixels>,
        glides: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let (room, middle) = stage(window);
        let Some(viewer) = &mut self.viewer else {
            return;
        };
        let zoom = (viewer.zoom * by).clamp(1., CLOSEST);
        let by = zoom / viewer.zoom;
        let from_middle = about - middle;
        viewer.offset = point(
            viewer.offset.x * by + from_middle.x * (1. - by),
            viewer.offset.y * by + from_middle.y * (1. - by),
        );
        viewer.zoom = zoom;
        viewer.glides = glides;
        viewer.keep_in_reach(room);
        cx.notify();
    }

    fn fit_viewer(&mut self, cx: &mut Context<Self>) {
        if let Some(viewer) = &mut self.viewer {
            viewer.zoom = 1.;
            viewer.offset = Point::default();
            viewer.glides = true;
            cx.notify();
        }
    }

    fn move_viewed(&mut self, by: Point<Pixels>, window: &Window, cx: &mut Context<Self>) {
        let room = stage(window).0;
        if let Some(viewer) = &mut self.viewer {
            viewer.offset += by;
            viewer.glides = false;
            viewer.keep_in_reach(room);
            cx.notify();
        }
    }

    fn copy_viewed(&mut self, cx: &mut Context<Self>) {
        let Some(viewer) = &self.viewer else { return };
        // The finer drawing of a diagram, when it is there.
        let copied = match (&viewer.subject, &viewer.fine) {
            (Subject::Diagram { .. }, Some(fine)) => {
                cx.write_to_clipboard(ClipboardItem::new_image(fine));
                true
            }
            (subject, _) => subject.copy(cx),
        };
        if copied {
            self.viewer_did(Done::Copied, cx);
        }
    }

    fn copy_viewed_source(&mut self, cx: &mut Context<Self>) {
        if let Some(Viewer {
            subject: Subject::Diagram { code, .. },
            ..
        }) = &self.viewer
        {
            cx.write_to_clipboard(ClipboardItem::new_string(code.to_string()));
            self.viewer_did(Done::CopiedSource, cx);
        }
    }

    /// Copy a diagram drawn with characters, for pasting into plain text.
    fn copy_viewed_as_text(&mut self, cx: &mut Context<Self>) {
        if let Some(Viewer {
            subject: Subject::Diagram { code, .. },
            ..
        }) = &self.viewer
            && let Ok(text) = mermaid::as_text(code)
        {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            self.viewer_did(Done::CopiedText, cx);
        }
    }

    /// Save the picture where the user says: a diagram as a PNG or, with
    /// `as_drawing`, as an SVG that stays sharp at any size; a picture as the
    /// file it is.
    fn save_viewed(&mut self, as_drawing: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(viewer) = &self.viewer else { return };
        let subject = viewer.subject.clone();
        let fine = viewer.fine.clone();
        let folder = self
            .active_doc()
            .and_then(|doc| doc.path.as_deref()?.parent().map(PathBuf::from))
            .or_else(system::home)
            .unwrap_or_default();
        let name = match &subject {
            Subject::Diagram { .. } if as_drawing => "diagram.svg".to_string(),
            Subject::Diagram { .. } => "diagram.png".to_string(),
            Subject::Picture { file, .. } => file
                .as_deref()
                .and_then(|file| file.file_name()?.to_str())
                .unwrap_or("picture.png")
                .to_string(),
        };
        // What to write, worked out here: the writing itself is done off
        // the thread that draws the window.
        enum What {
            Drawing(SharedString, Box<Palette>),
            Bytes(Vec<u8>),
            File(PathBuf),
        }
        let what = match &subject {
            Subject::Diagram { code, .. } if as_drawing => {
                What::Drawing(code.clone(), Box::new(theme::settled(cx)))
            }
            Subject::Diagram { picture, .. } => {
                What::Bytes(fine.as_ref().unwrap_or(picture).bytes().to_vec())
            }
            Subject::Picture {
                file: Some(file), ..
            } => What::File(file.clone()),
            subject => match subject.image() {
                Some(image) => What::Bytes(image.bytes().to_vec()),
                None => return,
            },
        };
        let chosen = cx.prompt_for_new_path(&folder, Some(&name));
        self._viewer_save = cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = chosen.await else {
                return;
            };
            let written = cx
                .background_executor()
                .spawn(async move {
                    let bytes = match what {
                        What::Drawing(code, palette) => {
                            mermaid::svg(&code, &palette).ok()?.into_bytes()
                        }
                        What::Bytes(bytes) => bytes,
                        What::File(file) => std::fs::read(file).ok()?,
                    };
                    std::fs::write(path, bytes).ok()
                })
                .await;
            if written.is_some() {
                this.update(cx, |this, cx| this.viewer_did(Done::Saved, cx))
                    .ok();
            }
        });
    }

    /// Have a button say what it did, for a moment.
    fn viewer_did(&mut self, done: Done, cx: &mut Context<Self>) {
        let said = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAID).await;
            this.update(cx, |this, cx| {
                if let Some(viewer) = &mut this.viewer {
                    viewer.done = None;
                    cx.notify();
                }
            })
            .ok();
        });
        if let Some(viewer) = &mut self.viewer {
            viewer.done = Some((done, Instant::now()));
            viewer._said = said;
            cx.notify();
        }
    }

    /// The picture, large over the page, while one is open or on its way out.
    pub(super) fn render_viewer(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let presence = transition(
            "viewer",
            if self.viewer_open { 1. } else { 0. },
            motion::glide(motion::PANEL, cx),
            window,
            cx,
        );
        if presence <= 0.001 {
            if !self.viewer_open {
                self.viewer = None;
            }
            return None;
        }
        let p = *cx.palette();
        let (room, middle) = stage(window);
        let viewer = self.viewer.as_ref()?;

        // Where it is going, reached at once or by a short glide.
        let pace = match viewer.glides {
            true => motion::glide(motion::QUICK, cx),
            false => Transition::new(Duration::ZERO),
        };
        let zoom = transition("viewer-zoom", viewer.zoom, pace.clone(), window, cx);
        let (x, y) = (
            transition(
                "viewer-x",
                f32::from(viewer.offset.x),
                pace.clone(),
                window,
                cx,
            ),
            transition("viewer-y", f32::from(viewer.offset.y), pace, window, cx),
        );
        // The shape of the picture, once it is known: a diagram's from its
        // drawing, a picture's once it has been read. It fits its room at
        // that shape, and grows from there by the zoom, past its own size.
        let shape: Option<f32> = match &viewer.subject {
            Subject::Diagram { size, .. } => Some(size.width / size.height),
            Subject::Picture {
                source: ImageSource::Resource(resource),
                ..
            } => window
                .use_asset::<ImageAssetLoader>(resource, cx)
                .and_then(Result::ok)
                .map(|read| {
                    let size = read.size(0);
                    size.width.0 as f32 / size.height.0.max(1) as f32
                }),
            Subject::Picture { .. } => None,
        };
        let whole = match shape {
            Some(shape) if room.width / room.height > shape => {
                gpui_kit::size(room.height * shape, room.height)
            }
            Some(shape) => gpui_kit::size(room.width, room.width / shape),
            None => room,
        };
        if let Some(viewer) = &mut self.viewer {
            viewer.whole = shape.map(|_| whole);
        }
        let viewer = self.viewer.as_ref()?;
        // It grows a little as it arrives.
        let grown = zoom * (0.96 + 0.04 * presence);
        let (width, height) = (whole.width * grown, whole.height * grown);
        let (left, top) = (
            middle.x - width / 2. + px(x),
            middle.y - height / 2. + px(y),
        );

        let (name, source): (SharedString, ImageSource) = match &viewer.subject {
            Subject::Picture { source, name, .. } => (name.clone(), source.clone()),
            Subject::Diagram { picture, .. } => (
                "Diagram".into(),
                viewer
                    .fine
                    .clone()
                    .unwrap_or_else(|| picture.clone())
                    .into(),
            ),
        };
        // Until the finer drawing of a diagram has loaded, the one from the
        // page stays.
        let coarse = match &viewer.subject {
            Subject::Diagram { picture, .. } => Some(picture.clone()),
            Subject::Picture { .. } => None,
        };
        let frame = div()
            .absolute()
            .left(left)
            .top(top)
            .w(width)
            .h(height)
            .flex()
            .items_center()
            .justify_center();
        let picture = match shape {
            // A box of the picture's own shape, which the picture fills.
            Some(_) => frame.child(img(source).size_full().when_some(
                coarse,
                move |image, coarse| {
                    image.with_loading(move || img(coarse.clone()).size_full().into_any_element())
                },
            )),
            // Its shape is not known yet: it is as big as fits, in the middle.
            None => frame.child(
                img(source)
                    .flex_none()
                    .max_w(width)
                    .max_h(height)
                    .object_fit(ObjectFit::Contain),
            ),
        };

        let bars = self.render_viewer_bars(name, zoom, &p, cx);
        Some(
            div()
                .id("viewer")
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                // Nothing of the page shows through: a diagram is see-through
                // between its shapes, and words behind it would muddle it.
                .bg(p.surface_preview)
                .opacity(presence)
                // The page under it is out of reach while it is up.
                .occlude()
                .cursor(match viewer.held {
                    Some(_) => CursorStyle::ClosedHand,
                    None if viewer.zoom > 1. => CursorStyle::OpenHand,
                    None => CursorStyle::Arrow,
                })
                .on_scroll(cx.listener(|this, event: &ScrollEvent, window, cx| {
                    cx.stop_propagation();
                    // How two fingers on a trackpad are taken: they move the
                    // picture once it is bigger than its room, and zoom while
                    // all of it is in view and there is nowhere to move it to.
                    let whole = this.viewer.as_ref().is_none_or(|viewer| viewer.zoom <= 1.);
                    match event.delta {
                        // A wheel always zooms.
                        ScrollDelta::Lines(lines) => {
                            let by = STEP.powf(lines.y.clamp(-3., 3.));
                            this.zoom_viewer(by, event.position, false, window, cx)
                        }
                        ScrollDelta::Pixels(moved)
                            if whole || system::command_held(&event.modifiers) =>
                        {
                            let by = (f32::from(moved.y) * 0.012).exp();
                            this.zoom_viewer(by, event.position, false, window, cx)
                        }
                        ScrollDelta::Pixels(moved) => this.move_viewed(moved, window, cx),
                    }
                }))
                .on_pinch(cx.listener(|this, event: &PinchEvent, window, cx| {
                    cx.stop_propagation();
                    this.zoom_viewer(1. + event.delta, event.position, false, window, cx)
                }))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        if event.click_count == 2 {
                            // A double click goes in, or back out to the whole picture.
                            return match this.viewer.as_ref().is_some_and(|viewer| viewer.zoom > 1.)
                            {
                                true => this.fit_viewer(cx),
                                false => this.zoom_viewer(2.5, event.position, true, window, cx),
                            };
                        }
                        if let Some(viewer) = &mut this.viewer {
                            viewer.held = Some((event.position, false));
                            cx.notify();
                        }
                    }),
                )
                .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                    let Some(Some((from, _))) = this.viewer.as_ref().map(|viewer| viewer.held)
                    else {
                        return;
                    };
                    if event.pressed_button != Some(MouseButton::Left) {
                        return;
                    }
                    if let Some(viewer) = &mut this.viewer {
                        viewer.held = Some((event.position, true));
                    }
                    this.move_viewed(event.position - from, window, cx);
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseUpEvent, window, cx| {
                        let Some(viewer) = &mut this.viewer else {
                            return;
                        };
                        let Some((_, moved)) = viewer.held.take() else {
                            return;
                        };
                        cx.notify();
                        // A plain click beside the picture goes back to the page.
                        let (room, middle) = stage(window);
                        let beside = (event.position.x - middle.x).abs() > room.width / 2.
                            || (event.position.y - middle.y).abs() > room.height / 2.;
                        if !moved && beside && viewer.zoom <= 1. {
                            this.close_viewer(cx);
                        }
                    }),
                )
                .child(picture)
                .children(bars)
                .into_any_element(),
        )
    }

    /// What is drawn over the picture: its name and the way out at the top,
    /// the buttons at the foot.
    fn render_viewer_bars(
        &self,
        name: SharedString,
        zoom: f32,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(viewer) = &self.viewer else {
            return Vec::new();
        };
        let did = viewer.done.map(|(done, _)| done);
        let is_diagram = matches!(viewer.subject, Subject::Diagram { .. });
        let can_copy = is_diagram || viewer.subject.image().is_some() || did == Some(Done::Copied);
        let (strong, hover) = (p.text_primary, p.surface_hover);

        // One button: an icon, a tip that names it and its key, and what it does.
        let button = |id: &'static str,
                      icon: IconName,
                      says: &'static str,
                      cx: &mut Context<Self>,
                      does: fn(&mut Self, &mut Window, &mut Context<Self>)| {
            div()
                .id(id)
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(36.))
                .rounded(size::RADIUS_SM)
                .text_color(p.text_secondary)
                .hover(move |style| style.bg(hover).text_color(strong))
                .active(|style| style.opacity(0.7))
                .cursor_pointer()
                .child(Icon::new(icon).size(px(18.)))
                .tooltip(move |window, cx| Tooltip::new(says).build(window, cx))
                // A press on a button is not the start of a drag.
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    does(this, window, cx)
                }))
        };
        let rule = || {
            div()
                .flex_none()
                .w(px(1.))
                .h(px(18.))
                .mx(px(4.))
                .bg(p.border_subtle)
        };
        let tick = |done: Done, icon: IconName| match did == Some(done) {
            true => IconName::Check,
            false => icon,
        };

        let tools = h_flex()
            .h(px(48.))
            .px(px(6.))
            .gap(px(3.))
            .rounded(size::RADIUS_LG)
            .border_1()
            .border_color(p.border_strong)
            .bg(p.surface_raised)
            .shadow_lg()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(button(
                "viewer-out",
                IconName::ZoomOut,
                "Zoom out  (−)",
                cx,
                |this, window, cx| {
                    let middle = stage(window).1;
                    this.zoom_viewer(1. / STEP, middle, true, window, cx)
                },
            ))
            .child(
                div()
                    .id("viewer-fit")
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .w(px(58.))
                    .h(px(36.))
                    .rounded(size::RADIUS_SM)
                    .text_color(p.text_secondary)
                    .hover(move |style| style.bg(hover).text_color(strong))
                    .child(format!("{}%", (zoom * 100.).round()))
                    .tooltip(|window, cx| Tooltip::new("Show it whole  (0)").build(window, cx))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|this, _, _, cx| this.fit_viewer(cx))),
            )
            .child(button(
                "viewer-in",
                IconName::ZoomIn,
                "Zoom in  (+)",
                cx,
                |this, window, cx| {
                    let middle = stage(window).1;
                    this.zoom_viewer(STEP, middle, true, window, cx)
                },
            ))
            .child(rule())
            .when(can_copy, |tools| {
                tools.child(button(
                    "viewer-copy",
                    tick(Done::Copied, IconName::Copy),
                    "Copy the picture  (C)",
                    cx,
                    |this, _, cx| this.copy_viewed(cx),
                ))
            })
            .when(is_diagram, |tools| {
                tools.child(button(
                    "viewer-source",
                    tick(Done::CopiedSource, IconName::Code),
                    "Copy the diagram's source",
                    cx,
                    |this, _, cx| this.copy_viewed_source(cx),
                ))
            })
            .when(is_diagram, |tools| {
                tools.child(button(
                    "viewer-text",
                    tick(Done::CopiedText, IconName::Type),
                    "Copy as a drawing made of text",
                    cx,
                    |this, _, cx| this.copy_viewed_as_text(cx),
                ))
            })
            .child(button(
                "viewer-save",
                tick(Done::Saved, IconName::Download),
                if is_diagram {
                    "Save as a PNG picture…"
                } else {
                    "Save a copy…"
                },
                cx,
                |this, window, cx| this.save_viewed(false, window, cx),
            ))
            .when(is_diagram, |tools| {
                tools.child(button(
                    "viewer-drawing",
                    IconName::FileCode,
                    "Save as an SVG drawing…",
                    cx,
                    |this, window, cx| this.save_viewed(true, window, cx),
                ))
            });

        // Until it has been zoomed, a line says how to: nothing on a
        // picture shows that it can be.
        let hint = (viewer.zoom <= 1.).then(|| {
            div()
                .text_size(size::FONT_UI_SMALL)
                .text_color(p.text_tertiary)
                .child("Pinch or scroll to zoom  ·  drag to move  ·  double-click to go in")
        });
        let foot = div()
            .absolute()
            .left_0()
            .right_0()
            .bottom(px(14.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(8.))
            .text_size(size::FONT_UI)
            .children(hint)
            .child(tools);

        // The name sits in the middle of the top: clear of the window's own
        // buttons at one end, and of the way out at the other. Each is on a
        // ground of its own, because zoomed in the picture runs under them.
        let top = div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(TOP)
            .px(px(96.))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .min_w_0()
                    .max_w_full()
                    .truncate()
                    .px(px(11.))
                    .py(px(6.))
                    .rounded(size::RADIUS_MD)
                    .bg(p.surface_raised.opacity(0.92))
                    .text_size(size::FONT_UI)
                    .text_color(p.text_secondary)
                    .child(name),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right(px(12.))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .rounded(size::RADIUS_MD)
                            .bg(p.surface_raised.opacity(0.92))
                            .child(button(
                                "viewer-close",
                                IconName::X,
                                "Back to the page  (Esc)",
                                cx,
                                |this, _, cx| this.close_viewer(cx),
                            )),
                    ),
            );

        vec![top.into_any_element(), foot.into_any_element()]
    }
}

impl Viewer {
    /// However far it is dragged, some of the picture stays in its room.
    fn keep_in_reach(&mut self, room: Size<Pixels>) {
        // It can be moved as far as it overflows its room, and no further.
        let whole = self.whole.unwrap_or(room);
        let reach = |picture: Pixels, room: Pixels| ((picture * self.zoom - room) / 2.).max(px(0.));
        let (x, y) = (
            reach(whole.width, room.width),
            reach(whole.height, room.height),
        );
        self.offset = point(self.offset.x.clamp(-x, x), self.offset.y.clamp(-y, y));
    }
}

/// The room a picture has in the window, and the middle of that room.
fn stage(window: &Window) -> (Size<Pixels>, Point<Pixels>) {
    let view = window.viewport_size();
    let room = gpui_kit::size(
        (view.width - SIDE * 2.).max(px(80.)),
        (view.height - TOP - FOOT).max(px(80.)),
    );
    (room, point(view.width / 2., TOP + room.height / 2.))
}
