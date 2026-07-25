//! The load state of a detail surface — the Inspect panel and the Logs viewer —
//! and the tab model the two share inside one dialog.
//!
//! The list pages already have [`LoadStatus`](super::containers::LoadStatus),
//! but it is the *table's* status: it keeps the previous rows visible while a
//! refresh runs, because blanking a populated table on every poll would be
//! hostile. A detail panel has the opposite requirement — it is opened for one
//! resource, so a manual refresh should say so, and a failure has nothing to fall
//! back to. Hence a second, smaller status, generic over what a ready panel
//! holds ([`InspectDetail`](crate::docker::models::inspect::InspectDetail) or a
//! [`Vec<LogLine>`](crate::docker::models::logs::LogLine)).
//!
//! [`DetailTabs`] is the other half: two independently-loaded slots and which of
//! them is showing. Keeping it here — plain data, no GPUI — is what makes the
//! rule that matters testable without a daemon or a window: *switching tabs
//! fetches only what has never been fetched*, so inspecting a container never
//! pulls its logs and going back to a tab already loaded hits nothing.
//!
//! The dialog itself, its tasks and its rendering live in
//! [`views::detail`](crate::docker::views::detail).

use crate::docker::models::inspect::InspectKind;
use crate::i18n::Str;

/// Where a detail surface's one fetch has got to.
pub enum DetailStatus<T> {
    /// The fetch is in flight and there is nothing to show yet.
    Loading,
    Ready(T),
    /// The fetch failed. The panel stays open showing this, with a Retry.
    Failed(Str),
}

/// Which surface of a detail dialog is showing.
///
/// A container has both; the other three resource types have Inspect alone, and
/// [`DetailTab::all_for`] is the single place that says so.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DetailTab {
    Inspect,
    Logs,
}

impl DetailTab {
    /// The tab a freshly opened dialog shows. Inspect, because it is the one
    /// surface every resource type has and the cheaper of the two: a container's
    /// logs are only fetched if the user actually asks for them.
    pub const DEFAULT: DetailTab = DetailTab::Inspect;

    /// The tabs a dialog on `kind` offers, in strip order. Only a container has
    /// a second one — the other three return a single tab, which the view draws
    /// as *no* strip at all rather than a one-tab strip that reads as a mistake.
    pub fn all_for(kind: InspectKind) -> &'static [DetailTab] {
        match kind {
            InspectKind::Container => &[DetailTab::Inspect, DetailTab::Logs],
            InspectKind::Image | InspectKind::Volume | InspectKind::Network => {
                &[DetailTab::Inspect]
            }
        }
    }

    /// The tab's label. Both strings predate the tabs — they were the two
    /// separately-opened overlays' titles — so nothing new is translated here.
    pub fn label(self) -> Str {
        match self {
            DetailTab::Inspect => Str::DockerInspect,
            DetailTab::Logs => Str::DockerViewLogs,
        }
    }
}

/// A detail dialog's tabs: which one is showing, and what each one has fetched.
///
/// A slot is `None` until that tab has been opened for the first time, which is
/// the whole mechanism behind on-demand loading: [`DetailTabs::activate`] reports
/// whether the tab it just switched to still needs a fetch, and answers `false`
/// for one that is already loaded, loading, or failed. A failed tab is therefore
/// retried by [`DetailTabs::begin_load`] (the dialog's Refresh) rather than by
/// switching away and back, which would otherwise re-fetch on every flick
/// between tabs.
pub struct DetailTabs<I, L> {
    active: DetailTab,
    inspect: Option<DetailStatus<I>>,
    logs: Option<DetailStatus<L>>,
}

impl<I, L> DetailTabs<I, L> {
    /// A dialog opening on `active`, with nothing fetched yet.
    pub fn new(active: DetailTab) -> Self {
        Self {
            active,
            inspect: None,
            logs: None,
        }
    }

    pub fn active(&self) -> DetailTab {
        self.active
    }

    /// Whether `tab` has ever been fetched — loaded, loading or failed.
    pub fn is_loaded(&self, tab: DetailTab) -> bool {
        match tab {
            DetailTab::Inspect => self.inspect.is_some(),
            DetailTab::Logs => self.logs.is_some(),
        }
    }

    /// Switches to `tab` and reports whether it needs a fetch, which is true
    /// only the first time that tab is shown.
    pub fn activate(&mut self, tab: DetailTab) -> bool {
        self.active = tab;
        !self.is_loaded(tab)
    }

    /// Puts one tab into its loading state, discarding whatever it held. This is
    /// both the first fetch and the Refresh/Retry, and it touches only that tab
    /// so refreshing Inspect never throws away logs already on screen.
    pub fn begin_load(&mut self, tab: DetailTab) {
        match tab {
            DetailTab::Inspect => self.inspect = Some(DetailStatus::Loading),
            DetailTab::Logs => self.logs = Some(DetailStatus::Loading),
        }
    }

    pub fn set_inspect(&mut self, status: DetailStatus<I>) {
        self.inspect = Some(status);
    }

    pub fn set_logs(&mut self, status: DetailStatus<L>) {
        self.logs = Some(status);
    }

    /// The Inspect tab's status, or `None` while it has never been opened.
    pub fn inspect(&self) -> Option<&DetailStatus<I>> {
        self.inspect.as_ref()
    }

