//! The shell: the sidebar, the main pane, the width rule between them, and the
//! quick-navigation bindings that live on the pane.
//!
//! **Which tools exist is not decided here.** [`crate::tools`] is the table —
//! one row per tool carrying its code, title, icon, platforms, view type and
//! accepted pastes — and [`View`] and [`Panes`] are generated from it. This file
//! draws whatever that table declares, in whatever order
//! [`Layout::features`](Layout::features) says the user wants, and knows about a
//! particular tool in exactly two places: [`pane_title`] and
//! [`Layout::activate`], which are Docker's rail heading and Docker's polling
//! lifecycle; and [`Layout::apply_route`], which unpacks a pasted payload into
//! the one method the receiving tool has for it.

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::sidebar::{
    Sidebar, SidebarCollapsible, SidebarGroup, SidebarHeader, SidebarItem, SidebarMenuItem,
};
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, Collapsible, StyledExt as _, h_flex, v_flex};

use crate::app_icon::AppIcon;
use crate::docker::DockerPage;
use crate::encoder_decoder::Format;
use crate::i18n::{Str, shell, t};
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

/// The heading above the main pane. Docker names the page its rail has selected
/// — the sidebar row says "Docker", the heading says "Containers" — so the
/// header keeps telling the user which of the four they are looking at, exactly
/// as it did when the four were sidebar children.
fn pane_title(view: View, docker_page: DockerPage) -> Str {
    match view {
        View::Docker => docker_page.title(),
        other => other.title(),
    }
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
/// The sidebar's two widths. The collapsed one is `COLLAPSED_WIDTH` in the
/// pinned checkout's `sidebar/mod.rs`, which the library does not export.
const SIDEBAR_WIDTH: f32 = 240.;
const SIDEBAR_RAIL_WIDTH: f32 = 48.;
/// The pane's own chrome around the tool: `p_4` left and right; and `p_4` top
/// and bottom plus the header row (`h_8`) and the `gap_4` under it.
const PANE_CHROME_WIDTH: f32 = 32.;
const PANE_CHROME_HEIGHT: f32 = 80.;

/// The window width at which the sidebar gives up its labels.
///
/// **Derived, not chosen**: it is exactly the width at which an expanded
/// sidebar would push the main pane below [`MAIN_MIN_WIDTH`]. Narrower than
/// this and the labels are costing the content more than they are worth.
const AUTO_COLLAPSE_WIDTH: f32 = SIDEBAR_WIDTH + PANE_CHROME_WIDTH + MAIN_MIN_WIDTH;

/// The smallest window dodo asks the platform to allow: the icon rail, plus the
/// main pane at its minimum. Handed to `WindowOptions::window_min_size` in
/// `main.rs`, which is what stops a drag before the layout has to cope at all —
/// the scroll container in [`Layout::render`] is the fallback for when it does.
pub fn window_min_size() -> Size<Pixels> {
    size(
        px(SIDEBAR_RAIL_WIDTH + PANE_CHROME_WIDTH + MAIN_MIN_WIDTH),
        px(PANE_CHROME_HEIGHT + MAIN_MIN_HEIGHT),
    )
}

/// Whether the sidebar is showing icons only, and how it came to be that way.
///
/// **The width rule is edge-triggered, and that is the whole reason this is a
/// struct rather than a `width < AUTO_COLLAPSE_WIDTH` test inside `render`.** A
/// level-triggered rule re-collapses the sidebar on the very next frame after
/// the user expands it, so at a narrow width the toggle would appear broken —
/// a control that undoes itself is worse than no control at all. Only the
/// window *crossing* the breakpoint moves the sidebar:
///
/// * Crossing downward collapses it, and records that the width did it.
/// * Crossing upward expands it again — but only if the collapse was the
///   width's own. A sidebar the user collapsed by hand stays collapsed.
/// * The toggle always wins and hands ownership back to the user: after a
///   manual press the sidebar stays exactly as left until the next crossing.
///
/// The `collapsed` flag **is** persisted, in `session.json`; the other two are
/// not, and must not be — see [`SidebarState::restored`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct SidebarState {
    collapsed: bool,
    /// Set only when [`SidebarState::resize`] was the one that collapsed it,
    /// which is what [`SidebarState::resize`] later needs in order to know
    /// whether re-expanding would be restoring the user's state or overriding
    /// it.
    collapsed_by_width: bool,
    /// Which side of the breakpoint the last width was on, or `None` before the
    /// first frame — so the first width ever seen is recorded rather than
    /// treated as a crossing, and the opening state is whatever [`Layout::new`]
    /// asked for.
    narrow: Option<bool>,
}

