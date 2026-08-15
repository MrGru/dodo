//! The Docker module's top view: the in-page tab rail, the four pages it
//! switches between, and the selected page's body.
//!
//! The sidebar has one flat Docker row; everything below it is this view's own
//! navigation. [`DockerView::render`] draws a vertical rail down the left edge —
//! one tab per [`DockerPage`], icon above label — and the selected page beside
//! it. Tabs call [`DockerView::set_page`]; the sidebar calls
//! [`DockerView::activate`] on the way in and
//! [`DockerView::set_section_active`] on the way out, which is what starts and
//! stops the background polling.
//!
//! The entity and its sub-views are built once and kept, so navigating between
//! pages — and away to another tool and back — preserves each page's state, the
//! same lifetime rule `Layout` follows for the top-level tools. Each page is
//! loaded lazily the first time it is shown.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    Pixels, Render, StatefulInteractiveElement as _, Styled as _, Window, div, px,
};
use gpui_component::{ActiveTheme as _, Icon, h_flex, v_flex};

use crate::app_icon::AppIcon;
use crate::i18n::{Str, docker, t};
use crate::views::containers::ContainersView;
use crate::views::images::ImagesView;
use crate::views::networks::NetworksView;
use crate::views::runtime::RuntimesView;
use crate::views::volumes::VolumesView;

/// The rail's width. Wide enough for the longest page name at `text_xs` on one
/// line — the four names are terms of art and identical in every language, so
/// this never has to grow — and narrow enough that it reads as a rail rather
/// than a second sidebar.
const RAIL_WIDTH: Pixels = px(84.);

/// The accent bar down the left edge of the selected tab. It occupies its width
/// on every tab, transparent when unselected, so selecting one never shifts the
/// icon or the label sideways.
const RAIL_ACCENT_WIDTH: Pixels = px(2.);

/// Which Docker page is showing. The five are the rail's tabs, in the order
/// [`DockerPage::ALL`] lists them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DockerPage {
    Containers,
    Images,
    Volumes,
    Networks,
    /// Round 7: automatic detection of the container runtimes/daemons on this
    /// machine plus Start/Stop. Last on the rail — it is about the host
    /// machine rather than the connected engine's own resources, so it reads
    /// as an addendum to the other four rather than a peer swapped in among
    /// them.
    Runtimes,
}

impl DockerPage {
    /// The pages the rail shows, top to bottom. `ALL[0]` is also the page a
    /// freshly opened Docker section starts on — see [`DockerView::new`].
    pub const ALL: [DockerPage; 5] = [
        DockerPage::Containers,
        DockerPage::Images,
        DockerPage::Volumes,
        DockerPage::Networks,
        DockerPage::Runtimes,
    ];

    /// The page a freshly opened Docker section shows.
    pub const DEFAULT: DockerPage = DockerPage::ALL[0];

    /// The localized page name. It labels the rail tab and is the title
    /// `Layout` shows above the main pane while Docker is the active view, so
    /// the header keeps naming the page rather than the section.
    pub fn title(self) -> Str {
        match self {
            DockerPage::Containers => docker::Text::Containers.into(),
            DockerPage::Images => docker::Text::Images.into(),
            DockerPage::Volumes => docker::Text::Volumes.into(),
            DockerPage::Networks => docker::Text::Networks.into(),
            DockerPage::Runtimes => docker::Text::Runtimes.into(),
        }
    }

    /// The page's icon, drawn above its label on the rail. The first four are
    /// the same icons the sidebar's Docker children carried before the pages
    /// moved in here, so nothing a user recognises changed; `Runtimes` is new
    /// in round 7.
    pub fn icon(self) -> AppIcon {
        match self {
            DockerPage::Containers => AppIcon::Container,
            DockerPage::Images => AppIcon::Layers,
            DockerPage::Volumes => AppIcon::HardDrive,
            DockerPage::Networks => AppIcon::Network,
            DockerPage::Runtimes => AppIcon::MemoryStick,
        }
    }

    /// The rail tab's element id. Stable per page, so gpui keeps each tab's
    /// hover and click state across re-renders.
    fn rail_id(self) -> &'static str {
        match self {
            DockerPage::Containers => "docker-rail-containers",
            DockerPage::Images => "docker-rail-images",
            DockerPage::Volumes => "docker-rail-volumes",
            DockerPage::Networks => "docker-rail-networks",
            DockerPage::Runtimes => "docker-rail-runtimes",
        }
    }
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
    runtimes: Entity<RuntimesView>,
}

