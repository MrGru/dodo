mod cleaner_state;
mod mock;

pub use cleaner_state::{CleanerState, CleanerStatus};
#[cfg(test)]
pub use mock::MockScanner;
pub use mock::default_scanners;
