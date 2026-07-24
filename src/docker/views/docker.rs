//! The Docker module's top view: it owns the four pages and shows the selected
//! one.
//!
//! The sidebar's four Docker children switch [`DockerPage`] through
//! [`DockerView::set_page`]; the entity and its sub-views are built once and
//! kept, so navigating between pages — and away to another tool and back —
//! preserves each page's state, the same lifetime rule `Layout` follows for the
//! top-level tools. Rounds 1–3 implement all four pages; each is loaded lazily
//! the first time it is shown.

use gpui::{AppContext as _, Context, Entity, IntoElement, Render, Window};

use crate::docker::views::containers::ContainersView;
use crate::docker::views::images::ImagesView;
use crate::docker::views::networks::NetworksView;
use crate::docker::views::volumes::VolumesView;

/// Which Docker page is showing. The discriminants line up with the four
/// sidebar children.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DockerPage {
    Containers,
    Images,
    Volumes,
    Networks,
}

pub struct DockerView {
    page: DockerPage,
    /// Whether the Docker section is the view the window is currently showing.
    /// Drives background polling: only the active page of a visible section polls
    /// (see [`should_poll`]), so navigating away to another tool stops the engine
    /// chatter, and returning resumes it.
    section_active: bool,
    /// Each page is built once and kept so its rows, search and (Containers')
    /// selection survive navigation between pages and tools.
    containers: Entity<ContainersView>,
    images: Entity<ImagesView>,
    volumes: Entity<VolumesView>,
    networks: Entity<NetworksView>,
}

impl DockerView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            page: DockerPage::Containers,
            // The window opens on another tool, so the Docker section is inactive
            // until the sidebar selects one of its pages.
            section_active: false,
            containers: cx.new(|cx| ContainersView::new(window, cx)),
            images: cx.new(|cx| ImagesView::new(window, cx)),
            volumes: cx.new(|cx| VolumesView::new(window, cx)),
            networks: cx.new(|cx| NetworksView::new(window, cx)),
        }
    }

    /// Shows `page` and triggers its first load. Selecting a Docker page always
    /// makes the section active, so polling starts (or moves to the new page).
    /// Each load is lazy so the engine is not touched until the page is actually
    /// opened, and idempotent so returning to a page does not reload it.
    pub fn set_page(&mut self, page: DockerPage, cx: &mut Context<Self>) {
        self.page = page;
        self.section_active = true;
        match page {
            DockerPage::Containers => {
                self.containers
                    .update(cx, |view, cx| view.ensure_loaded(cx));
            }
            DockerPage::Images => {
                self.images.update(cx, |view, cx| view.ensure_loaded(cx));
            }
            DockerPage::Volumes => {
                self.volumes.update(cx, |view, cx| view.ensure_loaded(cx));
            }
            DockerPage::Networks => {
                self.networks.update(cx, |view, cx| view.ensure_loaded(cx));
            }
        }
        self.sync_polling(cx);
        cx.notify();
    }

    /// Tells the section whether it is the visible view. The sidebar calls this
    /// with `false` when the user leaves for another tool, pausing all polling,
    /// and it resumes through [`set_page`] on return.
    pub fn set_section_active(&mut self, active: bool, cx: &mut Context<Self>) {
        if self.section_active == active {
            return;
        }
        self.section_active = active;
        self.sync_polling(cx);
    }

    /// Points each page's background poll at whether it should be running: exactly
    /// the one active, visible page polls; every other page stops. Idempotent, so
    /// it is safe to call on every page switch and active-state change.
    fn sync_polling(&mut self, cx: &mut Context<Self>) {
        let active = self.section_active;
        let page = self.page;
        self.containers.update(cx, |view, cx| {
            view.set_polling(should_poll(active, page, DockerPage::Containers), cx)
        });
        self.images.update(cx, |view, cx| {
            view.set_polling(should_poll(active, page, DockerPage::Images), cx)
        });
        self.volumes.update(cx, |view, cx| {
            view.set_polling(should_poll(active, page, DockerPage::Volumes), cx)
        });
        self.networks.update(cx, |view, cx| {
            view.set_polling(should_poll(active, page, DockerPage::Networks), cx)
        });
    }
}

/// Whether a given `page` should be polling, given whether the Docker section is
/// the visible view and which page is active. Only the active page of a visible
/// section polls — so at most one page ever hits the engine in the background,
/// and none does while the user is in another tool.
pub fn should_poll(section_active: bool, active_page: DockerPage, page: DockerPage) -> bool {
    section_active && active_page == page
}

impl Render for DockerView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        match self.page {
            DockerPage::Containers => self.containers.clone().into_any_element(),
            DockerPage::Images => self.images.clone().into_any_element(),
            DockerPage::Volumes => self.volumes.clone().into_any_element(),
            DockerPage::Networks => self.networks.clone().into_any_element(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DockerPage, should_poll};

    #[test]
    fn only_the_active_visible_page_polls() {
        // Active section, Containers showing: only Containers polls.
        assert!(should_poll(
            true,
            DockerPage::Containers,
            DockerPage::Containers
        ));
        assert!(!should_poll(
            true,
            DockerPage::Containers,
            DockerPage::Images
        ));
        assert!(!should_poll(
            true,
            DockerPage::Containers,
            DockerPage::Volumes
        ));

        // Section not visible (user is in another tool): nothing polls.
        assert!(!should_poll(
            false,
            DockerPage::Containers,
            DockerPage::Containers
        ));
        assert!(!should_poll(false, DockerPage::Images, DockerPage::Images));
    }
}
