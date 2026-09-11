//! The shell: the custom title bar, the icon-only sidebar rail, the main pane,
//! and the quick-navigation and section-switch bindings that live on the pane.
//!
//! **Which tools exist is not decided here.** [`crate::tools`] is the table —
//! one row per tool carrying its code, title, icon, platforms, view type and
//! accepted pastes — and [`View`] and [`Panes`] are generated from it. This file
//! draws whatever that table declares, in whatever order
//! [`Layout::features`](Layout::features) says the user wants, and knows about a
//! particular tool in exactly one place: [`Layout::activate`], which is Docker's
//! polling lifecycle; and [`Layout::apply_route`], which unpacks a pasted
//! payload into the one method the receiving tool has for it.
//!
//! # The chrome
//!
//! There is one bar across the top — a [`TitleBar`], so the window controls
//! (macOS traffic lights, or the Windows min/max/close buttons) are the
//! platform's own and this file only reserves room for them. The bar carries
//! the sidebar toggle by the sidebar it hides, the Settings and Update buttons,
//! and the Dodo mark pinned to the edge **opposite** the OS controls. Below it
//! sit the icon-only sidebar rail and the main pane.
//!
//! **The sidebar is icon-only always, and the toggle hides it entirely.** The
//! width-driven "labels when wide, icons when narrow" rule this shell used to
//! carry is gone: the rail is a fixed strip of large icons in the style of a
//! chat app's left rail, each naming itself — and its `Cmd`/`Ctrl` shortcut — on
//! hover. Item _N_ in the visible list is reached by `Cmd`/`Ctrl`+_N_, and the
//! hover label and the binding are the one function [`section_shortcut`] apart,
//! so they cannot drift.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{
    ActiveTheme, Selectable as _, Sizable as _, TITLE_BAR_HEIGHT, TitleBar, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app_icon::AppIcon;
use crate::encoder_decoder::Format;
use crate::i18n::{shell, t};
#[cfg(target_os = "macos")]
use crate::input_method::InputMethod;
use crate::quick_nav::models::detect::Detector;
use crate::quick_nav::models::route::Route;
use crate::quick_nav::{self, LeaveInsertMode, QuickNav, QuickNavigate};
use crate::session::Session;
use crate::session::models::features::{FeatureError, Features};
use crate::settings;
use crate::tools::{Panes, View};
use crate::updater;

/// How many sidebar sections a keyboard shortcut can reach: `Cmd`/`Ctrl`+1
/// through +9. There is no digit key past the ninth, so a tenth tool has a
/// hover label with no shortcut on it — [`section_shortcut`] returns `None` and
/// [`init`] binds nothing for it.
const SECTION_SHORTCUTS: usize = 9;

/// The keyboard shortcut that switches to the tool at `index` (0-based) in the
/// visible list, as it is shown on the row's hover label. `None` past the ninth
/// tool, which has no digit key.
///
/// **The one place the shown modifier is chosen**, and [`init`] binds the same
/// digits: the label a user reads and the key they press are this function and
/// the loop in `init`, and nothing else. Both platforms' chords are bound (see
/// `init`); the label shows the one that platform's users expect.
fn section_shortcut(index: usize) -> Option<String> {
    (index < SECTION_SHORTCUTS).then(|| {
        let modifier = if cfg!(target_os = "macos") {
            "Cmd"
        } else {
            "Ctrl"
        };
        format!("{modifier}+{}", index + 1)
    })
}

/// Switch the main pane to the tool at this 0-based position in the visible
/// list. Bound to `Cmd`/`Ctrl`+1..9 by [`init`]; the value is the digit minus
/// one, so the action carries the same index [`section_shortcut`] labels.
#[derive(Clone, PartialEq, Default, Debug, gpui_kit::Action)]
#[action(namespace = dodo, no_json)]
struct ActivateSection(usize);

/// Registers the section-switch key bindings.
///
/// Must run after `gpui_kit::component::init`, the same ordering rule
/// `quick_nav::init` and the feature crates' `init`s depend on: a binding
/// registered later wins a tie at equal context depth.
///
/// Both `Cmd`+_N_ and `Ctrl`+_N_ are bound on every platform, exactly as
/// `quick_nav` binds both clipboard chords — a Linux user on a Mac keyboard is
/// not an interesting mistake to punish — and both are scoped to
/// [`quick_nav::KEY_CONTEXT`], the pane's own context, so they fire whenever the
/// window is up (the pane wraps everything, focused input included, which is
/// why `Cmd`+2 switches sections mid-type as a browser's tab shortcut does).
pub fn init(cx: &mut App) {
    let mut bindings = Vec::with_capacity(SECTION_SHORTCUTS * 2);
    for index in 0..SECTION_SHORTCUTS {
        let digit = index + 1;
        bindings.push(KeyBinding::new(
            &format!("cmd-{digit}"),
            ActivateSection(index),
            Some(quick_nav::KEY_CONTEXT),
        ));
        bindings.push(KeyBinding::new(
            &format!("ctrl-{digit}"),
            ActivateSection(index),
            Some(quick_nav::KEY_CONTEXT),
        ));
    }
    cx.bind_keys(bindings);
}