impl DockerView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            page: DockerPage::DEFAULT,
            // The window opens on another tool, so the Docker section is inactive
            // until the sidebar selects it.
            section_active: false,
            containers: cx.new(|cx| ContainersView::new(window, cx)),
            images: cx.new(|cx| ImagesView::new(window, cx)),
            volumes: cx.new(|cx| VolumesView::new(window, cx)),
            networks: cx.new(|cx| NetworksView::new(window, cx)),
            runtimes: cx.new(|cx| RuntimesView::new(window, cx)),
        }
    }

    /// The page the rail currently has selected. `Layout` reads it to title the
    /// main pane after the page rather than after the section.
    pub fn page(&self) -> DockerPage {
        self.page
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
            DockerPage::Runtimes => {
                self.runtimes.update(cx, |view, cx| view.ensure_loaded(cx));
            }
        }
        self.sync_polling(cx);
        cx.notify();
    }

    /// Enters the section from the sidebar: shows whichever page the rail last
    /// had selected — [`DockerPage::DEFAULT`] the first time — and resumes its
    /// polling. Routing through [`set_page`] keeps the lazy load and the polling
    /// sync in one place.
    pub fn activate(&mut self, cx: &mut Context<Self>) {
        self.set_page(self.page, cx);
    }

    /// Tells the section whether it is the visible view. The sidebar calls this
    /// with `false` when the user leaves for another tool, pausing all polling,
    /// and it resumes through [`activate`] on return.
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
        self.runtimes.update(cx, |view, cx| {
            view.set_polling(should_poll(active, page, DockerPage::Runtimes), cx)
        });
    }

    /// The vertical tab rail down the left edge. Always visible and always
    /// showing all four tabs — it is a tab strip, not the API Explorer's
    /// collapsible panel switcher, which is otherwise the same idea.
    fn render_rail(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        v_flex()
            .h_full()
            .flex_shrink_0()
            .w(RAIL_WIDTH)
            .py_1()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .overflow_hidden()
            .children(DockerPage::ALL.map(|page| self.render_rail_tab(page, cx)))
            .into_any_element()
    }

    /// One rail tab: the accent bar, then the page's icon above its label,
    /// centred. The selected tab is marked twice over — the bar and a raised
    /// background — so it reads at a glance without relying on colour alone.
    fn render_rail_tab(&self, page: DockerPage, cx: &mut Context<Self>) -> gpui::AnyElement {
        let selected = self.page == page;
        let accent = if selected {
            cx.theme().primary
        } else {
            cx.theme().transparent
        };

        h_flex()
            .id(page.rail_id())
            .items_stretch()
            .w_full()
            .flex_shrink_0()
            .cursor_pointer()
            .when(selected, |this| this.bg(cx.theme().accent))
            .when(!selected, |this| {
                this.hover(|this| this.bg(cx.theme().accent.opacity(0.5)))
            })
            .child(div().w(RAIL_ACCENT_WIDTH).flex_shrink_0().bg(accent))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap_1()
                    .px_1()
                    .py_2()
                    .text_color(if selected {
                        cx.theme().foreground
                    } else {
                        cx.theme().muted_foreground
                    })
                    .child(Icon::new(page.icon()).size_5())
                    .child(div().text_xs().text_center().child(t(page.title(), cx))),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.set_page(page, cx)))
            .into_any_element()
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match self.page {
            DockerPage::Containers => self.containers.clone().into_any_element(),
            DockerPage::Images => self.images.clone().into_any_element(),
            DockerPage::Volumes => self.volumes.clone().into_any_element(),
            DockerPage::Networks => self.networks.clone().into_any_element(),
            DockerPage::Runtimes => self.runtimes.clone().into_any_element(),
        };

        h_flex()
            .items_stretch()
            .size_full()
            .gap_2()
            .child(self.render_rail(cx))
            .child(div().flex_1().min_w_0().h_full().child(body))
    }
}

#[cfg(test)]
mod tests {

    use super::{DockerPage, should_poll};
    use crate::i18n::{Str, docker};

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

    #[test]
    fn rail_lists_every_page_containers_first() {
        assert_eq!(
            DockerPage::ALL,
            [
                DockerPage::Containers,
                DockerPage::Images,
                DockerPage::Volumes,
                DockerPage::Networks,
                DockerPage::Runtimes,
            ]
        );
        assert_eq!(DockerPage::DEFAULT, DockerPage::Containers);
    }

    #[test]
    fn every_page_has_its_own_title_icon_and_tab_id() {
        // Page titles carry no runtime values, so they compare as themselves.
        let titles: Vec<Str> = DockerPage::ALL.iter().map(|page| page.title()).collect();
        assert_eq!(
            titles,
            vec![
                docker::Text::Containers.into(),
                docker::Text::Images.into(),
                docker::Text::Volumes.into(),
                docker::Text::Networks.into(),
                docker::Text::Runtimes.into(),
            ]
        );

        // Distinct icon paths and distinct element ids: two tabs sharing either
        // would make the rail unreadable or swallow a click.
        let mut icons: Vec<String> = DockerPage::ALL
            .iter()
            .map(|page| gpui_component::IconNamed::path(page.icon()).to_string())
            .collect();
        icons.sort();
        icons.dedup();
        assert_eq!(icons.len(), DockerPage::ALL.len());

        let mut ids: Vec<&str> = DockerPage::ALL.iter().map(|page| page.rail_id()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), DockerPage::ALL.len());
    }
}
