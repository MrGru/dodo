//! The Docker module's top view: it owns the four pages and shows the selected
//! one.
//!
//! The sidebar's four Docker children switch [`DockerPage`] through
//! [`DockerView::set_page`]; the entity and its sub-views are built once and
//! kept, so navigating between pages — and away to another tool and back —
//! preserves each page's state, the same lifetime rule `Layout` follows for the
//! top-level tools. Round 1 implements Containers; the other three are the
//! placeholder pages their real versions replace in a later round.

use gpui::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _,
    Window,
};
use gpui_component::{ActiveTheme as _, v_flex};

use crate::app_icon::AppIcon;
use crate::docker::components::states::empty_state;
use crate::docker::views::containers::ContainersView;
use crate::i18n::{Str, t};

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
    /// The Containers page, built once and kept so its rows, search and
    /// selection survive navigation.
    containers: Entity<ContainersView>,
}

impl DockerView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            page: DockerPage::Containers,
            containers: cx.new(|cx| ContainersView::new(window, cx)),
        }
    }

    /// Shows `page`. Selecting Containers also triggers its first load — the load
    /// is lazy so the engine is not touched until the page is actually opened,
    /// and idempotent so returning to it does not reload.
    pub fn set_page(&mut self, page: DockerPage, cx: &mut Context<Self>) {
        self.page = page;
        if matches!(page, DockerPage::Containers) {
            self.containers
                .update(cx, |view, cx| view.ensure_loaded(cx));
        }
        cx.notify();
    }
}

impl Render for DockerView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match self.page {
            DockerPage::Containers => self.containers.clone().into_any_element(),
            DockerPage::Images => placeholder(AppIcon::Layers, Str::Images, cx),
            DockerPage::Volumes => placeholder(AppIcon::HardDrive, Str::Volumes, cx),
            DockerPage::Networks => placeholder(AppIcon::Network, Str::Networks, cx),
        }
    }
}

/// A "coming soon" page: the nav shape is correct now; the real page lands in a
/// later round. Framed like the Containers page so switching between them does
/// not jump.
fn placeholder(icon: AppIcon, title: Str, cx: &mut App) -> gpui::AnyElement {
    v_flex()
        .size_full()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .overflow_hidden()
        .child(empty_state(
            icon,
            t(title, cx),
            Some(t(Str::DockerComingSoon, cx)),
            cx,
        ))
        .into_any_element()
}
