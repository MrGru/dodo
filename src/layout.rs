use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::sidebar::{
    Sidebar, SidebarCollapsible, SidebarGroup, SidebarHeader, SidebarItem, SidebarMenuItem,
};
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, Collapsible, StyledExt as _, h_flex, v_flex};

use crate::api_explorer::ApiExplorer;
use crate::app_icon::AppIcon;
use crate::database::DatabaseView;
use crate::docker::{DockerPage, DockerView};
use crate::encoder_decoder::{EncoderDecoder, Format};
use crate::i18n::{Str, t};
use crate::json_formatter::JsonFormatter;
use crate::quick_nav::models::route::Route;
use crate::quick_nav::{self, LeaveInsertMode, QuickNav, QuickNavigate};
use crate::settings;
use crate::updater;

/// Which tool is currently shown in the main pane. Selecting a sidebar item
/// switches the active view.
///
/// Every tool is one flat row, Docker included: its four pages moved onto the
/// tab rail inside [`DockerView`] because a nested sidebar group renders no
/// children at all once the sidebar collapses to icons, which made those pages
/// unreachable.
///
/// Adding a tool means: a variant here, a row in [`View::ALL`], an arm in
/// [`View::title`]/[`View::icon`], a field on [`Layout`] holding the view
/// entity, and an arm in the main-pane `match` of [`Layout::render`].
///
/// **If the new tool can also accept a pasted value**, quick navigation costs
/// three more small things and nothing else: a `Detector` variant with its arm
/// in `quick_nav::models::detect`, a `Route` variant, and an arm in
/// [`Layout::apply_route`] — which is the one place a route meets a `View`.
/// `quick_nav::models::detect`'s module doc is where the *order* it goes in has
/// to be argued.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum View {
    JsonFormatter,
    EncoderDecoder,
    ApiExplorer,
    Docker,
    Database,
}

impl View {
    /// Every tool, in sidebar order.
    const ALL: [View; 5] = [
        View::JsonFormatter,
        View::EncoderDecoder,
        View::ApiExplorer,
        View::Docker,
        View::Database,
    ];

    /// The tool's own name — what the sidebar row reads. The main pane's title
    /// goes through [`pane_title`] instead, because Docker titles itself after
    /// the rail's selected page.
    fn title(self) -> Str {
        match self {
            View::JsonFormatter => Str::JsonFormatterTitle,
            View::EncoderDecoder => Str::EncoderDecoderTitle,
            View::ApiExplorer => Str::ApiExplorerTitle,
            View::Docker => Str::Docker,
            View::Database => Str::DatabaseTitle,
        }
    }

