//! The Settings dialog, plus the app-level state it edits.
//!
//! There is deliberately no settings struct of our own for appearance: font
//! size, border radius and colours all live on `gpui_component::Theme`, which
//! is already a global the whole app renders from, so the dialog reads and
//! writes that directly and every change is live. Language is the one setting
//! with no home in `Theme`; it lives in [`crate::i18n::Language`].
//!
//! **Every setting here is persisted across restarts except one**, in
//! `session.json` — see [`crate::session`], which the captain asked for on
//! 2026-08-06. (Quick navigation keeps its own file, `quick-nav.json`, because
//! it was already there and its fields hold text the user typed.)
//!
//! The exception is **Run scripts**, and it is not an omission. `ScriptPolicy`
//! goes back to the cautious `Ask for imported` at every launch because it is
//! the gate in front of running code that arrived inside someone else's
//! collection file, not a preference about how the app looks. The *approvals*
//! it collects are persisted, per script, in `script-consent.json`, which is
//! the right granularity for that memory. [`run_scripts_field`] says the same
//! thing at the control; [`crate::session`]'s module doc argues it.
//!
//! The dialog body is [`SettingsView`]: a quick-navigation search box above the
//! library's own settings panel. Typing fuzzy-matches every setting and picking
//! a result jumps to it.
//!
//! # One page here is not a setting but an editor
//!
//! **Features** — which tools the sidebar lists and in what order, asked for on
//! 2026-08-06 — is the one page whose state is not a global. It edits `Layout`,
//! because switching a tool off has to move the main pane off it, and that is
//! the pane's business rather than a preference's. [`features_page`] carries the
//! consequences: a hand-built row instead of a [`SettingField`], a weak handle
//! to the pane instead of a `&mut App` closure pair, and the reorder rules
//! themselves nowhere near here — they are pure data in
//! [`crate::session::models::features`].

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::list::{List, ListDelegate, ListItem, ListState};
use gpui_component::setting::{
    SelectIndex, SettingField, SettingGroup, SettingItem, SettingPage, Settings,
};
use gpui_component::switch::Switch;
use gpui_component::tooltip::Tooltip;
use gpui_component::{
    ActiveTheme as _, Disableable as _, IndexPath, Sizable as _, Theme, ThemeRegistry,
    WindowExt as _, h_flex, v_flex,
};
use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use unicode_normalization::UnicodeNormalization as _;
use unicode_normalization::char::is_combining_mark;

use crate::api_explorer::ScriptPolicy;
use crate::api_explorer::models::script_consent::ConsentPolicy;
use crate::app_icon::AppIcon;
use crate::assets::Assets;
use crate::dialog_slot::{self, SingleDialog};
use crate::i18n::{Language, Str, t};
use crate::layout::{Layout, View};
use crate::quick_nav::QuickNav;
use crate::quick_nav::models::detect::Detector;
use crate::session::Session;
use crate::session::models::features::FeatureError;

/// Base text size in px, largest first. `Theme::font_size` drives the window's
/// rem size (see the library's `Root::render`), so these scale the whole UI.
const FONT_SIZES: [(Str, f32); 3] = [(Str::Large, 18.), (Str::Medium, 16.), (Str::Small, 14.)];
const DEFAULT_FONT_SIZE: f32 = 16.;

const RADII: [f32; 4] = [8., 6., 4., 0.];
const DEFAULT_RADIUS: f32 = 6.;

/// Themes offered in the dialog, by the `name` inside `assets/themes/*.json`.
/// "Default Light"/"Default Dark" are built into the library's registry; the
/// rest come from the vendored files loaded in [`init`].
const THEMES: [&str; 16] = [
    "Default Light",
    "Default Dark",
    "Ayu Light",
    "Catppuccin Latte",
    "Everforest Light",
    "Flexoki Light",
    "Gruvbox Light",
    "Hybrid Light",
    "macOS Classic Light",
    "Mellifluous Light",
    "Molokai Light",
    "Adventure Time",
    "Alduin",
    "Asciinema",
    "Ayu Dark",
    "Catppuccin Frappe",
];

/// Width of the dialog card, and of the settings panel's own sidebar inside it.
///
/// Named because the row layout depends on what is left over: the card spends
/// 2px on its border and `Dialog`'s own `Edges::all(16)` padding on each side,
/// the settings sidebar takes [`SIDEBAR_WIDTH`] of the rest, and each setting
/// row is what remains less the page's own `px_4`.
/// `row_layout::a_pattern_row_stays_inside_the_card` does that arithmetic
/// against a real frame; see [`pattern_field`] for why it matters.
const DIALOG_WIDTH: Pixels = px(760.);
const SIDEBAR_WIDTH: Pixels = px(200.);

/// Height of the search box once the user has typed something. It is fixed so
/// that the list's own `size_full` layout has a definite box to fill; an empty
/// query collapses the box back to [`collapsed_height`].
///
/// The box is drawn as an overlay, so growing to this height covers the
/// settings panel instead of pushing it down.
const RESULTS_HEIGHT: f32 = 232.;

/// Height of the search box with no results under it.
///
/// The library draws the query input at `h_8` — 2rem, so it tracks the font
/// size setting — with a 1px rule under it, and the box adds its own 1px border
/// top and bottom.
fn collapsed_height(window: &Window) -> Pixels {
    window.rem_size() * 2. + px(3.)
}

/// Key context of the search box. Escape has to be bound *tighter* than the
/// text input's own Escape, which propagates all the way to the dialog and
/// closes it — see [`SettingsView::dismiss_results`].
const SEARCH_CONTEXT: &str = "SettingsSearch";

actions!(dodo, [DismissSettingsResults]);

/// Registers the vendored themes with the library's [`ThemeRegistry`], and the
/// one key binding the search box needs.
///
/// Must run after `gpui_component::init`, which creates the registry and binds
/// the library's own keys — Escape resolves by depth first and registration
/// order second, so ours has to be registered last to win the tie.
pub fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new(
        "escape",
        DismissSettingsResults,
        Some(&format!("{SEARCH_CONTEXT} > Input")),
    )]);

    let themes: Vec<_> = Assets::themes().collect();
    let registry = ThemeRegistry::global_mut(cx);

    for (path, data) in themes {
        let Ok(json) = std::str::from_utf8(&data) else {
            eprintln!("theme {path} is not valid UTF-8");
            continue;
        };
        if let Err(err) = registry.load_themes_from_str(json) {
            eprintln!("failed to load theme {path}: {err}");
        }
    }
}

