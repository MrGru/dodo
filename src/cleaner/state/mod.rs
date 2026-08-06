mod cleaner_state;
mod mock;

pub use cleaner_state::{CleanerState, CleanerStatus};
pub use mock::default_scanners;
#[cfg(test)]
pub use mock::MockScanner;
