//! The menu bar / notification area item.
//!
//! `en` and `vi` each render every variant below; the compiler names any
//! string a language has not been given.

pub(crate) mod en;
pub(crate) mod vi;

#[cfg(test)]
pub(crate) mod samples;

/// The strings this area owns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Text {
    // The macOS menu bar item.
    //
    // The **input languages themselves are not here**: their names are endonyms
    // shown in their own language, so they never enter this mechanism. See
    // `tray::menu::label`.
    OpenDodo,
    KeyboardInput,
    QuitDodo,
}