/// The marker that keys this dialog's single slot. See [`crate::dialog_slot`].
struct SettingsDialog;

impl SingleDialog for SettingsDialog {}

/// Opens the Settings dialog. The dialog is dismissed with Escape, the close
/// button, or a click on the overlay.
///
/// **There is only ever one.** Two things open it — the sidebar footer's button
/// and the menu bar item's Settings row — and a dialog layer is a stack, so
/// until [`dialog_slot`](crate::dialog_slot) was put in front of it the two
/// paths put two identical cards on top of each other. A second request is
/// dropped rather than served; `on_close` is what gives the slot back, and it
/// pops nothing itself, so one dismissal stays one dismissal.
///
/// `layout` is the pane the Features page edits, and is the one thing here that
/// is not a global: which tools the sidebar lists is `Layout`'s state, because
/// changing it has to move the main pane off a tool that has just stopped being
/// listed. It is held **weakly** and never read while this runs — `open` is
/// reached from a click listener that has `Layout` leased, so a read here would
/// panic. See `gpui-component-recipes`.
pub fn open(layout: WeakEntity<Layout>, window: &mut Window, cx: &mut App) {
    if !dialog_slot::claim::<SettingsDialog>(window, cx) {
        return;
    }

    let view = cx.new(|cx| SettingsView::new(layout, window, cx));

    window.open_dialog(cx, move |dialog, _, cx| {
        dialog
            .title(t(Str::Settings, cx))
            .w(DIALOG_WIDTH)
            .on_close(|_, _, cx| dialog_slot::release::<SettingsDialog>(cx))
            .child(view.clone())
    });
}

/// Every setting the search box can navigate to.
///
/// Written out by hand rather than derived from [`pages`], because a
/// [`SettingItem`] exposes neither its title nor the page it ended up on. This
/// is the one list that has to be kept in step with `pages` by eye.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Setting {
    Language,
    RunScripts,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    StartWithOs,
    FontSize,
    BorderRadius,
    Theme,
    QuickNavEnabled,
    /// One per detector, in [`Detector::ORDER`] — so the searchable list grows
    /// with the detector list rather than beside it.
    QuickNavPattern(Detector),
    /// The whole tool list, which is one custom element rather than a control
    /// per tool, so it is one search result rather than six.
    Features,
}

impl Setting {
    /// Every setting, in the order the search box lists ties.
    fn all() -> Vec<Setting> {
        let mut all = vec![
            Setting::Language,
            Setting::RunScripts,
            Setting::FontSize,
            Setting::BorderRadius,
            Setting::Theme,
            Setting::QuickNavEnabled,
        ];
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        all.push(Setting::StartWithOs);
        all.extend(Detector::ORDER.map(Setting::QuickNavPattern));
        all.push(Setting::Features);
        all
    }

    /// Index into the vec [`pages`] returns — the sidebar entry to open.
    fn page_ix(self) -> usize {
        match self {
            Setting::Language | Setting::RunScripts => 0,
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            Setting::StartWithOs => 0,
            Setting::FontSize | Setting::BorderRadius | Setting::Theme => 1,
            Setting::QuickNavEnabled | Setting::QuickNavPattern(_) => 2,
            Setting::Features => 3,
        }
    }

    fn label(self) -> Str {
        match self {
            Setting::Language => Str::Language,
            Setting::RunScripts => Str::RunScripts,
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            Setting::StartWithOs => Str::StartWithOs,
            Setting::FontSize => Str::FontSize,
            Setting::BorderRadius => Str::BorderRadius,
            Setting::Theme => Str::Theme,
            Setting::QuickNavEnabled => Str::QuickNavEnabled,
            Setting::QuickNavPattern(detector) => detector.label(),
            Setting::Features => Str::Features,
        }
    }

    /// The section heading the setting sits under, shown beside every result so
    /// the user knows where the jump will land.
    fn section(self) -> Str {
        match self {
            Setting::Language | Setting::RunScripts => Str::General,
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            Setting::StartWithOs => Str::General,
            Setting::FontSize | Setting::BorderRadius | Setting::Theme => Str::Appearance,
            Setting::QuickNavEnabled | Setting::QuickNavPattern(_) => Str::QuickNavigation,
            Setting::Features => Str::Features,
        }
    }
}

/// The searchable text of every setting, in [`Setting::ALL`] order.
///
/// Both the label and the section name go in, so that typing a section name
/// lists that section's settings, exactly as the item keywords used to do for
/// the library's own search box.
fn haystacks(cx: &App) -> Vec<String> {
    Setting::all()
        .iter()
        .map(|setting| format!("{} {}", t(setting.label(), cx), t(setting.section(), cx)))
        .collect()
}

/// Strips the accents off `text`, so a Vietnamese label can be found by typing
/// it plainly — "co chu" for "Cỡ chữ", which is how most people type.
///
/// nucleo's own normalization table stops at Latin-1 and does not know
/// Vietnamese's horned and hooked vowels, so decomposing and dropping the
/// combining marks is the part it cannot do. `đ` has no combining form and is
/// mapped by hand.
fn fold(text: &str) -> String {
    text.nfd()
        .filter(|c| !is_combining_mark(*c))
        .map(|c| match c {
            'đ' => 'd',
            'Đ' => 'D',
            c => c,
        })
        .collect()
}

/// Fuzzy-ranks `haystacks` against `query`, best match first.
///
/// Returns `(index, score)` for the haystacks that match and drops the rest, so
/// "brdr" finds "Border radius" even though "Border radius" does not contain
/// "brdr". An empty query matches nothing: the result list is a jump
/// affordance, not a browse list. `sort_by` is stable, so equal scores keep the
/// input order.
fn rank(query: &str, haystacks: &[String]) -> Vec<(usize, u32)> {
    let query = fold(query.trim());
    if query.is_empty() {
        return Vec::new();
    }

    // `Ignore` rather than `Smart`: the labels are sentence-cased and someone
    // typing "Font" should not be held to matching the capital F.
    let pattern = Pattern::new(
        &query,
        CaseMatching::Ignore,
        Normalization::Smart,
        AtomKind::Fuzzy,
    );
    let mut matcher = Matcher::new(Config::DEFAULT);
    let mut buf = Vec::new();

    let mut scored: Vec<(usize, u32)> = haystacks
        .iter()
        .enumerate()
        .filter_map(|(ix, haystack)| {
            pattern
                .score(Utf32Str::new(&fold(haystack), &mut buf), &mut matcher)
                .map(|score| (ix, score))
        })
        .collect();

    scored.sort_by(|(_, a), (_, b)| b.cmp(a));
    scored
}

