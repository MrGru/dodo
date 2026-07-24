//! The load state of a detail surface — the Inspect panel and the Logs viewer.
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
//! Plain data, no GPUI: the panel's tasks live in
//! [`views::detail`](crate::docker::views::detail).

use crate::i18n::Str;

/// Where a detail surface's one fetch has got to.
pub enum DetailStatus<T> {
    /// The fetch is in flight and there is nothing to show yet.
    Loading,
    Ready(T),
    /// The fetch failed. The panel stays open showing this, with a Retry.
    Failed(Str),
}

impl<T> DetailStatus<T> {
    pub fn is_loading(&self) -> bool {
        matches!(self, DetailStatus::Loading)
    }

    /// The loaded content, or `None` while loading or after a failure.
    pub fn ready(&self) -> Option<&T> {
        match self {
            DetailStatus::Ready(content) => Some(content),
            _ => None,
        }
    }

    /// The failure message, or `None` when there is not one.
    pub fn error(&self) -> Option<&Str> {
        match self {
            DetailStatus::Failed(message) => Some(message),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DetailStatus;
    use crate::i18n::Str;

    #[test]
    fn each_state_exposes_only_its_own_content() {
        let loading: DetailStatus<u8> = DetailStatus::Loading;
        assert!(loading.is_loading());
        assert!(loading.ready().is_none());
        assert!(loading.error().is_none());

        let ready = DetailStatus::Ready(7u8);
        assert!(!ready.is_loading());
        assert_eq!(ready.ready(), Some(&7));
        assert!(ready.error().is_none());

        let failed: DetailStatus<u8> =
            DetailStatus::Failed(Str::DockerOperationError("no such container".into()));
        assert!(!failed.is_loading());
        assert!(failed.ready().is_none());
        assert!(failed.error().is_some());
    }
}
