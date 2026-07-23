//! Layout state: panel sizes, collapse flags, and which tab is in front.
//!
//! Kept apart from the request and response state so that resizing a panel
//! cannot invalidate a response, and so that the split geometry is owned in one
//! place rather than being read off whatever view happens to render it.

use gpui::{AppContext as _, Context, Entity, Pixels, px};
use gpui_component::resizable::ResizableState;

/// Default width of the Collections panel.
///
/// Narrower than the reference's, because dodo's window opens at 900px wide and
/// the request bar has to stay usable in it. The panel is resizable and
/// collapsible, so the reference's proportions are one drag away.
pub const COLLECTIONS_WIDTH: Pixels = px(185.);

/// Default height of the response viewer below the request editor.
///
/// The *response* pane is the sized one and the request pane is the one that
/// grows to fill the rest — deliberately, because the request is what is being
/// edited before any response exists, so it should get the larger share of a
/// short window rather than a fixed stub. A response arriving later does not
/// change either panel's geometry (the split is content-independent), so the
/// proportions never get yanked out from under a drag.
pub const RESPONSE_HEIGHT: Pixels = px(240.);

/// Smallest the request editor is allowed to shrink to when the response pane
/// is dragged up over it.
pub const REQUEST_MIN_HEIGHT: Pixels = px(160.);

/// Which view the left panel is showing. Selected from the far-left rail; the
/// panel body swaps between the Collections tree and the request History.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum LeftPanel {
    #[default]
    Collections,
    History,
}

pub struct UiState {
    /// left panel | (request over response). Held as an entity so the library's
    /// drag handling writes back into it, which is what makes a dragged size
    /// persist for the session.
    pub outer_split: Entity<ResizableState>,
    /// Request editor over response viewer.
    pub inner_split: Entity<ResizableState>,
    /// Which view the left panel shows.
    pub left_panel: LeftPanel,
    /// Whether the left panel is collapsed to just its rail.
    pub panel_collapsed: bool,
    /// Index into the open tabs. Kept valid by [`UiState::clamp_active`].
    pub active_tab: usize,
}

impl UiState {
    pub fn new<T: 'static>(cx: &mut Context<T>) -> Self {
        Self {
            outer_split: cx.new(|_| ResizableState::default()),
            inner_split: cx.new(|_| ResizableState::default()),
            left_panel: LeftPanel::default(),
            panel_collapsed: false,
            active_tab: 0,
        }
    }

    /// Keeps the active index inside the open tabs after one is closed.
    ///
    /// Closing the tab in front selects the one that took its place, or the new
    /// last tab if it was the final one — the behaviour every tabbed editor
    /// has, and the reason this is not just a `min`.
    pub fn clamp_active(&mut self, tab_count: usize) {
        if tab_count == 0 {
            self.active_tab = 0;
        } else if self.active_tab >= tab_count {
            self.active_tab = tab_count - 1;
        }
    }
}

#[cfg(test)]
mod tests {
    /// `UiState` itself needs a `Context` to build, so the index arithmetic is
    /// tested through the same logic in a free function rather than not at all.
    fn clamp(active: usize, tab_count: usize) -> usize {
        if tab_count == 0 {
            0
        } else if active >= tab_count {
            tab_count - 1
        } else {
            active
        }
    }

    #[test]
    fn closing_a_middle_tab_keeps_the_index() {
        assert_eq!(clamp(1, 3), 1);
    }

    #[test]
    fn closing_the_last_tab_steps_back() {
        assert_eq!(clamp(2, 2), 1);
    }

    #[test]
    fn closing_the_only_tab_lands_on_zero() {
        assert_eq!(clamp(0, 0), 0);
    }
}