/// The result list: ranked matches for the current query, and the jump.
struct SearchDelegate {
    view: WeakEntity<SettingsView>,
    /// Kept here because [`ListState`] does not expose its query input, and the
    /// panel's height depends on whether the user has typed anything.
    query: SharedString,
    matches: Vec<Setting>,
    selected: Option<IndexPath>,
}

impl ListDelegate for SearchDelegate {
    type Item = ListItem;

    fn perform_search(
        &mut self,
        query: &str,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        self.query = query.trim().to_owned().into();
        let all = Setting::all();
        self.matches = rank(&self.query, &haystacks(cx))
            .into_iter()
            .filter_map(|(ix, _)| all.get(ix).copied())
            .collect();
        Task::ready(())
    }

    fn items_count(&self, _: usize, _: &App) -> usize {
        self.matches.len()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let setting = *self.matches.get(ix.row)?;

        Some(
            ListItem::new(ix.row).h(px(36.)).child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .w_full()
                    .child(t(setting.label(), cx))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(t(setting.section(), cx)),
                    ),
            ),
        )
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        h_flex()
            .size_full()
            .justify_center()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(t(Str::NoSettingsMatch, cx))
    }

    /// An empty query leaves the dialog exactly as it was before this feature:
    /// a search box and nothing under it.
    fn render_initial(
        &mut self,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) -> Option<AnyElement> {
        Some(div().into_any_element())
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) {
        self.selected = ix;
    }

    /// Enter, or a click on a row. The list state is mid-update here, so the
    /// jump itself is deferred by [`SettingsView::navigate_to`].
    fn confirm(&mut self, _: bool, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        let Some(setting) = self
            .selected
            .and_then(|ix| self.matches.get(ix.row).copied())
        else {
            return;
        };

        _ = self
            .view
            .update(cx, |view, cx| view.navigate_to(setting, window, cx));
    }
}

/// The dialog body: the search box and result list above the library's own
/// settings panel.
struct SettingsView {
    search: Entity<ListState<SearchDelegate>>,
    /// The pane the Features page edits. See [`open`] for why it is weak.
    layout: WeakEntity<Layout>,
    /// Index into the vec [`pages`] returns — the open sidebar entry.
    page_ix: usize,
    /// Bumped on every jump. `Settings` keeps its selected page in window state
    /// keyed by the element id and reads `default_selected_index` only when
    /// that state is first created, so handing it a fresh id is the only way to
    /// drive the selection from outside.
    nonce: usize,
    /// Where the last jump landed. Its control is drawn with the accent colour
    /// until the next jump, so the setting is obvious on arrival.
    highlight: Option<Setting>,
}

impl SettingsView {
    fn new(layout: WeakEntity<Layout>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let view = cx.entity().downgrade();
        let search = cx.new(|cx| {
            let delegate = SearchDelegate {
                view,
                query: SharedString::default(),
                matches: Vec::new(),
                selected: None,
            };

            ListState::new(delegate, window, cx).searchable(true)
        });

        Self {
            search,
            layout,
            page_ix: 0,
            nonce: 0,
            highlight: None,
        }
    }

    /// Switches to the setting's section, highlights it, and clears the query so
    /// the result list gets out of the way.
    ///
    /// The clearing is deferred: this runs from inside the list's own update, so
    /// touching the list again here would panic.
    fn navigate_to(&mut self, setting: Setting, window: &mut Window, cx: &mut Context<Self>) {
        self.page_ix = setting.page_ix();
        self.highlight = Some(setting);
        self.nonce += 1;

        cx.defer_in(window, Self::clear_search);
        cx.notify();
    }

    /// Empties the search box and the results under it.
    ///
    /// `ListState::set_query` alone is not enough despite what its doc comment
    /// says: it goes through `InputState::set_value`, which suppresses the
    /// `Change` event, so the delegate's own search never runs. Clearing the
    /// delegate by hand is the other half.
    fn clear_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.update(cx, |list, cx| {
            list.set_query("", window, cx);

            let delegate = list.delegate_mut();
            delegate.query = SharedString::default();
            delegate.matches.clear();
            delegate.selected = None;

            cx.notify();
        });
        cx.notify();
    }

    /// Escape with results showing dismisses them; Escape with an empty query
    /// propagates, letting the input, the list and finally the dialog handle it
    /// as before.
    fn dismiss_results(
        &mut self,
        _: &DismissSettingsResults,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.search.read(cx).delegate().query.is_empty() {
            cx.propagate();
            return;
        }

        self.clear_search(window, cx);
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let searching = !self.search.read(cx).delegate().query.is_empty();
        let collapsed = collapsed_height(window);

        v_flex()
            .key_context(SEARCH_CONTEXT)
            .on_action(cx.listener(Self::dismiss_results))
            .w_full()
            .h(px(440.))
            .gap_2()
            .child(
                // The slot the search box occupies in the layout. It never grows:
                // the box itself is drawn by the overlay below, so results float
                // over the settings panel rather than pushing it down.
                div().relative().w_full().flex_none().h(collapsed).child(
                    // `deferred` paints after the rest of the dialog, which is
                    // what puts the results on top of the panel; `left_0` +
                    // `right_0` size the box from the slot's own edges, so the
                    // input inside it gets a real width to lay text out in.
                    deferred(
                        v_flex()
                            .absolute()
                            .top_0()
                            .left_0()
                            .right_0()
                            .h(if searching {
                                px(RESULTS_HEIGHT)
                            } else {
                                collapsed
                            })
                            .overflow_hidden()
                            .bg(cx.theme().background)
                            .border_1()
                            .border_color(cx.theme().border)
                            .rounded(cx.theme().radius)
                            .when(searching, |this| this.shadow_md())
                            .child(
                                List::new(&self.search)
                                    .search_placeholder(t(Str::SearchSettingsPlaceholder, cx)),
                            ),
                    )
                    .with_priority(1),
                ),
            )
            .child(
                div().flex_1().min_h_0().child(
                    Settings::new(SharedString::from(format!("dodo-settings-{}", self.nonce)))
                        .sidebar_width(SIDEBAR_WIDTH)
                        .header_style(&StyleRefinement::default().hidden())
                        .default_selected_index(SelectIndex {
                            page_ix: self.page_ix,
                            group_ix: None,
                        })
                        .pages(pages(&self.layout, self.highlight, cx)),
                ),
            )
    }
}