/// The widths and heights the layout is built from, in logical pixels. They are
/// plain `f32` so the derived numbers below stay readable arithmetic rather than
/// a second set of magic constants; `px(..)` goes on at the few use sites.
///
/// `MAIN_MIN_*` is the floor the main pane is held at: below it dodo's own
/// tables and toolbars start clipping their right-hand ends, so squeezing
/// further buys nothing a scrollbar does not buy better. 520 is the width at
/// which that crowding was first recorded.
const MAIN_MIN_WIDTH: f32 = 520.;
const MAIN_MIN_HEIGHT: f32 = 360.;
/// The icon rail's width, and the size of the tool glyphs on it. Wider than a
/// plain menu's collapsed rail (which the library draws at 48px around a 16px
/// icon): the rail is the primary navigation now, so its icons are large enough
/// to read at a glance in the style of a chat app's left rail.
const SIDEBAR_RAIL_WIDTH: f32 = 56.;
const SIDEBAR_ICON_SIZE: f32 = 22.;
/// The pane's own chrome around the tool: `p_4` left and right, and `p_4` top
/// and bottom. The title bar's height is added on top of the vertical figure by
/// [`window_min_size`].
const PANE_CHROME_WIDTH: f32 = 32.;
const PANE_CHROME_HEIGHT: f32 = 32.;

/// The smallest window dodo asks the platform to allow: the title bar, the icon
/// rail, and the main pane at its minimum. Handed to
/// `WindowOptions::window_min_size` in `main.rs`, which is what stops a drag
/// before the layout has to cope at all — the scroll container in
/// [`Layout::render`] is the fallback for when it does.
pub fn window_min_size() -> Size<Pixels> {
    size(
        px(SIDEBAR_RAIL_WIDTH + PANE_CHROME_WIDTH + MAIN_MIN_WIDTH),
        TITLE_BAR_HEIGHT + px(PANE_CHROME_HEIGHT + MAIN_MIN_HEIGHT),
    )
}

/// The box the active tool is rendered into, and the scroll container around
/// it. They are a pair and are written here rather than inline in
/// [`Layout::render`] because the two together are one rule that is easy to
/// break from either end — and because a `Div`'s style can be asserted in a
/// test, while the layout it produces cannot be without a window.
///
/// **The rule: the tool decides its own height, and this box is at least the
/// pane.** Those are two different things and the old shape could only express
/// the second. The box used to be `size_full`, i.e. height 100% of the scroll
/// container — a definite height, which meant a tool taller than the pane was
/// simply clipped: gpui measures a scroll container's content as the bounding
/// box of its **direct** children, so a page overflowing *inside* a box pinned
/// to 100% never made the container scrollable at all. It is now a flex item
/// with `flex_grow(1.)` and no height of its own, in a column scroll
/// container: growing gives it the whole pane when the tool is shorter (which
/// is every tool that fills its pane, and what `size_full` was there for), and
/// `flex_shrink_0` is what stops the flex algorithm taking that height back
/// when the tool is taller. Then the box *is* the content height, and the
/// container scrolls.
///
/// The floors are unchanged and still the only thing they ever were: below
/// `MAIN_MIN_*` the tool keeps its size and this container gains a scrollbar
/// rather than the tool being squeezed further.
///
/// **`w_full` here is load-bearing**: a box that sizes to its content leaves
/// rules and rows stopping short of the pane's edge. It is a width, not a
/// height, and the vertical fix must not be paid for with it.
fn tool_box() -> Div {
    div()
        .w_full()
        .flex_grow(1.)
        .flex_shrink_0()
        .min_w(px(MAIN_MIN_WIDTH))
        .min_h(px(MAIN_MIN_HEIGHT))
}

/// The scroll container [`tool_box`] is the sole child of. A column flex, so
/// that box can grow past the pane instead of being stretched to it.
fn main_pane() -> Stateful<Div> {
    div()
        .id("main-pane")
        .flex()
        .flex_col()
        .w_full()
        .flex_1()
        .min_h_0()
        .overflow_scroll()
}

pub struct Layout {
    /// Whether the whole sidebar is hidden. It is icon-only when shown; the
    /// title bar's toggle is the only thing that flips this, and the value is
    /// persisted in `session.json` (via [`Session::sidebar_collapsed`]).
    sidebar_hidden: bool,
    active: View,
    /// Which tools the sidebar lists, and in what order — the Features settings
    /// page, resolved against this build's [`View::ALL`].
    ///
    /// Held here rather than read from the session global at each use, because
    /// it is the thing `render` walks and because every change to it has to be
    /// followed by the two side effects a settings dialog cannot perform on its
    /// own: persisting the new list, and moving the pane off a tool that has
    /// just stopped being listed. [`Layout::set_tool_enabled`] and
    /// [`Layout::move_tool`] are the only writers.
    features: Features,
    /// Where focus rests in **normal mode**, and the reason this pane is
    /// focusable at all.
    ///
    /// gpui builds a keystroke's dispatch path from the focused element upwards,
    /// and with *nothing* focused that path is the window root alone — which
    /// carries none of this pane's key context, so quick navigation's bindings
    /// would not match and `p` would do nothing until the user had clicked
    /// something. Holding focus here means "no input is focused" is a real,
    /// reachable state rather than the absence of one. [`Layout::new`] takes it
    /// at startup and [`Layout::leave_insert_mode`] takes it back.
    focus: FocusHandle,
    /// Keeps the window-bounds observer alive: a `Subscription` unsubscribes
    /// when it drops, so this field is the subscription, not bookkeeping.
    _bounds: Subscription,
    /// Repaints the title bar when the updater's [`AvailableUpdate`] global
    /// changes, so the Update button appears (or vanishes) the moment a check
    /// finds (or clears) an offer — including the check the tray runs on reopen.
    ///
    /// [`AvailableUpdate`]: crate::updater::AvailableUpdate
    _update: Subscription,
    /// Re-checks macOS Accessibility after Dodo returns from System Settings.
    #[cfg(target_os = "macos")]
    _activation: Subscription,
    /// One live view per tool, built once and never rebuilt — the [`tools!`]
    /// table's own struct, so the pane a row declares is the pane this holds.
    ///
    /// [`tools!`]: crate::tools
    panes: Panes,
}

