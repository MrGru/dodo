//! The Settings dialog, plus the app-level state it edits.
//!
//! There is deliberately no settings struct of our own for appearance: font
//! size, border radius and colours all live on `gpui_component::Theme`, which
//! is already a global the whole app renders from, so the dialog reads and
//! writes that directly and every change is live. Language is the one setting
//! with no home in `Theme`; it lives in [`crate::i18n::Language`].
//!
//! Nothing is persisted across restarts — see the note in `AGENTS.md`.
//!
//! The dialog body is [`SettingsView`]: a quick-navigation search box above the
//! library's own settings panel. Typing fuzzy-matches every setting and picking
//! a result jumps to it.

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::list::{List, ListDelegate, ListItem, ListState};
use gpui_component::setting::{
    SelectIndex, SettingField, SettingGroup, SettingItem, SettingPage, Settings,
};
use gpui_component::{
    ActiveTheme as _, IndexPath, Theme, ThemeRegistry, WindowExt as _, h_flex, v_flex,
};
use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use unicode_normalization::UnicodeNormalization as _;
use unicode_normalization::char::is_combining_mark;

use crate::app_icon::AppIcon;
use crate::assets::Assets;
use crate::i18n::{Language, Str, t};

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

/// Opens the Settings dialog. The dialog is dismissed with Escape, the close
/// button, or a click on the overlay.
pub fn open(window: &mut Window, cx: &mut App) {
    let view = cx.new(|cx| SettingsView::new(window, cx));

    window.open_dialog(cx, move |dialog, _, cx| {
        dialog
            .title(t(Str::Settings, cx))
            .w(px(760.))
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
    FontSize,
    BorderRadius,
    Theme,
}

impl Setting {
    const ALL: [Setting; 4] = [
        Setting::Language,
        Setting::FontSize,
        Setting::BorderRadius,
        Setting::Theme,
    ];

    /// Index into the vec [`pages`] returns — the sidebar entry to open.
    fn page_ix(self) -> usize {
        match self {
            Setting::Language => 0,
            Setting::FontSize | Setting::BorderRadius | Setting::Theme => 1,
        }
    }

    fn label(self) -> Str {
        match self {
            Setting::Language => Str::Language,
            Setting::FontSize => Str::FontSize,
            Setting::BorderRadius => Str::BorderRadius,
            Setting::Theme => Str::Theme,
        }
    }

    /// The section heading the setting sits under, shown beside every result so
    /// the user knows where the jump will land.
    fn section(self) -> Str {
        match self {
            Setting::Language => Str::General,
            Setting::FontSize | Setting::BorderRadius | Setting::Theme => Str::Appearance,
        }
    }
}

/// The searchable text of every setting, in [`Setting::ALL`] order.
///
/// Both the label and the section name go in, so that typing a section name
/// lists that section's settings, exactly as the item keywords used to do for
/// the library's own search box.
fn haystacks(cx: &App) -> Vec<String> {
    Setting::ALL
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
        self.matches = rank(&self.query, &haystacks(cx))
            .into_iter()
            .map(|(ix, _)| Setting::ALL[ix])
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
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
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
                        .sidebar_width(px(200.))
                        .header_style(&StyleRefinement::default().hidden())
                        .default_selected_index(SelectIndex {
                            page_ix: self.page_ix,
                            group_ix: None,
                        })
                        .pages(pages(self.highlight, cx)),
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
fn pages(highlight: Option<Setting>, cx: &App) -> Vec<SettingPage> {
    let general = t(Str::General, cx);
    let appearance = t(Str::Appearance, cx);
    let lit = |setting: Setting| highlight == Some(setting);

    vec![
        SettingPage::new(general.clone())
            .icon(AppIcon::Sliders)
            .resettable(false)
            .default_open(true)
            .group(
                SettingGroup::new().title(general.clone()).item(
                    SettingItem::new(
                        t(Str::Language, cx),
                        highlighted(language_field(), lit(Setting::Language), cx),
                    )
                    .description(t(Str::LanguageDescription, cx))
                    .keywords([general]),
                ),
            ),
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
    ]
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
        |value: SharedString, cx: &mut App| Language::from_code(&value).set(cx),
    )
    .default_value(Language::default().code())
}

fn font_size_field(cx: &App) -> SettingField<SharedString> {
    let options = FONT_SIZES
        .map(|(label, size)| (size_value(size), t(label, cx)))
        .to_vec();

    SettingField::dropdown(
        options,
        |cx: &App| size_value(f32::from(Theme::global(cx).font_size)),
        |value: SharedString, cx: &mut App| {
            set_font_size(value.parse().unwrap_or(DEFAULT_FONT_SIZE), cx)
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
        |value: SharedString, cx: &mut App| set_radius(value.parse().unwrap_or(DEFAULT_RADIUS), cx),
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
        |value: SharedString, cx: &mut App| set_theme(&value, cx),
    )
    .default_value(THEMES[0])
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