/// The dialog's sections, in sidebar order.
///
/// `highlight` is the setting the search box last jumped to; its control is
/// drawn with the accent colour so the user can see where they landed.
///
/// Each item still repeats its section name as a search keyword. The library's
/// own search box — which is what those keywords feed — is styled away in
/// [`SettingsView::render`] so the dialog does not end up with two search
/// boxes, but the keywords cost nothing and keep it working if it comes back.
fn pages(layout: &WeakEntity<Layout>, highlight: Option<Setting>, cx: &App) -> Vec<SettingPage> {
    let general = t(Str::General, cx);
    let appearance = t(Str::Appearance, cx);
    let lit = |setting: Setting| highlight == Some(setting);

    let mut general_group = SettingGroup::new()
        .title(general.clone())
        .item(
            SettingItem::new(
                t(Str::Language, cx),
                highlighted(language_field(), lit(Setting::Language), cx),
            )
            .description(t(Str::LanguageDescription, cx))
            .keywords([general.clone()]),
        )
        .item(
            SettingItem::new(
                t(Str::RunScripts, cx),
                highlighted(run_scripts_field(cx), lit(Setting::RunScripts), cx),
            )
            .description(t(Str::RunScriptsDescription, cx))
            .keywords([general.clone()]),
        );

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        general_group = general_group.item(
            SettingItem::new(
                t(Str::StartWithOs, cx),
                highlighted(start_with_os_field(), lit(Setting::StartWithOs), cx),
            )
            .description(t(Str::StartWithOsDescription, cx))
            .keywords([general.clone()]),
        );
    }

    // Only when there is something wrong with `session.json` — and there is
    // something to say, because a refused file means nothing is being saved at
    // all this run. In the ordinary case, first run included, this row is not
    // there. Same treatment as quick navigation's storage row below.
    if let Some(problem) = Session::store_error(cx) {
        general_group = general_group.item(
            // The message is the *description*; the control is empty because
            // there is nothing here to change.
            SettingItem::new(
                t(Str::SessionStorageProblem, cx),
                SettingField::render(|_, _, _| div()),
            )
            .description(t(problem, cx))
            .keywords([general.clone()]),
        );
    }

    vec![
        SettingPage::new(general.clone())
            .icon(AppIcon::Sliders)
            .resettable(false)
            .default_open(true)
            .group(general_group),
        SettingPage::new(appearance.clone())
            .icon(AppIcon::Palette)
            .resettable(false)
            .group(
                SettingGroup::new()
                    .title(appearance.clone())
                    .item(
                        SettingItem::new(
                            t(Str::FontSize, cx),
                            highlighted(font_size_field(cx), lit(Setting::FontSize), cx),
                        )
                        .description(t(Str::FontSizeDescription, cx))
                        .keywords([appearance.clone()]),
                    )
                    .item(
                        SettingItem::new(
                            t(Str::BorderRadius, cx),
                            highlighted(radius_field(), lit(Setting::BorderRadius), cx),
                        )
                        .description(t(Str::BorderRadiusDescription, cx))
                        .keywords([appearance.clone()]),
                    )
                    .item(
                        SettingItem::new(
                            t(Str::Theme, cx),
                            highlighted(theme_field(), lit(Setting::Theme), cx),
                        )
                        .description(t(Str::ThemeDescription, cx))
                        .keywords([appearance]),
                    ),
            ),
        quick_nav_page(highlight, cx),
        features_page(layout, cx),
    ]
}

/// Height of one row on the Features page.
///
/// Fixed so that the rows are a regular ladder however long a tool's name is,
/// which is what makes a drag land where it looks like it will. `h_8` is the
/// height a `SidebarMenuItem` row uses, so the list reads as the sidebar it
/// edits.
const FEATURE_ROW_HEIGHT: Pixels = px(34.);

/// The Features page: one row per tool, in the sidebar's own order.
///
/// **One custom element for the whole list, not one [`SettingItem`] per tool.**
/// A `SettingItem` is a label with a control beside it and no way to reach the
/// row itself, so it can carry a switch but not a drag handle, a drop target or
/// a position — and the position is the feature. [`SettingItem::render`] hands
/// over the whole row instead.
///
/// The state it edits is `Layout`'s rather than a global's, which is the other
/// reason this page cannot use [`SettingField`]: those are get/set closure pairs
/// over `&App`/`&mut App`, and the two side effects that must follow every
/// change — persisting the list, and moving the pane off a tool that is no
/// longer listed — belong to the pane. [`Layout::set_tool_enabled`] and
/// [`Layout::move_tool`] are the seam.
fn features_page(layout: &WeakEntity<Layout>, cx: &App) -> SettingPage {
    let title = t(Str::Features, cx);
    let layout = layout.clone();

    SettingPage::new(title.clone())
        .icon(AppIcon::Layers)
        .resettable(false)
        .group(
            SettingGroup::new().title(title.clone()).item(
                SettingItem::render(move |_, _, cx| feature_list(&layout, cx))
                    .keywords([title.clone()]),
            ),
        )
}

/// The description, then the rows.
fn feature_list(layout: &WeakEntity<Layout>, cx: &mut App) -> AnyElement {
    let Some(pane) = layout.upgrade() else {
        return div().into_any_element();
    };
    let features = pane.read(cx).features().clone();
    let rows: Vec<AnyElement> = features
        .all()
        .iter()
        .enumerate()
        .map(|(ix, feature)| {
            feature_row(
                layout,
                feature.code,
                ix,
                features.all().len(),
                feature.enabled,
                !features.can_toggle(feature.code),
                cx,
            )
        })
        .collect();

    v_flex()
        .w_full()
        .gap_2()
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(t(Str::FeaturesDescription, cx)),
        )
        .child(v_flex().w_full().gap_1().children(rows))
        .into_any_element()
}

