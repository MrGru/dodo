use crate::cleaner::core::item::CleanableItem;
use crate::cleaner::core::risk::SelectionPolicy;

/// Whether a scan result starts ticked. Round 1's UI lists results without
/// selection controls, so nothing calls this yet; the allow comes off when the
/// result list grows checkboxes.
#[allow(dead_code)]
pub fn is_selected_by_default(item: &CleanableItem) -> bool {
    matches!(item.selection_policy, SelectionPolicy::SelectedByDefault)
}
