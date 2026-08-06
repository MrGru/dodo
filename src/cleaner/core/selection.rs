use std::collections::HashSet;

use crate::cleaner::core::item::CleanableItem;
use crate::cleaner::core::item::CleanableItemId;
use crate::cleaner::core::risk::SelectionPolicy;

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