    /// The Logs tab's status, or `None` while it has never been opened.
    pub fn logs(&self) -> Option<&DetailStatus<L>> {
        self.logs.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::{DetailStatus, DetailTab, DetailTabs};
    use crate::docker::models::inspect::InspectKind;
    use crate::i18n::Str;

    /// The concrete instantiation the tab tests use: the payload types do not
    /// matter to the switching rules, only whether a slot is filled.
    fn tabs() -> DetailTabs<&'static str, Vec<&'static str>> {
        DetailTabs::new(DetailTab::DEFAULT)
    }

    fn is_loading<T>(status: Option<&DetailStatus<T>>) -> bool {
        matches!(status, Some(DetailStatus::Loading))
    }

    fn is_failed<T>(status: Option<&DetailStatus<T>>) -> bool {
        matches!(status, Some(DetailStatus::Failed(_)))
    }

    fn ready<T>(status: Option<&DetailStatus<T>>) -> Option<&T> {
        match status {
            Some(DetailStatus::Ready(content)) => Some(content),
            _ => None,
        }
    }

    #[test]
    fn a_dialog_opens_on_inspect_with_nothing_fetched() {
        let tabs = tabs();
        assert_eq!(DetailTab::DEFAULT, DetailTab::Inspect);
        assert_eq!(tabs.active(), DetailTab::Inspect);
        // Crucially, opening Inspect has not touched the Logs slot: a container
        // the user merely inspects never has its logs fetched.
        assert!(!tabs.is_loaded(DetailTab::Inspect));
        assert!(!tabs.is_loaded(DetailTab::Logs));
        assert!(tabs.inspect().is_none());
        assert!(tabs.logs().is_none());
    }

    #[test]
    fn a_dialog_can_open_straight_on_logs() {
        // The container context menu's "View Logs" opens the same dialog with
        // the Logs tab active.
        let tabs: DetailTabs<&str, Vec<&str>> = DetailTabs::new(DetailTab::Logs);
        assert_eq!(tabs.active(), DetailTab::Logs);
        assert!(!tabs.is_loaded(DetailTab::Inspect));
    }

    #[test]
    fn switching_fetches_once_and_never_again() {
        let mut tabs = tabs();

        // First switch to Logs: it has never been opened, so it needs a fetch.
        assert!(tabs.activate(DetailTab::Logs));
        tabs.begin_load(DetailTab::Logs);
        tabs.set_logs(DetailStatus::Ready(vec!["line"]));

        // Inspect has still never been opened, so it does need one.
        assert!(tabs.activate(DetailTab::Inspect));
        tabs.begin_load(DetailTab::Inspect);
        tabs.set_inspect(DetailStatus::Ready("{}"));

        // From here on, switching between them refetches nothing at all.
        assert!(!tabs.activate(DetailTab::Logs));
        assert!(!tabs.activate(DetailTab::Inspect));
        assert!(!tabs.activate(DetailTab::Logs));
        assert_eq!(tabs.active(), DetailTab::Logs);
        assert_eq!(ready(tabs.logs()), Some(&vec!["line"]));
        assert_eq!(ready(tabs.inspect()), Some(&"{}"));
    }

    #[test]
    fn a_tab_still_in_flight_is_not_fetched_twice() {
        let mut tabs = tabs();
        tabs.begin_load(DetailTab::Inspect);
        assert!(is_loading(tabs.inspect()));

        // Flicking to Logs and back must not start a second Inspect fetch.
        assert!(tabs.activate(DetailTab::Logs));
        assert!(!tabs.activate(DetailTab::Inspect));
    }

    #[test]
    fn a_failed_tab_is_retried_by_refresh_not_by_switching() {
        let mut tabs = tabs();
        tabs.begin_load(DetailTab::Inspect);
        tabs.set_inspect(DetailStatus::Failed(Str::DockerOperationError(
            "nope".into(),
        )));

        // Switching away and back does not silently retry.
        assert!(tabs.activate(DetailTab::Logs));
        assert!(!tabs.activate(DetailTab::Inspect));
        assert!(is_failed(tabs.inspect()));

        // Refresh does.
        tabs.begin_load(DetailTab::Inspect);
        assert!(is_loading(tabs.inspect()));
    }

    #[test]
    fn refreshing_one_tab_leaves_the_other_alone() {
        let mut tabs = tabs();
        tabs.set_inspect(DetailStatus::Ready("{}"));
        tabs.set_logs(DetailStatus::Ready(vec!["line"]));

        tabs.begin_load(DetailTab::Inspect);
        assert!(is_loading(tabs.inspect()));
        assert_eq!(ready(tabs.logs()), Some(&vec!["line"]));
    }

    #[test]
    fn only_a_container_gets_a_tab_strip() {
        assert_eq!(
            DetailTab::all_for(InspectKind::Container),
            &[DetailTab::Inspect, DetailTab::Logs]
        );
        for kind in [
            InspectKind::Image,
            InspectKind::Volume,
            InspectKind::Network,
        ] {
            assert_eq!(
                DetailTab::all_for(kind),
                &[DetailTab::Inspect],
                "{kind:?} has only Inspect, so the view draws no strip"
            );
        }
    }

    #[test]
    fn each_tab_labels_itself_with_an_existing_string() {
        use std::mem::discriminant;
        assert_eq!(
            discriminant(&DetailTab::Inspect.label()),
            discriminant(&Str::DockerInspect)
        );
        assert_eq!(
            discriminant(&DetailTab::Logs.label()),
            discriminant(&Str::DockerViewLogs)
        );
    }
}