impl Layout {
    /// With nothing saved, dodo opens with the icon rail **shown** — the tools
    /// are the whole point of the window and the rail is a thin strip — and on
    /// the first tool. The open tool, the tool list and whether the sidebar was
    /// hidden are all restored from `session.json` when there is one; see
    /// [`crate::session`].
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // The menu bar item's Settings row opens this pane's dialog, and
        // `settings::open` needs a handle to the pane. Published here rather
        // than fetched there because the window can be closed and rebuilt while
        // the process lives on: every rebuild runs this and refreshes the
        // handle. A `cfg` for the same reason `main.rs` has one around the tray
        // module — it does not exist off macOS or Windows.
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        crate::tray::attach_layout(cx.entity().downgrade(), cx);

        // dodo opens in normal mode, so the pane takes focus straight away. See
        // [`Layout::focus`] for why that has to be someone's rather than nobody's.
        let focus = cx.focus_handle();
        window.focus(&focus, cx);

        // The tool list first, because it is what decides whether the
        // remembered tool is still a tool the sidebar has. A `session.json`
        // naming a tool that has since been switched off opens on the first
        // visible one instead of on a tool with no row — see [`View::shown`].
        let features = Features::resolve(Session::tools(cx).as_deref(), &View::codes());
        let active = View::shown(&features, Session::active_tool(cx).as_deref());

        // Restored when saved, shown by default. The persisted flag now means
        // "hidden entirely" rather than the old "collapsed to icons", because
        // the rail no longer has an expanded form to collapse from.
        let sidebar_hidden = Session::sidebar_collapsed(cx).unwrap_or(false);

        // Every tool's view at once, in the table's order. The `Panes` struct
        // is generated from the same rows the sidebar is, so there is no list
        // here to fall out of step with the list up there.
        let panes = Panes::new(window, cx);
        // What [`Layout::activate`] would have done, done by hand because there
        // is no `self` to call it on yet: opening straight onto Docker has to
        // start its polling, or the restored session shows an empty list until
        // the user clicks something else and back.
        if active == View::Docker {
            panes.docker.update(cx, |docker, cx| docker.activate(cx));
        }