    fn icon(self) -> AppIcon {
        match self {
            View::JsonFormatter => AppIcon::Json,
            View::EncoderDecoder => AppIcon::Binary,
            View::ApiExplorer => AppIcon::Globe,
            View::Docker => AppIcon::Container,
            View::Database => AppIcon::Database,
        }
    }
}

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
/// None of it is persisted; [`Layout::new`] says why.
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
    /// How dodo opens: on the icon rail, by choice rather than by width.
    const fn new() -> Self {
        Self {
            collapsed: true,
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

pub struct Layout {
    collapsible: SidebarCollapsible,
    sidebar: SidebarState,
    active: View,
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
    json_formatter: Entity<JsonFormatter>,
    encoder_decoder: Entity<EncoderDecoder>,
    api_explorer: Entity<ApiExplorer>,
    docker: Entity<DockerView>,
    database: Entity<DatabaseView>,
}

impl Layout {
    /// dodo opens on the **icon rail**, not the labelled sidebar: the tools are
    /// five fixed entries a user learns once, and the pane they are choosing
    /// between is the whole point of the window, so 240px of permanent chrome
    /// is a poor default. The toggle in the pane header is unchanged, and every
    /// collapsed icon carries its title as a tooltip, which is what keeps the
    /// rail readable to someone who has not learned it yet.
    ///
    /// The choice is **not persisted**, deliberately: dodo's six saved files are
    /// listed in `CLAUDE.md`, adding a seventh is a decision of its own, and a
    /// sidebar that opens the same way every time is at least predictable.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // dodo opens in normal mode, so the pane takes focus straight away. See
        // [`Layout::focus`] for why that has to be someone's rather than nobody's.
        let focus = cx.focus_handle();
        window.focus(&focus, cx);

        Self {
            collapsible: SidebarCollapsible::Icon,
            sidebar: SidebarState::new(),
            active: View::JsonFormatter,
            focus,
            json_formatter: cx.new(|cx| JsonFormatter::new(window, cx)),
            encoder_decoder: cx.new(|cx| EncoderDecoder::new(window, cx)),
            api_explorer: cx.new(|cx| ApiExplorer::new(window, cx)),
            docker: cx.new(|cx| DockerView::new(window, cx)),
            database: cx.new(|cx| DatabaseView::new(window, cx)),
        }
    }

    /// Switches the main pane, and tells Docker whether its polling should be
    /// running.
    ///
    /// Both the sidebar rows and quick navigation come through here, so a jump
    /// leaves the app in exactly the state a click would have.
    fn activate(&mut self, view: View, cx: &mut Context<Self>) {
        self.active = view;
        self.docker.update(cx, |docker, cx| match view {
            View::Docker => docker.activate(cx),
            _ => docker.set_section_active(false, cx),
        });
        cx.notify();
    }

    /// `Cmd+V` / `Ctrl+V` / `p` in normal mode: read the clipboard, work out
    /// what it is, and go there.
    ///
    /// Nothing happens on three ordinary paths — the feature is off, the
    /// clipboard holds no text, or nothing was recognised confidently — and in
    /// each the keystroke is propagated, because a shortcut that silently
    /// swallows a key it did not use is worse than one that declines it.
    ///
    /// `quick_nav`'s key context is what guarantees this only runs with no input
    /// focused; there is no mode flag to consult and none to get out of step.
    fn quick_navigate(&mut self, _: &QuickNavigate, window: &mut Window, cx: &mut Context<Self>) {
        let route = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .and_then(|text| QuickNav::detect(&text, cx));

        let Some(route) = route else {
            cx.propagate();
            return;
        };
        self.apply_route(route, window, cx);
    }

    /// Hands a detected route to the tool that owns it.
    ///
    /// **The one place a `Route` meets a `View`**, which is what keeps adding a
    /// tool to quick navigation from being an edit in three files: the detector
    /// decides *what*, this decides *where*, and neither knows the other's list.
    fn apply_route(&mut self, route: Route, window: &mut Window, cx: &mut Context<Self>) {
        match route {
            Route::Json(text) => {
                self.activate(View::JsonFormatter, cx);
                self.json_formatter
                    .update(cx, |view, cx| view.accept_text(text, window, cx));
            }
            Route::Jwt(token) => {
                self.activate(View::EncoderDecoder, cx);
                self.encoder_decoder.update(cx, |view, cx| {
                    view.accept_decode(token, Format::Jwt, window, cx)
                });
            }
            Route::Base64 { text, url_safe } => {
                let format = if url_safe {
                    Format::Base64UrlSafe
                } else {
                    Format::Base64
                };
                self.activate(View::EncoderDecoder, cx);
                self.encoder_decoder
                    .update(cx, |view, cx| view.accept_decode(text, format, window, cx));
            }
            Route::Curl(snapshot) => {
                self.activate(View::ApiExplorer, cx);
                self.api_explorer
                    .update(cx, |view, cx| view.accept_curl(*snapshot, window, cx));
            }
            Route::Database(parsed) => {
                self.activate(View::Database, cx);
                self.database
                    .update(cx, |view, cx| view.accept_uri(&parsed, cx));
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

    /// The sidebar menu: one flat row per tool, no nesting. Nesting is what the
    /// icon-collapsed sidebar cannot render, so there is none.
    fn menu(&self, cx: &mut Context<Self>) -> [ToolItem; View::ALL.len()] {
        View::ALL.map(|view| self.tool_item(view, cx))
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
        let title = pane_title(self.active, self.docker.read(cx).page());

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
                    .child(SidebarGroup::new(t(Str::Tools, cx)).children(self.menu(cx)))
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
                                    Str::CheckForUpdates,
                                    icon_collapsed,
                                    cx,
                                )
                                .on_click(|_, window, cx| updater::open(window, cx)),
                            )
                            .child(
                                footer_button(
                                    "open-settings",
                                    AppIcon::Settings,
                                    Str::Settings,
                                    icon_collapsed,
                                    cx,
                                )
                                .on_click(|_, window, cx| settings::open(window, cx)),
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
                                        cx.notify();
                                    })),
                            )
                            .child(div().font_bold().child(t(title, cx))),
                    )
                    // The tool scrolls rather than being squeezed. The inner
                    // box is `size_full` so on any ordinary window it is
                    // exactly the pane and nothing scrolls at all; `min_w` /
                    // `min_h` are the floor under that, and the only thing they
                    // change is that below it the content keeps its size and
                    // this container gains a scrollbar.
                    //
                    // The header row above stays outside, so the pane title and
                    // the sidebar toggle never scroll away.
                    //
                    // `w_full` on the scroll container is load-bearing: one
                    // that sizes to its content leaves rules and rows stopping
                    // short of the pane's edge.
                    .child(
                        div()
                            .id("main-pane")
                            .w_full()
                            .flex_1()
                            .min_h_0()
                            .overflow_scroll()
                            .child(
                                div()
                                    .size_full()
                                    .min_w(px(MAIN_MIN_WIDTH))
                                    .min_h(px(MAIN_MIN_HEIGHT))
                                    .map(|this| match self.active {
                                        View::JsonFormatter => {
                                            this.child(self.json_formatter.clone())
                                        }
                                        View::EncoderDecoder => {
                                            this.child(self.encoder_decoder.clone())
                                        }
                                        View::ApiExplorer => this.child(self.api_explorer.clone()),
                                        View::Docker => this.child(self.docker.clone()),
                                        View::Database => this.child(self.database.clone()),
                                    }),
                            ),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use std::mem::{Discriminant, discriminant};

    use gpui::px;
    use gpui_component::Collapsible as _;
    use gpui_component::sidebar::SidebarMenuItem;

    use super::{
        AUTO_COLLAPSE_WIDTH, MAIN_MIN_HEIGHT, MAIN_MIN_WIDTH, PANE_CHROME_HEIGHT,
        PANE_CHROME_WIDTH, SIDEBAR_RAIL_WIDTH, SIDEBAR_WIDTH, SidebarState, ToolItem, View,
        pane_title, window_min_size,
    };
    use crate::docker::DockerPage;
    use crate::i18n::Str;

    /// A width comfortably on each side of the breakpoint. 1280 and 520 are the
    /// two the layout is reviewed at.
    const WIDE: f32 = 1280.;
    const NARROW: f32 = 520.;

    fn title_of(view: View, page: DockerPage) -> Discriminant<Str> {
        discriminant(&pane_title(view, page))
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

    #[test]
    fn the_sidebar_lists_every_tool_once_with_docker_flat_and_last() {
        assert_eq!(
            View::ALL,
            [
                View::JsonFormatter,
                View::EncoderDecoder,
                View::ApiExplorer,
                View::Docker,
                View::Database,
            ]
        );
        // One row per tool: Docker and Database are each a single entry, not a
        // group of children — an icon-collapsed sidebar renders no children at
        // all, which is what made Docker's four pages unreachable.
        assert_eq!(View::ALL.len(), 5);
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
        assert_eq!(
            discriminant(&View::Docker.title()),
            discriminant(&Str::Docker)
        );

        for (page, expected) in [
            (DockerPage::Containers, Str::Containers),
            (DockerPage::Images, Str::Images),
            (DockerPage::Volumes, Str::Volumes),
            (DockerPage::Networks, Str::Networks),
        ] {
            assert_eq!(
                title_of(View::Docker, page),
                discriminant(&expected),
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
            View::Database,
        ] {
            for page in DockerPage::ALL {
                assert_eq!(title_of(view, page), discriminant(&view.title()));
            }
        }
    }
}
