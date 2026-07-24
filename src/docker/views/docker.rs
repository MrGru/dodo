//! The Docker module's top view: it owns the four pages and shows the selected
//! one.
//!
//! The sidebar's four Docker children switch [`DockerPage`] through
//! [`DockerView::set_page`]; the entity and its sub-views are built once and
//! kept, so navigating between pages — and away to another tool and back —
//! preserves each page's state, the same lifetime rule `Layout` follows for the
//! top-level tools. Rounds 1–3 implement all four pages; each is loaded lazily
//! the first time it is shown.

use gpui::{
    AppContext as _, Context, Entity, IntoElement, Render, Window,
};

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
            containers: cx.new(|cx| ContainersView::new(window, cx)),
            images: cx.new(|cx| ImagesView::new(window, cx)),
            volumes: cx.new(|cx| VolumesView::new(window, cx)),
            networks: cx.new(|cx| NetworksView::new(window, cx)),
        }
    }

    /// Shows `page` and triggers its first load. Each load is lazy so the engine
    /// is not touched until the page is actually opened, and idempotent so
    /// returning to a page does not reload it.
    pub fn set_page(&mut self, page: DockerPage, cx: &mut Context<Self>) {
        self.page = page;
        match page {
            DockerPage::Containers => {
                self.containers.update(cx, |view, cx| view.ensure_loaded(cx));
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
        cx.notify();
    }
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