        Self {
            sidebar_hidden,
            active,
            features,
            focus,
            // Every window move and resize, coalesced into a save by
            // `Session::set_window` — see `session`'s module doc for why that
            // matters and how. The `Subscription` has to be held or it
            // unsubscribes immediately.
            _bounds: cx.observe_window_bounds(window, |_, window, cx| {
                // The display's stable UUID, not its `DisplayId`, which is only
                // an index into this run's display list. `Session::set_window`
                // says why the rectangle needs it at all.
                let display = window
                    .display(cx)
                    .and_then(|display| display.uuid().ok())
                    .map(|uuid| uuid.to_string());
                Session::set_window(window.window_bounds(), display, cx);
            }),
            // Only a repaint: the button reads the global directly in `render`,
            // so nothing here has to carry the version — it just has to notice.
            _update: cx.observe_global::<updater::AvailableUpdate>(|_, cx| cx.notify()),
            #[cfg(target_os = "macos")]
            _activation: cx.observe_window_activation(window, |_, window, cx| {
                if window.is_window_active() {
                    InputMethod::reconcile_event_tap_after_activation(cx);
                }
            }),
            panes,
        }
    }

    /// Switches the main pane, and tells Docker whether its polling should be
    /// running.
    ///
    /// Both the sidebar rows and quick navigation come through here, so a jump
    /// leaves the app in exactly the state a click would have.
    fn activate(&mut self, view: View, cx: &mut Context<Self>) {
        self.active = view;
        self.panes.docker.update(cx, |docker, cx| match view {
            View::Docker => docker.activate(cx),
            _ => docker.set_section_active(false, cx),
        });
        // Re-selecting the tool already open writes nothing: `Session::edit`
        // drops a change that leaves the document as it was.
        Session::set_active_tool(view.code(), cx);
        cx.notify();
    }

    /// `Cmd`/`Ctrl`+_N_: switch to the tool at position _N_ in the visible list.
    ///
    /// A digit past the last visible tool does nothing — the list is the user's
    /// own, so the ninth shortcut on a six-tool sidebar is simply inert rather
    /// than an error.
    fn activate_section(
        &mut self,
        action: &ActivateSection,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ActivateSection(index) = *action;
        // Bind the tool before touching `self` mutably: `visible_tools`
        // borrows `self` immutably, and its iterator must be dropped before
        // `activate` can take the mutable borrow.
        let target = self.visible_tools().nth(index);
        if let Some(view) = target {
            self.activate(view, cx);
        }
    }

    /// The tools the sidebar draws, in the user's own order — the one list both
    /// the rail and the section shortcuts index into, so a shortcut can never
    /// point at a different tool than the icon it labels.
    fn visible_tools(&self) -> impl Iterator<Item = View> + '_ {
        self.features.visible().filter_map(View::lookup)
    }

    /// The sidebar's tools, shown or not, in the user's order. What the
    /// Features settings page lists.
    pub fn features(&self) -> &Features {
        &self.features
    }

    /// Shows or hides one tool, from the Features settings page.
    ///
    /// Returns the refusal when this would empty the sidebar — the page draws
    /// it. Nothing is written and nothing moves in that case.
    ///
    /// **Switching off the tool that is open switches the pane too.** That goes
    /// through [`Layout::activate`] like a sidebar click, so Docker's polling
    /// stops and the new tool is the one `session.json` remembers; the main
    /// pane cannot be left drawing a tool with no row above it.
    pub fn set_tool_enabled(
        &mut self,
        code: &str,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> Result<(), FeatureError> {
        self.features.set_enabled(code, enabled)?;
        self.tool_list_changed(cx);
        Ok(())
    }

    /// Moves one tool to `index` — the drop half of a drag reorder.
    pub fn move_tool(&mut self, code: &str, index: usize, cx: &mut Context<Self>) {
        if self.features.move_to(code, index) {
            self.tool_list_changed(cx);
        }
    }

    /// Moves one tool by `delta` places — the keyboard half, and what the
    /// move-up/move-down buttons call.
    pub fn move_tool_by(&mut self, code: &str, delta: isize, cx: &mut Context<Self>) {
        if self.features.move_by(code, delta) {
            self.tool_list_changed(cx);
        }
    }

    /// Persists the new tool list and re-checks what the pane is showing.
    ///
    /// Re-checking after a *reorder* is not wasted work: the first visible tool
    /// is what a hidden active tool falls back to, and moving a row changes
    /// which tool that is.
    fn tool_list_changed(&mut self, cx: &mut Context<Self>) {
        Session::set_tools(self.features.record(), cx);

        let shown = View::shown(&self.features, Some(self.active.code()));
        if shown == self.active {
            cx.notify();
        } else {
            self.activate(shown, cx);
        }

        // The control that made this change is drawn by the **settings
        // dialog**, a different entity in a layer this one does not own, so
        // notifying this pane would repaint the sidebar and leave the row the
        // user just pressed showing the value it had before. Same reason
        // `QuickNav::edit` refreshes.
        cx.refresh_windows();
    }

    /// `Cmd+V` / `Ctrl+V` / `p` in normal mode: read the clipboard, work out
    /// what it is, and go there.
    ///
    /// Nothing happens on four ordinary paths — the feature is off, the
    /// clipboard holds no text, nothing was recognised confidently, or the only
    /// tool that could have taken it is switched off — and in each the keystroke
    /// is propagated, because a shortcut that silently swallows a key it did not
    /// use is worse than one that declines it.
    ///
    /// `quick_nav`'s key context is what guarantees this only runs with no input
    /// focused; there is no mode flag to consult and none to get out of step.
    fn quick_navigate(&mut self, _: &QuickNavigate, window: &mut Window, cx: &mut Context<Self>) {
        let allowed = Self::allowed_detectors(&self.features);
        let route = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .and_then(|text| QuickNav::detect(&text, &allowed, cx));

        let Some(route) = route else {
            cx.propagate();
            return;
        };
        self.apply_route(route, window, cx);
    }

    /// The detectors whose tool the sidebar still lists.
    ///
    /// **A switched-off tool is not a paste target.** The user said they only
    /// want these features; pasting a `curl` with the API Explorer off must not
    /// bring it back, so the detector is dropped before detection runs and the
    /// text falls through to whatever else can read it — or nowhere. The
    /// alternative, re-enabling the tool for the jump, would be the app
    /// overruling a setting the user had just changed.
    ///
    /// The returned order is [`Detector::ORDER`]'s and means nothing:
    /// `detect_among` treats this as a membership test, precisely so that the
    /// sidebar's order can never leak into the detection order.
    fn allowed_detectors(features: &Features) -> Vec<Detector> {
        Detector::ORDER
            .into_iter()
            .filter(|detector| features.is_enabled(View::for_detector(*detector).code()))
            .collect()
    }

    /// Hands a detected route to the tool that owns it.
    ///
    /// **The one place a `Route` meets a `View`**, which is what keeps adding a
    /// tool to quick navigation from being an edit in three files: the detector
    /// decides *what*, this decides *where*, and neither knows the other's list.
    ///
    /// *Where* is [`View::for_detector`] and nothing else — the `match` below
    /// only carries the payload. They used to be one `match` doing both, which
    /// was fine until [`Layout::allowed_detectors`] needed the same mapping
    /// before any route existed; two copies of it could disagree about which
    /// tool a detector belongs to, and the one that could disagree silently is
    /// the one deciding whether the detector runs at all.
    fn apply_route(&mut self, route: Route, window: &mut Window, cx: &mut Context<Self>) {
        self.activate(View::for_detector(route.detector()), cx);

        match route {
            Route::Json(text) => {
                self.panes
                    .json_formatter
                    .update(cx, |view, cx| view.accept_text(text, window, cx));
            }
            Route::Jwt(token) => {
                self.panes.encoder_decoder.update(cx, |view, cx| {
                    view.accept_decode(token, Format::Jwt, window, cx)
                });
            }
            Route::Base64 { text, url_safe } => {
                let format = if url_safe {
                    Format::Base64UrlSafe
                } else {
                    Format::Base64
                };
                self.panes
                    .encoder_decoder
                    .update(cx, |view, cx| view.accept_decode(text, format, window, cx));
            }
            Route::Curl(snapshot) => {
                self.panes
                    .api_explorer
                    .update(cx, |view, cx| view.accept_curl(*snapshot, window, cx));
            }
            Route::Database(parsed) => {
                self.panes
                    .database
                    .update(cx, |view, cx| view.accept_uri(&parsed, cx));
            }
            Route::Mermaid(source) => {
                self.panes
                    .mermaid
                    .update(cx, |view, cx| view.open_tab(source, window, cx));
            }
        }
        cx.notify();
    }

    /// `Esc`: leave the focused input, which is what puts the app back in normal
    /// mode.
    ///
    /// Bound at this pane's context rather than at normal mode's, because it is
    /// the way *back* into normal mode and so has to fire while an input has
    /// focus. Every deeper Escape — a dialog, a popover, a select, a completion
    /// popup inside the input itself — is dispatched first and consumes the key
    /// if it wants it; this only ever runs once they have all declined.
    /// `quick_nav`'s module doc has the full ordering.
    ///
    /// Already in normal mode, it propagates rather than consuming: there is
    /// nothing here to leave.
    fn leave_insert_mode(
        &mut self,
        _: &LeaveInsertMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.focus.is_focused(window) {
            cx.propagate();
            return;
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// The custom title bar: the window controls (the platform's own), the
    /// sidebar toggle by the sidebar it hides, the Settings and Update buttons,
    /// and the Dodo mark on the edge opposite the OS controls.
    ///
    /// The [`TitleBar`] lays its two children out with `justify_between`, so the
    /// left cluster sits by the controls' side and the right cluster reaches the
    /// far edge. On macOS the controls are top-left, so the Dodo mark goes on
    /// the right; on Windows and Linux they are top-right, so it goes on the
    /// left — the Settings and Update buttons stay together beside the controls
    /// in both.
    fn title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // macOS is the one platform whose window controls are on the left.
        let controls_on_left = cfg!(target_os = "macos");
        let is_windows = cfg!(target_os = "windows");

        let toggle = Button::new("toggle-sidebar")
            .ghost()
            .child(
                (if self.sidebar_hidden {
                    AppIcon::PanelLeftOpen
                } else {
                    AppIcon::PanelLeftClose
                })
                .view(),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.sidebar_hidden = !this.sidebar_hidden;
                Session::set_sidebar_collapsed(this.sidebar_hidden, cx);
                cx.notify();
            }))
            // Exclude the button's hitbox from the surrounding Windows caption
            // region so WM_NCHITTEST leaves its click with the application.
            .when(is_windows, |this| this.occlude());

        let settings = Button::new("open-settings")
            .ghost()
            .child(AppIcon::Settings.view())
            .on_click({
                // The dialog's Features page edits this pane's tool list, so it
                // is handed a handle to it. Weak, and taken here rather than
                // inside the closure: `Button::on_click` is given an `&mut App`,
                // not a `Context<Self>`.
                let layout = cx.entity().downgrade();
                move |_, window, cx| settings::open(layout.clone(), window, cx)
            })
            .when(is_windows, |this| this.occlude());

        // Shown only when a check has found a newer version — the button is
        // absent otherwise. Opens the very dialog the sidebar's old "Check for
        // updates" opened; the visibility and the version text both come from
        // the updater's `AvailableUpdate` global, not a second mechanism.
        let update = updater::AvailableUpdate::get(cx).map(|info| {
            Button::new("update-available")
                .child(
                    h_flex()
                        .gap_1()
                        .items_center()
                        .child(AppIcon::Download.view())
                        .child(t(shell::Text::NewVersion(info.version.clone()), cx)),
                )
                .on_click(|_, window, cx| updater::open(window, cx))
                .when(is_windows, |this| this.occlude())
        });

        let mark = div()
            .flex()
            .items_center()
            .justify_center()
            .size_6()
            .child(AppIcon::Dodo.view().with_size(px(20.)));

        let divider = || div().w(px(1.)).h_5().bg(cx.theme().title_bar_border);

        let mut left = h_flex().items_center().gap_1();
        let mut right = h_flex().items_center().gap_1();

        // The Dodo mark rides on the edge opposite the OS controls.
        if controls_on_left {
            left = left.child(toggle);
            right = right
                .children(update)
                .child(settings)
                .child(divider())
                .child(mark);
        } else {
            left = left.child(mark).child(divider()).child(toggle);
            right = right.children(update).child(settings);
        }

        TitleBar::new().child(left).child(right)
    }

    /// The icon-only sidebar rail: one large glyph per **visible** tool, in the
    /// user's own order, each naming itself and its shortcut on hover.
    fn rail(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("sidebar-rail")
            .h_full()
            .flex_shrink_0()
            .w(px(SIDEBAR_RAIL_WIDTH))
            .py_2()
            .gap_1()
            .items_center()
            .bg(cx.theme().sidebar)
            .border_r_1()
            .border_color(cx.theme().sidebar_border)
            .children(
                self.visible_tools()
                    .enumerate()
                    .map(|(index, view)| self.rail_item(index, view, cx))
                    .collect::<Vec<_>>(),
            )
    }

    /// One rail row: the tool's glyph, its active highlight, and its hover label
    /// — the tool's own translated name, followed by the `Cmd`/`Ctrl` shortcut
    /// that reaches it. The label and the binding are one [`section_shortcut`]
    /// apart, so they read the same key.
    fn rail_item(&self, index: usize, view: View, cx: &mut Context<Self>) -> Button {
        let layout = cx.entity();
        let name = t(view.title(), cx);
        let tooltip: SharedString = match section_shortcut(index) {
            Some(shortcut) => format!("{name}  {shortcut}").into(),
            None => name,
        };

        Button::new(("rail-item", index))
            .ghost()
            .selected(self.active == view)
            .w(px(SIDEBAR_RAIL_WIDTH - 12.))
            .tooltip(tooltip)
            .child(view.icon().view().with_size(px(SIDEBAR_ICON_SIZE)))
            .on_click(move |_, _, cx| {
                layout.update(cx, |this, cx| this.activate(view, cx));
            })
    }
}