/// One tool's row: the handle, the tool, the two move buttons, the switch.
///
/// `locked` is "this is the last tool the sidebar has", which
/// `Features::can_toggle` answers — **not** `can_hide`, which is also false for
/// a tool that is already hidden and would draw every hidden tool's switch dead,
/// making switching one off a one-way door.
///
/// A locked switch is drawn **disabled with the reason beside it** rather than
/// left live to refuse a press: the refusal is the same either way —
/// [`Layout::set_tool_enabled`] enforces it whatever the control does — but a
/// control that visibly cannot be pressed, next to a sentence saying why,
/// explains itself before the user is puzzled rather than after.
#[allow(clippy::too_many_arguments)]
fn feature_row(
    layout: &WeakEntity<Layout>,
    code: &'static str,
    ix: usize,
    count: usize,
    enabled: bool,
    locked: bool,
    cx: &mut App,
) -> AnyElement {
    let Some(view) = View::lookup(code) else {
        // Unreachable: every code here came out of `View::codes`.
        return div().into_any_element();
    };
    let title = t(view.title(), cx);

    h_flex()
        .id(SharedString::from(format!("feature-row-{code}")))
        .w_full()
        .h(FEATURE_ROW_HEIGHT)
        .items_center()
        .gap_2()
        .px_1()
        .rounded(cx.theme().radius)
        .border_2()
        .border_color(cx.theme().transparent)
        // The drop half of the drag. `drag_over` draws the row the tool would
        // land on, so the gesture says where it is going before it is let go.
        .drag_over::<DragTool>(|this, _, _, cx| {
            this.border_color(cx.theme().drag_border)
                .bg(cx.theme().accent.opacity(0.4))
        })
        .on_drop({
            let layout = layout.clone();
            move |drag: &DragTool, _, cx| {
                let code = drag.code;
                _ = layout.update(cx, |pane, cx| pane.move_tool(code, ix, cx));
            }
        })
        .child({
            // The grab half. Only the handle starts a drag, so pressing the
            // switch or a move button never does.
            let hint = t(Str::FeatureDragToReorder, cx);

            div()
                .id(SharedString::from(format!("feature-grip-{code}")))
                .flex_shrink_0()
                .cursor_grab()
                .text_color(cx.theme().muted_foreground)
                .tooltip(move |window, cx| Tooltip::new(hint.clone()).build(window, cx))
                .on_drag(
                    DragTool {
                        code,
                        title: title.clone(),
                    },
                    |drag, _, _, cx| {
                        cx.stop_propagation();
                        cx.new(|_| drag.clone())
                    },
                )
                .child(AppIcon::GripVertical.view())
        })
        .child(div().flex_shrink_0().child(view.icon().view()))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .whitespace_nowrap()
                .child(title),
        )
        .when(locked, |this| {
            this.child(
                div()
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(t(FeatureError::LastVisibleTool.message(), cx)),
            )
        })
        .child(move_button(
            layout,
            code,
            "up",
            AppIcon::ArrowUp,
            Str::FeatureMoveUp,
            -1,
            ix == 0,
            cx,
        ))
        .child(move_button(
            layout,
            code,
            "down",
            AppIcon::ArrowDown,
            Str::FeatureMoveDown,
            1,
            ix + 1 >= count,
            cx,
        ))
        .child(
            Switch::new(SharedString::from(format!("feature-switch-{code}")))
                .checked(enabled)
                .disabled(locked)
                .tooltip(if locked {
                    t(FeatureError::LastVisibleTool.message(), cx)
                } else {
                    t(Str::FeatureShowInSidebar, cx)
                })
                .on_click({
                    let layout = layout.clone();
                    move |checked: &bool, _, cx| {
                        let checked = *checked;
                        _ = layout.update(cx, |pane, cx| {
                            // The refusal is already drawn by the row: the
                            // switch that could produce one is disabled, so a
                            // press cannot reach here.
                            let _ = pane.set_tool_enabled(code, checked, cx);
                        });
                    }
                }),
        )
        .into_any_element()
}

/// One of the two reorder buttons, which are also the whole keyboard path
/// through this page — a drag has no keyboard equivalent, and the sidebar it
/// edits is keyboard-navigable.
///
/// **The keyboard activation here is Space, not Enter, and that is not a
/// choice.** `Button` is a tab stop by default and gpui synthesizes a click from
/// either key on a focused element — but `Dialog` binds `enter` to
/// `ConfirmDialog` in its own key context, whose default `on_ok` returns `true`
/// and closes the card. That is how every control in this dialog has always
/// behaved, so it is not this page's to change; Space is the conventional
/// button key and it reaches the button. A disabled button is not a tab stop
/// (`Button::render` only calls `track_focus` when it is enabled), so Tab skips
/// the top row's Up and the bottom row's Down rather than landing on a control
/// that does nothing.
#[allow(clippy::too_many_arguments)]
fn move_button(
    layout: &WeakEntity<Layout>,
    code: &'static str,
    direction: &'static str,
    icon: AppIcon,
    label: Str,
    delta: isize,
    at_the_end: bool,
    cx: &App,
) -> Button {
    let layout = layout.clone();

    Button::new(SharedString::from(format!("feature-{direction}-{code}")))
        .ghost()
        .xsmall()
        .icon(icon)
        .tooltip(t(label, cx))
        .disabled(at_the_end)
        .on_click(move |_, _, cx| {
            _ = layout.update(cx, |pane, cx| pane.move_tool_by(code, delta, cx));
        })
}

/// The tool under the pointer during a drag, and the card that follows it.
///
/// gpui's drag-and-drop wants an entity to render the thing being dragged;
/// this is it. The card is deliberately the row's own label rather than a copy
/// of the row: a floating switch that cannot be pressed reads as a bug.
#[derive(Clone)]
struct DragTool {
    code: &'static str,
    title: SharedString,
}

impl Render for DragTool {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .id("drag-tool")
            .gap_2()
            .px_2()
            .py_1()
            .items_center()
            .overflow_hidden()
            .whitespace_nowrap()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .children(View::lookup(self.code).map(|view| view.icon().view()))
            .child(self.title.clone())
    }
}

