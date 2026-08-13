mod cleaner_view;
pub mod results_layout;
mod results_sync;
mod results_table;
#[cfg(target_os = "macos")]
mod uninstall_review_dialog;

pub use cleaner_view::CleanerView;
