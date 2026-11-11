//! Telling the user about a new version, and restarting into it.
//!
//! Looking for a new version and fetching it happen in the background and
//! say nothing. Once the new version is here and checked, a small card rises
//! at the foot of the window: "ZinMD 0.2.0 is ready", and a button. One
//! click swaps the new version in and starts it, with every file open as it
//! was, unsaved edits included.
//!
//! Asked for by hand ("Check for updates"), every step is said on the card,
//! not only the good news.

use std::sync::atomic::Ordering;
use std::time::Duration;

use gpui_kit::base::transition;
use gpui_kit::component::h_flex;
use gpui_kit::*;

use super::{Mode, Workspace};
use crate::motion;
use crate::settings::Settings;
use crate::system::VERSION;
use crate::theme::{ActivePalette as _, size};
use crate::update::{self, Fetched, Install, Ready, Release};

/// How long a card that only reports stays up.
const LINGER: Duration = Duration::from_secs(6);

/// What there is to say about a new version.
#[derive(Clone)]
pub(super) enum Update {
    Quiet,
    /// Asked for by hand: looking.
    Looking,
    /// Asked for by hand: this is the newest there is.
    Newest,
    /// A new version is on its way down.
    Fetching(Release, Fetched),
    /// A new version is here and checked: one click away.
    Ready(Ready),
    /// A new version is out, for the user to fetch: this copy is not ours to
    /// replace.
    Out(Release),
    /// Something went wrong that the user asked about, or has to know. With
    /// `by_hand`, the way round it is to fetch the new version oneself.
    Failed {
        message: &'static str,
        by_hand: bool,
    },
    /// This copy has just taken the place of an older one.
    Done,
}

