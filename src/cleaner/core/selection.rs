use crate::cleaner::core::item::CleanableItem;
use crate::cleaner::core::risk::SelectionPolicy;

pub fn is_selected_by_default(item: &CleanableItem) -> bool {
    matches!(item.selection_policy, SelectionPolicy::SelectedByDefault)
}