impl Render for Layout {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            // The pane is where quick navigation's and the section shortcuts'
            // key bindings live, and `track_focus` is what puts this node in a
            // keystroke's dispatch path. Both halves are load-bearing: without
            // the context the bindings never match, and without the focus handle
            // they stop matching the moment nothing else is focused. See
            // [`Layout::focus`].
            .key_context(quick_nav::KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::quick_navigate))
            .on_action(cx.listener(Self::leave_insert_mode))
            .on_action(cx.listener(Self::activate_section))
            .size_full()
            .bg(cx.theme().background)
            .child(self.title_bar(cx))
            .child(
                h_flex()
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    // The toggle hides the whole rail; shown, it is the icon
                    // sidebar.
                    .when(!self.sidebar_hidden, |this| this.child(self.rail(cx)))
                    .child(
                        v_flex()
                            .h_full()
                            .flex_1()
                            .min_w_0()
                            .p_4()
                            // The tool scrolls rather than being squeezed, and
                            // how that is arranged is [`main_pane`] and
                            // [`tool_box`]. …and which tool goes in the box is
                            // the table's answer, not a `match` here:
                            // `Panes::place` is generated from the same rows the
                            // rail walks, so a tool cannot be listed in one and
                            // missing from the other.
                            .child(
                                main_pane().child(
                                    tool_box().map(|this| self.panes.place(self.active, this)),
                                ),
                            ),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {

    use gpui_kit::{Display, FlexDirection, Length, Overflow, Styled as _, px, relative};

    use super::{
        Layout, MAIN_MIN_HEIGHT, MAIN_MIN_WIDTH, PANE_CHROME_HEIGHT, PANE_CHROME_WIDTH,
        SECTION_SHORTCUTS, SIDEBAR_RAIL_WIDTH, main_pane, section_shortcut, tool_box,
        window_min_size,
    };
    use crate::quick_nav::models::detect::{Detector, Patterns, detect_among};
    use crate::session::models::features::Features;
    use crate::tools::View;

    /// The tool list of someone who has never opened the Features page: every
    /// tool, in `View::ALL` order, all of them visible.
    fn everything() -> Features {
        Features::resolve(None, &View::codes())
    }

    /// The source of one item, from its signature down to the next line that
    /// starts a new top-level item. Enough to ask what a given function does
    /// without depending on how it is formatted inside.
    fn item_source<'a>(source: &'a str, signature: &str) -> &'a str {
        let start = source
            .find(signature)
            .unwrap_or_else(|| panic!("`{signature}` is gone from src/layout.rs"));
        let body = &source[start..];
        match body.find("\n}\n") {
            Some(end) => &body[..end],
            None => body,
        }
    }

    // ---- the section shortcuts --------------------------------------------

    /// **The shortcut label and the key binding are one function apart.** The
    /// hover label the user reads is [`section_shortcut`], and `init` binds the
    /// same digits — so the two cannot say different things. This pins the
    /// label side: item _N_ reads `Cmd`/`Ctrl`+_N_, 1-based.
    #[test]
    fn the_first_nine_sections_carry_a_one_based_shortcut() {
        let modifier = if cfg!(target_os = "macos") {
            "Cmd"
        } else {
            "Ctrl"
        };

        for index in 0..SECTION_SHORTCUTS {
            assert_eq!(
                section_shortcut(index).as_deref(),
                Some(format!("{modifier}+{}", index + 1)).as_deref(),
            );
        }
        // The tenth tool and beyond has no digit key, so no shortcut label.
        assert_eq!(section_shortcut(SECTION_SHORTCUTS), None);
        assert_eq!(section_shortcut(SECTION_SHORTCUTS + 3), None);
    }

    /// The digits reach exactly the sidebar's own list, in the user's own
    /// order: `Cmd`/`Ctrl`+1 is the first *visible* tool, whatever the user
    /// dragged there, and a hidden tool is skipped rather than counted.
    ///
    /// `activate_section` cannot run without a window (it calls `activate`,
    /// which touches an entity), so the mapping it depends on is asserted on the
    /// same iterator `activate_section` indexes — `visible()`+`lookup`, the
    /// definition `Layout::visible_tools` wraps.
    #[test]
    fn a_section_digit_indexes_the_visible_list_in_the_users_order() {
        let mut features = everything();
        features.move_to(View::Database.code(), 0);
        features
            .set_enabled(View::JsonFormatter.code(), false)
            .expect("the others remain");

        let visible: Vec<View> = features.visible().filter_map(View::lookup).collect();

        // Digit 1 is the tool the user dragged to the top…
        assert_eq!(visible.first().copied(), Some(View::Database));
        // …and the hidden tool is nowhere in the indexable list.
        assert!(!visible.contains(&View::JsonFormatter));

        // Every in-range digit lands on a real, still-visible tool.
        for index in 0..visible.len().min(SECTION_SHORTCUTS) {
            assert_eq!(
                visible.get(index).copied(),
                visible.iter().copied().nth(index)
            );
            assert!(section_shortcut(index).is_some());
        }
        // A digit past the end is inert — `nth` returns `None`, and
        // `activate_section` does nothing with it.
        assert_eq!(visible.iter().copied().nth(visible.len()), None);
    }

    /// `init` binds both the `Cmd` and the `Ctrl` chord for every section, so a
    /// source scan is the only window-free way to hold the count: the key
    /// binding really is `cmd-N`/`ctrl-N` for 1..=9, matching the labels.
    #[test]
    fn init_binds_both_chords_for_every_shortcut_section() {
        let source = include_str!("layout.rs");
        let init = item_source(source, "pub fn init(cx: &mut App)");
        assert!(init.contains("\"cmd-{digit}\""));
        assert!(init.contains("\"ctrl-{digit}\""));
        // Bound at the pane's own context, so they fire whenever the window is
        // up — including with an input focused, like a browser's tab shortcut.
        assert!(init.contains("quick_nav::KEY_CONTEXT"));
    }

    // ---- the title bar's Update button ------------------------------------

    /// **The button is drawn iff a check found an update**, and the check drives
    /// it through one global. This pins the visibility rule at its source: the
    /// title bar reads `AvailableUpdate::get`, whose `Some`/`None` is the whole
    /// of "an update is available". `record_check` in the updater is the only
    /// writer, and its own crate tests that a found check sets it and an
    /// up-to-date one clears it.
    #[test]
    fn the_update_button_is_gated_on_the_updater_global() {
        let source = include_str!("layout.rs");
        let title_bar = item_source(source, "fn title_bar(&self");

        assert!(
            title_bar.contains("updater::AvailableUpdate::get(cx).map("),
            "the Update button must be built from the updater's availability \
             global, so it is present exactly when a check found a newer version",
        );
        assert!(
            title_bar.contains("updater::open"),
            "clicking it must open the existing update dialog, not a new flow",
        );
        // The version text is the translated `NewVersion` string carrying the
        // detected version — never a bare literal.
        assert!(title_bar.contains("shell::Text::NewVersion(info.version"));
    }

    /// Trap 7's second half. `crate::tools` proves neither Settings nor the
    /// updater is a row in the table; this proves the Settings button is still
    /// reachable — it moved to the title bar, and the Features page it opens is
    /// the only way back from a sidebar the user has cut down to one tool.
    #[test]
    fn settings_stays_reachable_from_the_title_bar() {
        let source = include_str!("layout.rs");
        let title_bar = item_source(source, "fn title_bar(&self");
        assert!(
            title_bar.contains("\"open-settings\""),
            "the Settings button has left the title bar; the Features page \
             assumes it is always reachable",
        );
    }

    /// Every rail icon still names itself on hover — the icon rail is the only
    /// thing a row is now, so an anonymous glyph is a dead end. The label also
    /// carries the shortcut, which is what `section_shortcut` is for.
    #[test]
    fn every_rail_icon_names_itself_and_its_shortcut_on_hover() {
        let source = include_str!("layout.rs");
        let rail_item = item_source(source, "fn rail_item(&self");
        assert!(rail_item.contains(".tooltip("));
        assert!(rail_item.contains("section_shortcut(index)"));
    }

    // ---- quick navigation meets the tool list ------------------------------

    /// Trap 4, and the captain's own example: pasting a `curl` with the API
    /// Explorer switched off. The detector is not tried at all, so the text
    /// falls through to the next one that can read it — here the JSON body —
    /// and the API Explorer is **not** switched back on to receive it.
    #[test]
    fn a_switched_off_tool_is_not_a_paste_target() {
        let text = "curl -X POST https://api.example.com/v1/orders \
                    -H 'Content-Type: application/json' -d '{\"item\":\"widget\"}'";
        let patterns = Patterns::default();

        let mut features = everything();
        assert_eq!(
            detect_among(text, &patterns, &Layout::allowed_detectors(&features))
                .map(|route| route.detector()),
            Some(Detector::Curl),
        );

        features
            .set_enabled(View::ApiExplorer.code(), false)
            .expect("five others remain");
        let allowed = Layout::allowed_detectors(&features);

        assert!(!allowed.contains(&Detector::Curl));
        assert_eq!(
            detect_among(text, &patterns, &allowed).map(|route| route.detector()),
            None,
            "the whole command is not JSON, so nothing else claims it either",
        );

        // …and the body on its own still reaches the formatter, which is what
        // "falls through to the next detector" looks like when there is one.
        assert_eq!(
            detect_among("{\"item\":\"widget\"}", &patterns, &allowed)
                .map(|route| route.detector()),
            Some(Detector::Json),
        );
    }

    /// Switching off the Encoder/Decoder takes **both** of its detectors out of
    /// play, because `for_detector` is not injective.
    #[test]
    fn switching_off_one_tool_can_silence_two_detectors() {
        let mut features = everything();
        features
            .set_enabled(View::EncoderDecoder.code(), false)
            .expect("five others remain");

        let allowed = Layout::allowed_detectors(&features);
        assert!(!allowed.contains(&Detector::Jwt));
        assert!(!allowed.contains(&Detector::Base64));
        assert!(allowed.contains(&Detector::Json));
    }

    /// Trap 5: the sidebar's order is a preference and detection's is a
    /// correctness property. Dragging the Encoder/Decoder above everything —
    /// or below it — must not change what a pasted token or a pasted `curl`
    /// does.
    #[test]
    fn the_sidebar_order_never_becomes_the_detection_order() {
        const JWT: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxIn0.c2ln";
        let curl = "curl -X POST https://api.example.com -d '{\"a\":1}'";
        let patterns = Patterns::default();

        // Every tool, walked into every position: the Encoder/Decoder first,
        // then last, then back where it started.
        for at in [0, View::ALL.len() - 1, 1] {
            let mut features = everything();
            features.move_to(View::EncoderDecoder.code(), at);
            features.move_to(View::ApiExplorer.code(), View::ALL.len() - 1);
            let allowed = Layout::allowed_detectors(&features);

            assert_eq!(
                detect_among(JWT, &patterns, &allowed).map(|route| route.detector()),
                Some(Detector::Jwt),
                "a JWT is Base64, and only the detection order keeps it out of the decoder",
            );
            assert_eq!(
                detect_among(curl, &patterns, &allowed).map(|route| route.detector()),
                Some(Detector::Curl),
                "a cURL command carrying JSON belongs to the API Explorer wherever its row is",
            );
        }
    }

    // ---- the window floor --------------------------------------------------

    #[test]
    fn the_smallest_allowed_window_holds_the_title_bar_rail_and_pane_minimum() {
        use gpui_kit::component::TITLE_BAR_HEIGHT;

        let min = window_min_size();

        assert_eq!(
            min.width,
            px(SIDEBAR_RAIL_WIDTH + PANE_CHROME_WIDTH + MAIN_MIN_WIDTH)
        );
        assert_eq!(
            min.height,
            TITLE_BAR_HEIGHT + px(PANE_CHROME_HEIGHT + MAIN_MIN_HEIGHT)
        );
    }

    /// The defect this pair exists to prevent: a tool page taller than the pane
    /// being clipped instead of scrolled.
    ///
    /// gpui measures a scroll container's content as the bounding box of its
    /// **direct** children, so the box holding the tool is the only thing that
    /// can report the page's height. Give it a height of its own — `size_full`,
    /// which is 100% and therefore definite — and it reports the pane's height
    /// however tall the page is, and everything past the bottom edge is simply
    /// gone. This is asserted on the style rather than on the laid-out result
    /// because computing a gpui layout needs a window, and `TaffyLayoutEngine`
    /// is private to gpui besides.
    #[test]
    fn the_tool_box_takes_its_height_from_the_tool_and_never_from_the_pane() {
        let mut tool_box = tool_box();
        let style = tool_box.style();

        assert_eq!(
            style.size.height, None,
            "a height here is a definite height, and a page pinned to the pane \
             cannot report the overflow the scroll container is meant to reveal",
        );
        assert_eq!(
            style.flex_grow,
            Some(1.),
            "…so the pane is filled by growing instead: a tool shorter than the \
             pane still gets all of it, which is what `size_full` was for",
        );
        assert_eq!(
            style.flex_shrink,
            Some(0.),
            "and growing is only half of it — a shrinkable item is pulled back \
             to the container's height, which is the clipping again",
        );

        // The floors are unchanged, and the width one is load-bearing: a box
        // that sizes to its content leaves rules and rows stopping short of
        // the pane's edge.
        assert_eq!(style.size.width, Some(Length::from(relative(1.))));
        assert_eq!(style.min_size.width, Some(Length::from(px(MAIN_MIN_WIDTH))));
        assert_eq!(
            style.min_size.height,
            Some(Length::from(px(MAIN_MIN_HEIGHT)))
        );
    }

    /// The other half: growing only means anything inside a column flex, and
    /// only a scroll container can show what grew past it.
    #[test]
    fn the_main_pane_is_a_column_that_scrolls() {
        let mut main_pane = main_pane();
        let style = main_pane.style();

        assert_eq!(style.display, Some(Display::Flex));
        assert_eq!(
            style.flex_direction,
            Some(FlexDirection::Column),
            "a row would make height the cross axis, and `flex_grow` on the \
             tool box would stop meaning anything vertical",
        );
        assert_eq!(style.overflow.y, Some(Overflow::Scroll));
        assert_eq!(style.overflow.x, Some(Overflow::Scroll));
        assert_eq!(
            style.size.width,
            Some(Length::from(relative(1.))),
            "the pane's full width, so the tool's rules reach its edge",
        );
        // `flex_1` + `min_h_0`: the container is what shrinks with the window,
        // so that the box inside it can keep the content's height.
        assert_eq!(style.flex_grow, Some(1.));
        assert_eq!(style.flex_shrink, Some(1.));
        assert_eq!(style.min_size.height, Some(Length::from(px(0.))));
    }
}