impl SidebarState {
    /// How dodo opens with nothing saved: on the icon rail, by choice rather
    /// than by width.
    const fn new() -> Self {
        Self {
            collapsed: true,
            collapsed_by_width: false,
            narrow: None,
        }
    }

    /// How dodo opens with a saved sidebar.
    ///
    /// **Only `collapsed` is restored, and the other two fields are reset**,
    /// which is the whole subtlety here. `collapsed_by_width` says "the width
    /// rule did this, so the width rule may undo it", and that is a claim about
    /// a window size from the *last* run — restoring it would let the first
    /// widening past the breakpoint expand a sidebar the user had collapsed by
    /// hand. `narrow` stays `None` so the first width this run sees is recorded
    /// rather than treated as a crossing, exactly as on a fresh start; the
    /// restored state is then whatever the user left, at any window size.
    const fn restored(collapsed: bool) -> Self {
        Self {
            collapsed,
            collapsed_by_width: false,
            narrow: None,
        }
    }

    /// Apply the width rule to a new window width.
    ///
    /// Pure, and idempotent for any width on the same side of the breakpoint —
    /// which is what makes it safe to call once per frame from `render`.
    fn resize(mut self, width: Pixels) -> Self {
        let narrow = width < px(AUTO_COLLAPSE_WIDTH);
        let crossed = self.narrow.is_some_and(|was| was != narrow);
        self.narrow = Some(narrow);

        if !crossed {
            return self;
        }

        if narrow {
            if !self.collapsed {
                self.collapsed = true;
                self.collapsed_by_width = true;
            }
        } else if self.collapsed_by_width {
            self.collapsed = false;
            self.collapsed_by_width = false;
        }

        self
    }

    /// The user pressed the toggle. Their choice, and theirs to keep.
    fn toggle(self) -> Self {
        Self {
            collapsed: !self.collapsed,
            collapsed_by_width: false,
            ..self
        }
    }
}

/// A tool row that names itself while the sidebar is collapsed to icons.
///
/// **Wrapping is the only way to get that tooltip, and there is no risk of a
/// second one.** `SidebarMenuItem` at the pinned revision has no tooltip of its
/// own — `sidebar/menu.rs` has neither the field nor the builder — and
/// `SidebarMenu::children` accepts nothing but a `SidebarMenuItem`, so it
/// cannot be added from inside the menu either. `SidebarItem` is public though,
/// and `SidebarGroup` takes any implementation of it, so the rows go into the
/// group directly, each inside a `div` carrying the tooltip. `SidebarGroup`
/// already stacks its children with the same `gap_2` `SidebarMenu` used, so
/// dropping `SidebarMenu` changes nothing that is drawn.
///
/// This is still one flat row per tool — [`SidebarMenuItem::children`] stays
/// unused, for the reason [`View`] gives.
#[derive(Clone)]
struct ToolItem {
    item: SidebarMenuItem,
    /// The row's own translated title, the very string its label shows. A
    /// tooltip that read differently from the label would be a second string
    /// to translate and a second thing to keep in step.
    title: SharedString,
    collapsed: bool,
}

impl Collapsible for ToolItem {
    fn is_collapsed(&self) -> bool {
        self.collapsed
    }

    fn collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = collapsed;
        self
    }
}