/// What a button on the card does.
#[derive(Clone, Copy)]
enum Press {
    Restart,
    Later,
    Open(&'static str),
    WhatsNew,
}

impl Workspace {
    /// Begin looking for new versions. Called once the first frame is on
    /// screen, so none of this holds up opening.
    pub(super) fn start_updates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // A copy that has just replaced an older one says so, once.
        if update::settle().is_some() {
            self.say(Update::Done, Some(LINGER), cx);
        }
        let wanted = cx.global::<Settings>().updates && !update::silenced();
        if !wanted || self.install == Install::Developer {
            return;
        }
        self._update_look = cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(update::FIRST_LOOK).await;
            loop {
                let looking = this.update_in(cx, |this, window, cx| {
                    this.look_for_update(false, window, cx);
                });
                if looking.is_err() {
                    break;
                }
                cx.background_executor().timer(update::EVERY).await;
            }
        });
    }

    /// Look for a newer version, and fetch it if there is one. `asked` is
    /// whether the user asked: then each step is said, whatever it finds.
    pub(super) fn look_for_update(
        &mut self,
        asked: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // One already found is not looked for again.
        if matches!(self.update, Update::Fetching(..) | Update::Ready(_)) {
            if asked {
                self.update_shown = true;
                cx.notify();
            }
            return;
        }
        if self.install == Install::Developer {
            return self.say(
                Update::Failed {
                    message: "A build made from the source is not updated.",
                    by_hand: false,
                },
                Some(LINGER),
                cx,
            );
        }
        if asked {
            self.say(Update::Looking, None, cx);
        }

        let install = self.install.clone();
        self._update_work = cx.spawn_in(window, async move |this, cx| {
            let looked = cx
                .background_executor()
                .spawn({
                    let install = install.clone();
                    async move { update::look(&install) }
                })
                .await;
            let release = match looked {
                Ok(Some(release)) => release,
                Ok(None) => {
                    crate::startup::trace(|| "no newer version".into());
                    if asked {
                        this.update(cx, |this, cx| this.say(Update::Newest, Some(LINGER), cx))
                            .ok();
                    }
                    return;
                }
                Err(problem) => {
                    crate::startup::trace(|| {
                        format!("could not look for a new version: {problem}")
                    });
                    if asked {
                        this.update(cx, |this, cx| {
                            let failed = Update::Failed {
                                message: "Could not look for a new version.",
                                by_hand: false,
                            };
                            this.say(failed, Some(LINGER), cx)
                        })
                        .ok();
                    }
                    return;
                }
            };
            crate::startup::trace(|| format!("version {} is out", release.version));

            // A copy we cannot replace is told where the new one is.
            if !release.fetchable() {
                this.update(cx, |this, cx| this.say(Update::Out(release), None, cx))
                    .ok();
                return;
            }

            let fetched = Fetched::default();
            this.update(cx, |this, cx| {
                this.update = Update::Fetching(release.clone(), fetched.clone());
                this.update_shown = asked;
                this._update_hide = Task::ready(());
                cx.notify();
            })
            .ok();
            let fetching = cx.background_executor().spawn({
                let release = release.clone();
                async move { update::fetch(&release, &install, &fetched) }
            });
            // While the card shows how far the download has got, keep it moving.
            let ticking = asked.then(|| {
                let this = this.clone();
                cx.spawn(async move |cx| {
                    loop {
                        cx.background_executor()
                            .timer(Duration::from_millis(120))
                            .await;
                        let going = this.update(cx, |this, cx| {
                            cx.notify();
                            matches!(this.update, Update::Fetching(..))
                        });
                        if !matches!(going, Ok(true)) {
                            break;
                        }
                    }
                })
            });
            let outcome = fetching.await;
            drop(ticking);

            this.update_in(cx, |this, window, cx| match outcome {
                Ok(ready) => {
                    crate::startup::trace(|| format!("version {} is ready", ready.version));
                    this.say(Update::Ready(ready), None, cx);
                    if update::unattended() {
                        this.restart_into_update(window, cx);
                    }
                }
                Err(problem) => {
                    crate::startup::trace(|| format!("could not fetch the new version: {problem}"));
                    // Unasked, it is simply tried again the next time round.
                    match asked {
                        true => this.say(
                            Update::Failed {
                                message: "The new version could not be fetched.",
                                by_hand: true,
                            },
                            None,
                            cx,
                        ),
                        false => this.update = Update::Quiet,
                    }
                }
            })
            .ok();
        });
    }

    /// Swap the new version in and start it. Nothing is asked about unsaved
    /// edits: they are written down first, and the new copy opens with them
    /// as they were.
    pub(super) fn restart_into_update(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Update::Ready(ready) = self.update.clone() else {
            return;
        };
        self.back_up_now();
        self.session(window).save();
        match update::swap_in(&ready, &self.install) {
            Ok(()) => {
                self.close_confirmed = true;
                cx.quit();
            }
            Err(problem) => {
                crate::startup::trace(|| {
                    format!("could not restart into the new version: {problem}")
                });
                self.say(
                    Update::Failed {
                        message: "The new version could not be put in place.",
                        by_hand: true,
                    },
                    None,
                    cx,
                );
            }
        }
    }

    /// Put the card up in a given state, for scripted runs.
    #[cfg(feature = "shot")]
    pub(super) fn debug_update(&mut self, state: &str, cx: &mut Context<Self>) {
        let release = update::Release::pretend("0.2.0", 25_000_000);
        let update = match state {
            "looking" => Update::Looking,
            "newest" => Update::Newest,
            "fetching" => {
                let fetched = Fetched::default();
                fetched.store(10_500_000, Ordering::Relaxed);
                Update::Fetching(release, fetched)
            }
            "ready" => Update::Ready(Ready::pretend("0.2.0")),
            "out" => Update::Out(release),
            "failed" => Update::Failed {
                message: "The new version could not be put in place.",
                by_hand: true,
            },
            "done" => Update::Done,
            _ => Update::Quiet,
        };
        self.say(update, None, cx);
    }

    /// Put something on the card. With `linger`, the card goes by itself
    /// after that long.
    fn say(&mut self, update: Update, linger: Option<Duration>, cx: &mut Context<Self>) {
        self.update = update;
        self.update_shown = true;
        self._update_hide = match linger {
            Some(linger) => cx.spawn(async move |this, cx| {
                cx.background_executor().timer(linger).await;
                this.update(cx, |this, cx| {
                    this.update_shown = false;
                    cx.notify();
                })
                .ok();
            }),
            None => Task::ready(()),
        };
        cx.notify();
    }

    fn press(&mut self, press: Press, window: &mut Window, cx: &mut Context<Self>) {
        match press {
            Press::Restart => return self.restart_into_update(window, cx),
            // The update stays ready; the card is back at the next start.
            Press::Later => {}
            Press::Open(page) => cx.open_url(page),
            Press::WhatsNew => {
                cx.open_url(&format!("{}/tag/v{VERSION}", update::RELEASES));
            }
        }
        self.update_shown = false;
        cx.notify();
    }

    /// The card at the foot of the window, while there is something to say.
    pub(super) fn render_update(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let presence = transition(
            "update-card",
            if self.update_shown { 1. } else { 0. },
            motion::glide(motion::PANEL, cx),
            window,
            cx,
        );
        if presence <= 0.001 {
            return None;
        }
        let p = *cx.palette();

        // What it says, how it is marked, and the buttons: a quiet one, then
        // the one that matters.
        let (words, mark, quiet, main): (SharedString, Hsla, Option<_>, Option<_>) =
            match &self.update {
                Update::Quiet => return None,
                Update::Looking => (
                    "Looking for a new version…".into(),
                    p.text_tertiary,
                    None,
                    None,
                ),
                Update::Newest => (
                    format!("ZinMD {VERSION} is the newest version").into(),
                    p.text_tertiary,
                    None,
                    None,
                ),
                Update::Fetching(release, fetched) => {
                    let (got, all) = (fetched.load(Ordering::Relaxed), release.size());
                    let words = match all {
                        0 => format!("Getting ZinMD {}…", release.version),
                        _ => format!(
                            "Getting ZinMD {}… {}%",
                            release.version,
                            (got * 100 / all).min(100)
                        ),
                    };
                    (words.into(), p.accent, None, None)
                }
                Update::Ready(ready) => (
                    format!("ZinMD {} is ready", ready.version).into(),
                    p.accent,
                    Some(("Later", Press::Later)),
                    Some(("Restart", Press::Restart)),
                ),
                Update::Out(release) => (
                    format!("ZinMD {} is out", release.version).into(),
                    p.accent,
                    Some(("Later", Press::Later)),
                    Some(("Get it", Press::Open(update::DOWNLOADS))),
                ),
                Update::Failed { message, by_hand } => (
                    (*message).into(),
                    p.status_danger,
                    Some(("Close", Press::Later)),
                    by_hand.then_some(("Get it", Press::Open(update::DOWNLOADS))),
                ),
                Update::Done => (
                    format!("Updated to ZinMD {VERSION}").into(),
                    p.accent,
                    Some(("What's new", Press::WhatsNew)),
                    None,
                ),
            };

        let (strong, hover) = (p.text_primary, p.surface_hover);
        let quiet = quiet.map(|(label, press)| {
            div()
                .id("update-quiet")
                .flex()
                .items_center()
                .h(px(28.))
                .px(px(9.))
                .rounded(size::RADIUS_SM)
                .text_color(p.text_secondary)
                .hover(move |style| style.bg(hover).text_color(strong))
                .child(label)
                .on_click(cx.listener(move |this, _, window, cx| this.press(press, window, cx)))
        });
        let bright = p.accent_hover;
        let main = main.map(|(label, press)| {
            div()
                .id("update-main")
                .flex()
                .items_center()
                .h(px(28.))
                .px(px(12.))
                .rounded(size::RADIUS_SM)
                .bg(p.accent)
                .text_color(p.text_on_accent)
                .font_weight(FontWeight::SEMIBOLD)
                .hover(move |style| style.bg(bright))
                .active(|style| style.opacity(0.8))
                .child(label)
                .on_click(cx.listener(move |this, _, window, cx| this.press(press, window, cx)))
        });
        // With buttons, they close the card; without, the words have the
        // same room either side.
        let ends = match quiet.is_some() || main.is_some() {
            true => px(6.),
            false => px(16.),
        };

        let card = h_flex()
            .id("update-card")
            .flex_none()
            .h(px(40.))
            .pl(px(14.))
            .pr(ends)
            .gap(px(4.))
            .rounded(size::RADIUS_LG)
            .border_1()
            .border_color(p.border_strong)
            .bg(p.surface_raised)
            .shadow_lg()
            // What is under the card is not clicked through it.
            .occlude()
            .child(div().flex_none().size(px(7.)).rounded_full().bg(mark))
            .child(div().flex_none().pl(px(6.)).pr(px(8.)).child(words))
            .children(quiet)
            .children(main);

        // In the middle of the foot, clear of the rail at the page's edge and
        // of the bar under the editor. It rises a little as it comes.
        let foot = match self.mode {
            Mode::Read => px(24.),
            Mode::Edit => size::STATUSBAR + px(14.),
        };
        Some(
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom(foot - px(10.) * (1. - presence))
                .flex()
                .justify_center()
                .opacity(presence)
                .child(card),
        )
    }
}