/// The Quick navigation section: the master switch, then one pattern field per
/// detector, in detection order.
///
/// The pattern items are **generated from [`Detector::ORDER`]**, so a sixth
/// detector appears here with no edit to this file. Each one's description is
/// chosen by [`Detector::has_parser`], which is the "a pattern selects
/// candidates, the parser confirms" rule showing through to the user: a gate in
/// front of a real parser reads differently from a shape test, because it *is*
/// different.
fn quick_nav_page(highlight: Option<Setting>, cx: &App) -> SettingPage {
    let title = t(Str::QuickNavigation, cx);
    let lit = |setting: Setting| highlight == Some(setting);

    let mut group = SettingGroup::new().title(title.clone()).item(
        SettingItem::new(
            t(Str::QuickNavEnabled, cx),
            highlighted(
                SettingField::switch(
                    |cx: &App| QuickNav::enabled(cx),
                    |value: bool, cx: &mut App| QuickNav::set_enabled(value, cx),
                )
                .default_value(true),
                lit(Setting::QuickNavEnabled),
                cx,
            ),
        )
        .description(t(Str::QuickNavEnabledDescription, cx))
        .keywords([title.clone()]),
    );

    for detector in Detector::ORDER {
        // An unreadable pattern says so here, in place of the description: the
        // detector is meanwhile running on its built-in default, so the user
        // needs to know their pattern is not the thing being used.
        let description = match QuickNav::pattern_error(detector, cx) {
            Some(error) => t(error.message(), cx),
            None if detector.has_parser() => t(Str::QuickNavGateDescription, cx),
            None => t(Str::QuickNavShapeDescription, cx),
        };

        group = group.item(
            input_item(
                t(detector.label(), cx),
                highlighted(
                    pattern_field(detector),
                    lit(Setting::QuickNavPattern(detector)),
                    cx,
                ),
            )
            .description(description)
            .keywords([title.clone()]),
        );
    }

    // Only when there is something wrong with the file. In the ordinary case —
    // including a first run with no file at all — this section is not there.
    if let Some(problem) = QuickNav::store_error(cx) {
        group = group.item(
            // The message is the *description*; the control is empty because
            // there is nothing here to change. `SettingItem` requires a field,
            // so an empty render closure is the way to say "none".
            SettingItem::new(
                t(Str::QuickNavStorageProblem, cx),
                SettingField::render(|_, _, _| div()),
            )
            .description(t(problem, cx))
            .keywords([title.clone()]),
        );
    }

    SettingPage::new(title)
        .icon(AppIcon::Binary)
        .resettable(false)
        .group(group)
}

/// A setting item whose control is a text input — **use this rather than
/// [`SettingItem::new`] for every [`SettingField::input`]**, here or on a future
/// page.
///
/// The only difference is `layout(Axis::Vertical)`, and it is not cosmetic: an
/// input is the widest of the *fixed*-width controls the library builds, wide
/// enough that a side-by-side row here cannot hold it. [`pattern_field`] has the
/// numbers. Stacked, the control is `w_full` and therefore bounded by the row at
/// any dialog or window size, which is also what the library falls back to on
/// its own once the panel is narrow enough.
fn input_item(title: SharedString, field: SettingField<SharedString>) -> SettingItem {
    SettingItem::new(title, field).layout(Axis::Vertical)
}

/// One detector's pattern, as a plain text field.
///
/// The library's string field emits a change per keystroke; the coalescing that
/// keeps that from writing the file once per character lives in
/// [`QuickNav::set_pattern`], not here. The value is stored **raw** — untrimmed,
/// uncompiled — so the field never fights the user mid-edit.
///
/// **Its item is built by [`input_item`], which stacks it.**
/// `setting::fields::string::StringField::render` gives an input `w_64` (256px)
/// in a horizontal row, where a switch or a dropdown is content-sized and a
/// number input is half as wide. `SettingItem::render_item` caps the label
/// column at `max_w_3_5` and lets it grow into whatever the 256px control
/// leaves, but nothing shrinks the control, so a row narrower than
/// `256 + gap + 0.6 * row` cannot hold both and the input is laid out past the
/// row's right edge, where the dialog clips it. At [`DIALOG_WIDTH`] each row is 494px and the input
/// lands 70px outside — the regression the stacked layout fixes. The library
/// reaches for the same stacked layout by itself, but only once the whole panel
/// has dropped to 480px. `row_layout` measures both halves of that.
fn pattern_field(detector: Detector) -> SettingField<SharedString> {
    SettingField::input(
        move |cx: &App| QuickNav::pattern(detector, cx).into(),
        move |value: SharedString, cx: &mut App| QuickNav::set_pattern(detector, value, cx),
    )
    .default_value(SharedString::default())
}

/// Marks the field the search box jumped to. The style refines the field's own
/// control (the dropdown button), which is the thing the user came to change.
fn highlighted<T>(field: SettingField<T>, on: bool, cx: &App) -> SettingField<T> {
    if !on {
        return field;
    }

    field
        .border_color(cx.theme().primary)
        .bg(cx.theme().primary.opacity(0.1))
}

fn language_field() -> SettingField<SharedString> {
    let options = Language::ALL
        .map(|language| (language.code().into(), language.label().into()))
        .to_vec();

    SettingField::dropdown(
        options,
        |cx: &App| Language::current(cx).code().into(),
        |value: SharedString, cx: &mut App| {
            let language = Language::from_code(&value);
            language.set(cx);
            // Persisted here rather than inside `Language::set`: `i18n` is the
            // mechanism and has no business knowing dodo writes files.
            Session::set_language(language.code(), cx);
        },
    )
    .default_value(Language::default().code())
}

/// Whether the API Explorer runs a request's scripts.
///
/// **The one setting on this page that is not persisted**, now that
/// `session.json` keeps the rest: a fresh launch always asks about imported
/// scripts rather than running them. A security default that silently stopped
/// resetting is exactly the kind of change nobody notices until it matters, so
/// this stays deliberate rather than convenient. The *approvals* the prompt
/// collects are persisted separately, per script — see
/// `api_explorer::services::consent_store`.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn start_with_os_field() -> SettingField<bool> {
    SettingField::switch(
        |_: &App| crate::tray::startup::enabled(),
        |enabled: bool, cx: &mut App| {
            if let Err(error) = crate::tray::startup::set_enabled(enabled) {
                eprintln!("start with OS: {error}");
            }
            // The OS registration is the source of truth, so repaint from its
            // answer rather than retaining a second, potentially stale toggle.
            cx.refresh_windows();
        },
    )
    .default_value(false)
}

fn run_scripts_field(cx: &App) -> SettingField<SharedString> {
    let options = ConsentPolicy::ALL
        .map(|policy| (policy.code().into(), t(policy.label(), cx)))
        .to_vec();

    SettingField::dropdown(
        options,
        |cx: &App| ScriptPolicy::current(cx).code().into(),
        |value: SharedString, cx: &mut App| ScriptPolicy::set(ConsentPolicy::from_code(&value), cx),
    )
    .default_value(ConsentPolicy::default().code())
}