impl SidebarItem for ToolItem {
    fn render(
        self,
        id: impl Into<ElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> impl IntoElement {
        let id = id.into();
        let collapsed = self.collapsed;
        let title = self.title;

        div()
            // Its own id, not the row's: `tooltip` comes from
            // `StatefulInteractiveElement`, so the wrapper has to be stateful,
            // and reusing the row's id for the element above it reads like a
            // mistake even though the two paths differ.
            .id(SharedString::from(format!("tool-tip-{id}")))
            .w_full()
            .when(collapsed, |this| {
                this.tooltip(move |window, cx| Tooltip::new(title.clone()).build(window, cx))
            })
            .child(self.item.collapsed(collapsed).render(id, window, cx))
    }
}

/// One sidebar-footer row: the icon, and the label beside it when the sidebar
/// is wide enough to have one.
///
/// **Lining the collapsed icon up with the tool icons above is arithmetic, not
/// taste**, and it is worth writing down because two of the three numbers come
/// from inside the widget library (`sidebar/mod.rs`, `sidebar/menu.rs` and
/// `button/button.rs` in the pinned checkout):
///
/// * Collapsed the rail is 48px and `Sidebar` insets the menu (`#inner`'s
///   `p_2`) and the footer (`px_2`) by the same 8px, so each gets the same
///   31px-wide box. A menu row fills that box and centres its icon in it, so
///   the footer button has to fill it too — hence `w_full` and **`px_0`**,
///   because `Button`'s own `px_4` is wider than the whole box and pushes its
///   contents out past the right-hand edge.
/// * Expanded both are inset 12px (`px_3`) and a menu row puts its icon 8px
///   further in (`p_2`), so `px_2` on the button lands the footer icon in the
///   same column, and the label in the same column as the row labels.
///
/// Two things this deliberately does *not* use, having read what they do:
///
/// * **`SidebarFooter`** — it adds a second `p_2` the menu rows do not have,
///   which halves the collapsed box to 15px and leaves no way to reach the
///   rail's centre, plus a hover highlight spanning the whole footer that a
///   menu row has no counterpart for. `Sidebar::footer` takes any element, so
///   the `v_flex` goes in directly. It is still a stack rather than two loose
///   buttons: the sidebar's own footer wrapper is an `h_flex`, so siblings
///   would sit side by side.
/// * **`.justify_start()`** — which these buttons used to carry, and which
///   cannot align anything here: `Button` wraps its children in an
///   `h_flex().size_full().justify_center()`, so the outer justification never
///   reaches the icon. The child's own `w_full` is what left-aligns the
///   expanded row.
fn footer_button(
    id: &'static str,
    icon: AppIcon,
    label: Str,
    icon_collapsed: bool,
    cx: &App,
) -> Button {
    // Translated once: collapsed it is the tooltip, expanded it is the label.
    // The tooltip is never a second string written for the purpose.
    let label = t(label, cx);

    Button::new(id)
        .ghost()
        .w_full()
        .map(|this| {
            if icon_collapsed {
                this.px_0().tooltip(label.clone())
            } else {
                this.px_2()
            }
        })
        .child(
            h_flex()
                .gap_2()
                .when(!icon_collapsed, |this| this.w_full())
                .child(icon.view())
                .when(!icon_collapsed, |this| {
                    // Fixed-length label in a 240px-wide sidebar: without these
                    // it wraps to two lines and pushes the footer taller.
                    this.child(div().flex_shrink_0().whitespace_nowrap().child(label))
                }),
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
/// that box can grow past the pane instead of being stretched to it; the pane
/// header stays outside, so the pane title and the sidebar toggle never scroll
/// away.
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
    collapsible: SidebarCollapsible,
    sidebar: SidebarState,
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
    /// With nothing saved, dodo opens on the **icon rail**, not the labelled
    /// sidebar: the tools are a handful of entries a user learns once, and the
    /// pane they are choosing between is the whole point of the window, so
    /// 240px of permanent chrome is a poor default. The toggle in the pane
    /// header is unchanged, and every collapsed icon carries its title as a
    /// tooltip, which is what keeps the rail readable to someone who has not
    /// learned it yet.
    ///
    /// **That, the open tool and the tool list itself are restored from
    /// `session.json`** when there is one — the captain asked for session
    /// restoration on 2026-08-06, which is also what settles the sidebar
    /// question the sidebar round left open as being above that worker. See
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

        let sidebar = match Session::sidebar_collapsed(cx) {
            Some(collapsed) => SidebarState::restored(collapsed),
            None => SidebarState::new(),
        };

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
            collapsible: SidebarCollapsible::Icon,
            sidebar,
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

    /// The sidebar menu: one flat row per **visible** tool, in the user's own
    /// order, no nesting. Nesting is what the icon-collapsed sidebar cannot
    /// render, so there is none.
    ///
    /// A `Vec` rather than the fixed-size array this used to return, because
    /// the number of rows is now the user's choice. It is never empty:
    /// `Features` will not let the last tool be switched off.
    fn menu(&self, cx: &mut Context<Self>) -> Vec<ToolItem> {
        self.features
            .visible()
            .filter_map(View::lookup)
            .map(|view| self.tool_item(view, cx))
            .collect()
    }

    /// A flat, top-level tool row. Docker is one of these like any other: the
    /// click enters the section on whichever page its rail last had selected,
    /// which resumes that page's polling; every other tool pauses it.
    fn tool_item(&self, view: View, cx: &mut Context<Self>) -> ToolItem {
        let layout = cx.entity();
        let title = t(view.title(), cx);
        let item = SidebarMenuItem::new(title.clone())
            .icon(view.icon().view())
            .active(self.active == view)
            .on_click(move |_, _, cx| {
                layout.update(cx, |this, cx| this.activate(view, cx));
            });

        ToolItem {
            item,
            title,
            // `SidebarGroup` hands every child its own collapsed state on the
            // way to rendering it, so this is only a starting value.
            collapsed: false,
        }
    }
}

impl Render for Layout {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The width rule is applied here rather than from a resize observer,
        // and deliberately without a `cx.notify()`: GPUI re-renders on resize
        // anyway, this frame is built from the value below, and `resize` is
        // idempotent within one side of the breakpoint — so a frame at an
        // unchanged width changes nothing and no render can schedule another.
        self.sidebar = self.sidebar.resize(window.viewport_size().width);

        let icon_collapsed = self.sidebar.collapsed && self.collapsible == SidebarCollapsible::Icon;
        let title = pane_title(self.active, self.panes.docker.read(cx).page());

        h_flex()
            // The pane is where quick navigation's key bindings live, and
            // `track_focus` is what puts this node in a keystroke's dispatch
            // path. Both halves are load-bearing: without the context the
            // bindings never match, and without the focus handle they stop
            // matching the moment nothing else is focused. See
            // [`Layout::focus`].
            .key_context(quick_nav::KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::quick_navigate))
            .on_action(cx.listener(Self::leave_insert_mode))
            .size_full()
            .bg(cx.theme().background)
            .child(
                Sidebar::new("side-bar")
                    .collapsible(self.collapsible)
                    .collapsed(self.sidebar.collapsed)
                    .w(px(SIDEBAR_WIDTH))
                    .header(
                        SidebarHeader::new().child(
                            h_flex()
                                .gap_2()
                                .child(AppIcon::Dodo.view())
                                // Collapsed to icons the header keeps the mark
                                // and drops the word, the same treatment the
                                // Settings button below uses. "Dodo" is the
                                // product name and stays untranslated.
                                .when(!icon_collapsed, |this| this.child("Dodo")),
                        ),
                    )
                    .child(SidebarGroup::new(t(shell::Text::Tools, cx)).children(self.menu(cx)))
                    .footer(
                        // A plain stack, not a `SidebarFooter` — see
                        // [`footer_button`] for why, and for where its two
                        // paddings come from. `gap_2` is the menu's own row
                        // gap, so collapsed the icons keep the same rhythm all
                        // the way down the rail.
                        v_flex()
                            .w_full()
                            .gap_2()
                            .child(
                                // Beside Settings rather than inside it: this is
                                // an action, not a preference, and the one
                                // preference it carries ("check automatically")
                                // lives in the dialog it opens.
                                footer_button(
                                    "check-for-updates",
                                    AppIcon::Download,
                                    shell::Text::CheckForUpdates.into(),
                                    icon_collapsed,
                                    cx,
                                )
                                .on_click(|_, window, cx| updater::open(window, cx)),
                            )
                            .child(
                                footer_button(
                                    "open-settings",
                                    AppIcon::Settings,
                                    shell::Text::Settings.into(),
                                    icon_collapsed,
                                    cx,
                                )
                                // The dialog's Features page edits this pane's
                                // tool list, so it is handed a handle to it.
                                // Weak, and taken here rather than inside the
                                // closure: `Button::on_click` is given an
                                // `&mut App`, not a `Context<Self>`.
                                .on_click({
                                    let layout = cx.entity().downgrade();
                                    move |_, window, cx| settings::open(layout.clone(), window, cx)
                                }),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .h_full()
                    .flex_1()
                    .min_w_0()
                    .gap_4()
                    .p_4()
                    .child(
                        h_flex()
                            .items_center()
                            .gap_3()
                            .child(
                                Button::new("toggle-sidebar")
                                    .child(
                                        (if icon_collapsed {
                                            AppIcon::PanelLeftOpen
                                        } else {
                                            AppIcon::PanelLeftClose
                                        })
                                        .view(),
                                    )
                                    .ghost()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.sidebar = this.sidebar.toggle();
                                        // The user's own choice, and the only
                                        // one worth remembering: a collapse the
                                        // *width* caused is this window's
                                        // business, not the next launch's.
                                        Session::set_sidebar_collapsed(this.sidebar.collapsed, cx);
                                        cx.notify();
                                    })),
                            )
                            .child(div().font_bold().child(t(title, cx))),
                    )
                    // The tool scrolls rather than being squeezed, and how
                    // that is arranged is [`main_pane`] and [`tool_box`].
                    // …and which tool goes in the box is the table's answer,
                    // not a `match` here: `Panes::place` is generated from the
                    // same rows the sidebar walks, so a tool cannot be listed
                    // in one and missing from the other.
                    .child(
                        main_pane()
                            .child(tool_box().map(|this| self.panes.place(self.active, this))),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {

    use gpui::{Display, FlexDirection, Length, Overflow, Styled as _, px, relative};
    use gpui_component::Collapsible as _;
    use gpui_component::sidebar::SidebarMenuItem;

    use super::{
        AUTO_COLLAPSE_WIDTH, Layout, MAIN_MIN_HEIGHT, MAIN_MIN_WIDTH, PANE_CHROME_HEIGHT,
        PANE_CHROME_WIDTH, SIDEBAR_RAIL_WIDTH, SIDEBAR_WIDTH, SidebarState, ToolItem, main_pane,
        pane_title, tool_box, window_min_size,
    };
    use crate::docker::DockerPage;
    use crate::i18n::{Str, docker};
    use crate::quick_nav::models::detect::{Detector, Patterns, detect_among};
    use crate::session::models::features::Features;
    use crate::tools::View;

    /// A width comfortably on each side of the breakpoint. 1280 and 520 are the
    /// two the layout is reviewed at.
    const WIDE: f32 = 1280.;
    const NARROW: f32 = 520.;

    /// The tool list of someone who has never opened the Features page: every
    /// tool, in `View::ALL` order, all of them visible.
    fn everything() -> Features {
        Features::resolve(None, &View::codes())
    }

    fn title_of(view: View, page: DockerPage) -> Str {
        pane_title(view, page)
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

    /// The sidebar's restored flag is the user's own choice and nothing else.
    /// Restoring `collapsed_by_width` would let the first widening past the
    /// breakpoint expand a sidebar the user had collapsed by hand — using a
    /// window size from the *previous* run to justify it.
    #[test]
    fn a_restored_sidebar_carries_only_the_users_own_choice() {
        for collapsed in [true, false] {
            let restored = SidebarState::restored(collapsed);
            assert_eq!(restored.collapsed, collapsed);
            assert!(!restored.collapsed_by_width);
            assert!(restored.narrow.is_none());

            // …so the first width this run sees is recorded, not acted on.
            assert_eq!(restored.resize(px(WIDE)).collapsed, collapsed);
            assert_eq!(restored.resize(px(NARROW)).collapsed, collapsed);
        }
    }

    /// A sidebar restored expanded still collapses when the window is dragged
    /// narrow, and comes back when it is widened — the width rule is unchanged
    /// by restoration.
    #[test]
    fn the_width_rule_still_applies_to_a_restored_sidebar() {
        let restored = SidebarState::restored(false).resize(px(WIDE));
        let collapsed = restored.resize(px(NARROW));
        assert!(collapsed.collapsed && collapsed.collapsed_by_width);
        assert!(!collapsed.resize(px(WIDE)).collapsed);
    }

    #[test]
    fn the_breakpoint_is_where_labels_would_start_costing_the_pane_its_minimum() {
        // The rule the constant exists to express: at exactly the breakpoint an
        // expanded sidebar still leaves the main pane its minimum, and one pixel
        // narrower it does not.
        assert_eq!(
            AUTO_COLLAPSE_WIDTH - SIDEBAR_WIDTH - PANE_CHROME_WIDTH,
            MAIN_MIN_WIDTH
        );
        assert_eq!(AUTO_COLLAPSE_WIDTH, 792.);
    }

    #[test]
    fn the_smallest_allowed_window_still_holds_the_pane_minimum() {
        let min = window_min_size();

        assert_eq!(
            min.width,
            px(SIDEBAR_RAIL_WIDTH + PANE_CHROME_WIDTH + MAIN_MIN_WIDTH)
        );
        assert_eq!(min.height, px(PANE_CHROME_HEIGHT + MAIN_MIN_HEIGHT));
        assert_eq!(min, gpui::size(px(600.), px(440.)));

        // …and a window at that floor is narrow enough that the rail, not the
        // labelled sidebar, is what the width leaves room for. If these two
        // ever disagree the smallest window would open with a sidebar it cannot
        // afford.
        assert!(min.width < px(AUTO_COLLAPSE_WIDTH));
    }

    #[test]
    fn dodo_opens_collapsed_and_the_first_width_seen_is_not_a_crossing() {
        let start = SidebarState::new();
        assert!(start.collapsed);
        assert!(!start.collapsed_by_width);

        // Opening wide must not expand a sidebar the app deliberately opened
        // collapsed…
        assert!(start.resize(px(WIDE)).collapsed);
        // …and opening narrow must not mark it as the width's doing, or the
        // first widening would expand it.
        let narrow_first = start.resize(px(NARROW));
        assert!(narrow_first.collapsed);
        assert!(!narrow_first.collapsed_by_width);
        assert!(narrow_first.resize(px(WIDE)).collapsed);
    }

    #[test]
    fn narrowing_past_the_breakpoint_collapses_the_sidebar_and_widening_restores_it() {
        let expanded = SidebarState::new().resize(px(WIDE)).toggle();
        assert!(!expanded.collapsed);

        let collapsed = expanded.resize(px(NARROW));
        assert!(collapsed.collapsed);
        assert!(collapsed.collapsed_by_width);

        let restored = collapsed.resize(px(WIDE));
        assert!(!restored.collapsed);
        assert!(!restored.collapsed_by_width);
    }

    #[test]
    fn the_breakpoint_itself_counts_as_wide() {
        let expanded = SidebarState::new().resize(px(WIDE)).toggle();

        assert!(!expanded.resize(px(AUTO_COLLAPSE_WIDTH)).collapsed);
        assert!(expanded.resize(px(AUTO_COLLAPSE_WIDTH - 1.)).collapsed);
    }

    #[test]
    fn expanding_the_sidebar_at_a_narrow_width_is_not_undone_by_the_next_frame() {
        // The defect this whole struct exists to prevent: `render` applies the
        // width rule every frame, so a level-triggered rule would collapse the
        // sidebar again before the user let go of the mouse.
        let mut state = SidebarState::new().resize(px(WIDE)).resize(px(NARROW));
        state = state.toggle();
        assert!(!state.collapsed);

        for _ in 0..10 {
            state = state.resize(px(NARROW));
            assert!(!state.collapsed, "the width rule must not fight the user");
        }
        // Even a different narrow width is not a crossing.
        assert!(!state.resize(px(NARROW - 100.)).collapsed);
    }

    #[test]
    fn a_sidebar_the_user_collapsed_stays_collapsed_when_the_window_grows() {
        let by_hand = SidebarState::new().resize(px(WIDE));
        assert!(by_hand.collapsed && !by_hand.collapsed_by_width);

        // Narrow and wide again: nothing here was the width's to restore.
        let round_trip = by_hand.resize(px(NARROW)).resize(px(WIDE));
        assert!(round_trip.collapsed);

        // Same once the user has expanded and re-collapsed it by hand.
        let re_collapsed = by_hand.toggle().toggle();
        assert!(re_collapsed.collapsed);
        assert!(re_collapsed.resize(px(NARROW)).resize(px(WIDE)).collapsed);
    }

    #[test]
    fn a_tool_row_takes_the_collapsed_state_the_group_hands_it() {
        // `SidebarGroup` calls `collapsed(..)` on each child on its way to
        // rendering it, and `ToolItem` decides whether to show a tooltip from
        // that same flag. Dropping it on the floor would leave every collapsed
        // icon anonymous, and nothing else would fail.
        let item = ToolItem {
            item: SidebarMenuItem::new("JSON Formatter"),
            title: "JSON Formatter".into(),
            collapsed: false,
        };

        assert!(!item.is_collapsed());
        assert!(item.clone().collapsed(true).is_collapsed());
        assert!(!item.collapsed(true).collapsed(false).is_collapsed());
    }

    /// Trap 7's second half. `crate::tools` proves neither footer button is a
    /// row in the table; this proves the Settings button is still *in* the
    /// footer, because the Features page it opens is the only way back from a
    /// sidebar the user has cut down to one tool.
    #[test]
    fn settings_stays_in_the_sidebar_footer() {
        let source = include_str!("layout.rs");
        let footer = item_source(source, "fn render(&mut self, window: &mut Window");
        assert!(
            footer.contains("\"open-settings\""),
            "the Settings button has left the sidebar footer; the Features page \
             assumes it is always reachable",
        );
    }

    #[test]
    fn every_icon_on_the_collapsed_rail_can_still_name_itself() {
        // A source scan, because a tooltip cannot be driven from a test on
        // macOS: `Root::new` dereferences a real `NSView`, so there is no
        // window to hover. Both call sites are checked because they reach the
        // tooltip by different routes — the tool rows through `ToolItem`,
        // which exists only for this, and the footer through `Button::tooltip`.
        let source = include_str!("layout.rs");

        assert!(
            item_source(source, "fn footer_button(").contains(".tooltip("),
            "the collapsed footer buttons show no label, so they must show a tooltip",
        );
        assert!(
            item_source(source, "impl SidebarItem for ToolItem").contains(".tooltip("),
            "the collapsed tool rows show no label, so they must show a tooltip",
        );
        // …and the extraction really is bounded, or the two above would pass
        // on any file that mentions a tooltip anywhere.
        assert!(!item_source(source, "fn pane_title(").contains(".tooltip("));
    }

    #[test]
    fn the_docker_row_reads_docker_while_the_pane_reads_the_page() {
        assert_eq!(View::Docker.title(), Str::from(docker::Text::Docker));

        for (page, expected) in [
            (DockerPage::Containers, Str::from(docker::Text::Containers)),
            (DockerPage::Images, Str::from(docker::Text::Images)),
            (DockerPage::Volumes, Str::from(docker::Text::Volumes)),
            (DockerPage::Networks, Str::from(docker::Text::Networks)),
        ] {
            assert_eq!(
                title_of(View::Docker, page),
                expected,
                "the pane heading must follow the rail's selected page",
            );
        }
    }

    #[test]
    fn the_other_tools_ignore_the_docker_page() {
        for view in [
            View::JsonFormatter,
            View::EncoderDecoder,
            View::ApiExplorer,
            View::Cleaner,
            View::Database,
        ] {
            for page in DockerPage::ALL {
                assert_eq!(title_of(view, page), view.title());
            }
        }
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
