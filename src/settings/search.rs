use gpui::*;
use gpui_component::list::{ListDelegate, ListItem, ListState};
use gpui_component::{ActiveTheme as _, IndexPath, h_flex};
use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use unicode_normalization::UnicodeNormalization as _;
use unicode_normalization::char::is_combining_mark;

use super::view::SettingsView;
use crate::i18n::{Str, shell, t};
use crate::quick_nav::models::detect::Detector;

/// Every setting the search box can navigate to.
///
/// Written out by hand rather than derived from [`super::pages::pages`], because a
/// [`SettingItem`] exposes neither its title nor the page it ended up on. This
/// is the one list that has to be kept in step with `pages` by eye.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Setting {
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

    /// Index into the vec [`super::pages::pages`] returns — the sidebar entry to open.
    pub(super) fn page_ix(self) -> usize {
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
            Setting::Language => shell::Text::Language.into(),
            Setting::RunScripts => shell::Text::RunScripts.into(),
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            Setting::StartWithOs => shell::Text::StartWithOs.into(),
            Setting::FontSize => shell::Text::FontSize.into(),
            Setting::BorderRadius => shell::Text::BorderRadius.into(),
            Setting::Theme => shell::Text::Theme.into(),
            Setting::QuickNavEnabled => shell::Text::QuickNavEnabled.into(),
            Setting::QuickNavPattern(detector) => detector.label(),
            Setting::Features => shell::Text::Features.into(),
        }
    }

    /// The section heading the setting sits under, shown beside every result so
    /// the user knows where the jump will land.
    fn section(self) -> Str {
        match self {
            Setting::Language | Setting::RunScripts => shell::Text::General.into(),
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            Setting::StartWithOs => shell::Text::General.into(),
            Setting::FontSize | Setting::BorderRadius | Setting::Theme => {
                shell::Text::Appearance.into()
            }
            Setting::QuickNavEnabled | Setting::QuickNavPattern(_) => {
                shell::Text::QuickNavigation.into()
            }
            Setting::Features => shell::Text::Features.into(),
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
pub(super) fn fold(text: &str) -> String {
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
pub(super) fn rank(query: &str, haystacks: &[String]) -> Vec<(usize, u32)> {
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
pub(super) struct SearchDelegate {
    pub(super) view: WeakEntity<SettingsView>,
    /// Kept here because [`ListState`] does not expose its query input, and the
    /// panel's height depends on whether the user has typed anything.
    pub(super) query: SharedString,
    pub(super) matches: Vec<Setting>,
    pub(super) selected: Option<IndexPath>,
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
            .child(t(shell::Text::NoSettingsMatch, cx))
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