fn font_size_field(cx: &App) -> SettingField<SharedString> {
    let options = FONT_SIZES
        .map(|(label, size)| (size_value(size), t(label, cx)))
        .to_vec();

    SettingField::dropdown(
        options,
        |cx: &App| size_value(f32::from(Theme::global(cx).font_size)),
        |value: SharedString, cx: &mut App| {
            let size = value.parse().unwrap_or(DEFAULT_FONT_SIZE);
            set_font_size(size, cx);
            Session::set_font_size(size, cx);
        },
    )
    .default_value(size_value(DEFAULT_FONT_SIZE))
}

fn radius_field() -> SettingField<SharedString> {
    let options = RADII
        .map(|radius| (size_value(radius), format!("{radius}px").into()))
        .to_vec();

    SettingField::dropdown(
        options,
        |cx: &App| size_value(f32::from(Theme::global(cx).radius)),
        |value: SharedString, cx: &mut App| {
            let radius = value.parse().unwrap_or(DEFAULT_RADIUS);
            set_radius(radius, cx);
            Session::set_border_radius(radius, cx);
        },
    )
    .default_value(size_value(DEFAULT_RADIUS))
}

fn theme_field() -> SettingField<SharedString> {
    let options = THEMES
        .map(|name| {
            (
                SharedString::new_static(name),
                SharedString::new_static(name),
            )
        })
        .to_vec();

    SettingField::scrollable_dropdown(
        options,
        |cx: &App| Theme::global(cx).theme_name().clone(),
        |value: SharedString, cx: &mut App| {
            set_theme(&value, cx);
            Session::set_theme(value.to_string(), cx);
        },
    )
    .default_value(THEMES[0])
}

/// Applies the appearance choices `session.json` held, if any.
///
/// Called from `main` **after** `session::load` and **before** the window is
/// opened, so the first frame is already the user's theme rather than a flash
/// of the default one.
///
/// The order matters and is the same order the dialog produces by hand: the
/// theme first, because [`set_theme`] re-asserts the font size and radius that
/// were current over whatever the theme config brought with it, then the two
/// explicit values over that. Doing it the other way round would let a theme's
/// own numbers win over the user's.
///
/// A field the user never touched stays `None` and is not applied at all —
/// notably the theme, because `gpui_component::init` picks light or dark from
/// the *system appearance* and forcing "Default Light" over that merely because
/// it was what the app happened to show would break appearance following for
/// everyone who never opened this dialog. `session::models::document` argues it
/// at more length.
pub fn apply_session(cx: &mut App) {
    if let Some(name) = Session::theme(cx) {
        set_theme(&name, cx);
    }
    if let Some(size) = Session::font_size(cx) {
        set_font_size(size, cx);
    }
    if let Some(radius) = Session::border_radius(cx) {
        set_radius(radius, cx);
    }
    if let Some(code) = Session::language(cx) {
        Language::from_code(&code).set(cx);
    }
}

fn set_font_size(size: f32, cx: &mut App) {
    Theme::global_mut(cx).font_size = px(size);
    cx.refresh_windows();
}

/// `radius_lg` (dialogs, notifications) tracks `radius` so that picking 0px
/// squares off every corner rather than leaving overlays rounded.
fn set_radius(radius: f32, cx: &mut App) {
    let theme = Theme::global_mut(cx);
    theme.radius = px(radius);
    theme.radius_lg = px(radius);
    cx.refresh_windows();
}

/// Applies a registered theme by name, keeping the user's font size and radius.
///
/// **Does not persist**, unlike the three field writers above, because
/// [`apply_session`] calls it on the restore path too and a restore that wrote
/// back what it had just read would be noise. The dialog's own writer persists;
/// see [`theme_field`].
///
/// An unregistered name is a no-op, which is also what makes a `session.json`
/// naming a theme this build dropped harmless rather than fatal.
fn set_theme(name: &str, cx: &mut App) {
    let Some(config) = ThemeRegistry::global(cx).themes().get(name).cloned() else {
        eprintln!("theme {name} is not registered");
        return;
    };

    // A theme config may carry its own font size and radius. Ours are explicit
    // user choices, so re-assert them over whatever the theme brought with it.
    let font_size = f32::from(Theme::global(cx).font_size);
    let radius = f32::from(Theme::global(cx).radius);
    Theme::global_mut(cx).apply_config(&config);
    set_font_size(font_size, cx);
    set_radius(radius, cx);
}

/// Dropdown values are stable identifiers, never localized labels, so the
/// stored choice does not change meaning when the language does.
fn size_value(size: f32) -> SharedString {
    format!("{size}").into()
}

#[cfg(test)]
mod tests {
    // Deliberately not `use super::*`: that pulls in `use gpui::*`, whose `test`
    // re-export shadows the standard attribute. See the dodo-build-validate skill.
    use super::rank;

    fn labels() -> Vec<String> {
        [
            "Language General",
            "Font size Appearance",
            "Border radius Appearance",
            "Theme Appearance",
        ]
        .map(str::to_owned)
        .to_vec()
    }

    fn best(query: &str) -> Option<usize> {
        rank(query, &labels()).first().map(|(ix, _)| *ix)
    }

    #[test]
    fn abbreviations_find_their_setting() {
        assert_eq!(best("brdr"), Some(2));
        assert_eq!(best("fnt"), Some(1));
        assert_eq!(best("lang"), Some(0));
        assert_eq!(best("thm"), Some(3));
    }

    #[test]
    fn several_matches_come_back_best_first() {
        // "ea" is a subsequence of every label, so this exercises the ordering
        // rather than the filtering.
        let ranked = rank("ea", &labels());
        assert_eq!(ranked.len(), labels().len());
        assert!(
            ranked.windows(2).all(|pair| pair[0].1 >= pair[1].1),
            "ranked = {ranked:?}"
        );
        // The three Appearance labels contain "ea" in "Appearance"; the General
        // one only scatters it, so it has to come last.
        assert_eq!(ranked.last().map(|(ix, _)| *ix), Some(0));
    }

    #[test]
    fn a_section_name_lists_that_section() {
        let ranked = rank("appearance", &labels());
        let mut found: Vec<usize> = ranked.into_iter().map(|(ix, _)| ix).collect();
        found.sort();
        assert_eq!(found, vec![1, 2, 3]);
    }

    #[test]
    fn an_empty_query_matches_nothing() {
        assert!(rank("", &labels()).is_empty());
        assert!(rank("   ", &labels()).is_empty());
    }

    #[test]
    fn a_query_matching_nothing_returns_nothing() {
        assert!(rank("zzqx", &labels()).is_empty());
    }

    #[test]
    fn matching_is_case_insensitive() {
        assert_eq!(best("FONT"), Some(1));
        assert_eq!(best("BoRdEr"), Some(2));
    }

