use std::collections::HashSet;

use crate::core::item::CleanableItem;
use crate::core::item::CleanableItemId;
use crate::core::risk::SelectionPolicy;

/// Whether a scan result starts ticked. Round 1's UI lists results without
/// selection controls, so nothing calls this yet; the allow comes off when the
/// result list grows checkboxes.
#[allow(dead_code)]
pub fn is_selected_by_default(item: &CleanableItem) -> bool {
    matches!(item.selection_policy, SelectionPolicy::SelectedByDefault)
}

pub fn selected_by_default_ids(items: &[CleanableItem]) -> HashSet<CleanableItemId> {
    items
        .iter()
        .filter(|item| is_selected_by_default(item))
        .map(|item| item.id)
        .collect()
}
