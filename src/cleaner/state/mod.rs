mod cleaner_state;
// Round-1 scaffolding: every category now has a real scanner
// (`macos::scanners::default_scanners` etc.), so nothing outside `mock`'s own
// unit tests constructs a `MockScanner` any more — hence the whole module is
// `cfg(test)` rather than a per-item `#[allow(dead_code)]`.
#[cfg(test)]
mod mock;
mod registry;

pub use cleaner_state::{CategoryState, CleanerState};
#[cfg(test)]
pub use mock::MockScanner;
pub use registry::default_scanners;