    #[test]
    fn vietnamese_labels_match_accented_and_plain_typing() {
        let vietnamese = ["Ngôn ngữ Chung".to_owned(), "Cỡ chữ Giao diện".to_owned()];
        let best = |query: &str| rank(query, &vietnamese).first().map(|(ix, _)| *ix);

        assert_eq!(best("cỡ chữ"), Some(1));
        assert_eq!(best("co chu"), Some(1));
        assert_eq!(best("ngon ngu"), Some(0));
    }

    #[test]
    fn folding_strips_accents_without_losing_letters() {
        assert_eq!(super::fold("Cỡ chữ"), "Co chu");
        assert_eq!(super::fold("Giao diện"), "Giao dien");
        assert_eq!(super::fold("Định dạng"), "Dinh dang");
        assert_eq!(super::fold("Border radius"), "Border radius");
    }
}

/// Measures a setting row against the box that has to contain it.
///
/// These are the only tests here that need a frame. They do not drive the
/// dialog — `Root::new` dereferences a real `NSView`, so a dialog cannot be
/// hosted in a GPUI test window on macOS — but the dialog contributes nothing
/// to a row's width except the box it hands the panel, so the panel is rendered
/// directly into a div of exactly that width ([`DIALOG_WIDTH`] less
/// `CARD_CHROME`) and the row is measured inside it.
///
/// The field is a stand-in rather than the real [`pattern_field`]: nothing can
/// tag a library-internal element for `debug_bounds`, so the probe reproduces
/// what `setting::fields::string::StringField::render` builds — `w_64` in a
/// horizontal row, `w_full` in a stacked one. Should upstream drop that fixed
/// width, [`a_side_by_side_row_would_not_fit`] fails and this whole workaround
/// can go.
#[cfg(test)]
mod row_layout {
    use gpui::prelude::FluentBuilder as _;
    use gpui::{
        AppContext as _, Axis, Bounds, Context, InteractiveElement as _, IntoElement,
        ParentElement as _, Pixels, Render, SharedString, StyleRefinement, Styled as _,
        TestAppContext, VisualTestContext, Window, WindowBounds, WindowOptions, div, point, px,
        size,
    };
    use gpui_component::setting::{SettingField, SettingGroup, SettingItem, SettingPage, Settings};

    use super::{DIALOG_WIDTH, SIDEBAR_WIDTH};

    /// What the dialog card keeps for itself before the panel sees any width:
    /// a 1px border and `Dialog`'s default `Edges::all(16)` padding, per side.
    const CARD_CHROME: Pixels = px(34.);

    /// The settings panel, sized and configured exactly as the dialog does it,
    /// holding one item that stands in for a quick-navigation pattern row.
    ///
    /// `stacked` picks how that item is built: through [`super::input_item`],
    /// which is the production path, or through the bare [`SettingItem::new`]
    /// it replaced.
    struct Probe {
        width: Pixels,
        stacked: bool,
    }

    impl Render for Probe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let title = SharedString::from("Database URI pattern");
            let field = SettingField::render(|options, _, _| {
                let horizontal = matches!(options.layout, Axis::Horizontal);
                div()
                    .debug_selector(|| "field".into())
                    .h(px(32.))
                    .map(|this| {
                        if horizontal {
                            this.w_64()
                        } else {
                            this.w_full()
                        }
                    })
            });

            let item = if self.stacked {
                super::input_item(title, field)
            } else {
                SettingItem::new(title, field)
            };

            let page = SettingPage::new("Quick navigation").resettable(false).group(
                SettingGroup::new().title("Quick navigation").item(
                    item
                    // The longest of the three descriptions these rows carry:
                    // the label column's width is what the control has to fit
                    // beside, so a short one would understate the row.
                    .description(
                        "Optional. dodo already has a real parser for this format and uses it; a \
                         pattern here only narrows what is offered to it. Leave it empty to try \
                         the parser on everything.",
                    ),
                ),
            );

            div()
                .w(self.width)
                .h(px(440.))
                .debug_selector(|| "panel".into())
                .child(
                    Settings::new("row-layout-probe")
                        .sidebar_width(SIDEBAR_WIDTH)
                        .header_style(&StyleRefinement::default().hidden())
                        .pages(vec![page]),
                )
        }
    }

    /// Right edge of the row's control, and of the box that must contain it.
    fn edges(cx: &mut TestAppContext, width: Pixels, stacked: bool) -> (Pixels, Pixels) {
        cx.update(gpui_component::init);

        let window = cx
            .update(|cx| {
                cx.open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds {
                            origin: point(px(0.), px(0.)),
                            size: size(px(1200.), px(800.)),
                        })),
                        ..Default::default()
                    },
                    |_, cx| cx.new(|_| Probe { width, stacked }),
                )
            })
            .unwrap();
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.run_until_parked();

        let field = cx.debug_bounds("field").expect("the probe row was drawn");
        let panel = cx.debug_bounds("panel").expect("the probe panel was drawn");
        (field.right(), panel.right())
    }

    /// The panel width the dialog actually hands the settings panel, plus what
    /// it would have at the narrowest the window itself can be dragged. The
    /// dialog does not resize with the window, so the second is hypothetical
    /// today — but measuring both says the row is bounded by its own box rather
    /// than by luck about how much room happens to be there.
    fn widths() -> [Pixels; 2] {
        [
            DIALOG_WIDTH - CARD_CHROME,
            crate::layout::window_min_size().width - CARD_CHROME,
        ]
    }

    #[gpui::test]
    fn a_pattern_row_stays_inside_the_card(cx: &mut TestAppContext) {
        for width in widths() {
            let (field, panel) = edges(cx, width, true);
            assert!(
                field <= panel,
                "at a {width:?} panel the stacked control reaches {field:?}, past {panel:?}"
            );
        }
    }

    /// Why [`super::input_item`] exists. Not a wish — if this ever stops
    /// overflowing, the stacked layout is no longer load-bearing and the row can
    /// go back to sitting beside its label.
    #[gpui::test]
    fn a_side_by_side_row_would_not_fit(cx: &mut TestAppContext) {
        let width = DIALOG_WIDTH - CARD_CHROME;
        let (field, panel) = edges(cx, width, false);
        assert!(
            field > panel,
            "a horizontal input row now fits ({field:?} within {panel:?}); \
             super::input_item's stacked layout may no longer be needed"
        );
    }
}
