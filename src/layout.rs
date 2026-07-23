use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::sidebar::{
    Sidebar, SidebarCollapsible, SidebarFooter, SidebarGroup, SidebarHeader, SidebarMenu,
    SidebarMenuItem,
};
use gpui_component::{ActiveTheme, StyledExt as _, h_flex, v_flex};

use crate::api_explorer::ApiExplorer;
use crate::app_icon::AppIcon;
use crate::docker::{DockerPage, DockerView};
use crate::encoder_decoder::EncoderDecoder;
use crate::i18n::{Str, t};
use crate::json_formatter::JsonFormatter;
use crate::settings;

/// Which tool is currently shown in the main pane. Selecting a sidebar item
/// switches the active view.
///
/// The three standalone tools sit in the Tools group; the four Docker views are
/// the children of the expandable Docker section and all resolve to the one
/// [`DockerView`] entity, which shows the page the active variant names.
///
/// Adding a standalone tool means: a variant here, a row in [`View::TOOLS`], an
/// arm in [`View::title`]/[`View::icon`], a field on [`Layout`] holding the view
/// entity, and an arm in the main-pane `match` of [`Layout::render`].
#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    JsonFormatter,
    EncoderDecoder,
    ApiExplorer,
    DockerContainers,
    DockerImages,
    DockerVolumes,
    DockerNetworks,
}

impl View {
    /// The standalone tools, shown flat in the Tools group.
    const TOOLS: [View; 3] = [
        View::JsonFormatter,
        View::EncoderDecoder,
        View::ApiExplorer,
    ];

    /// The Docker section's children, shown under the expandable Docker item.
    const DOCKER: [View; 4] = [
        View::DockerContainers,
        View::DockerImages,
        View::DockerVolumes,
        View::DockerNetworks,
    ];

    fn title(self) -> Str {
        match self {
            View::JsonFormatter => Str::JsonFormatterTitle,
            View::EncoderDecoder => Str::EncoderDecoderTitle,
            View::ApiExplorer => Str::ApiExplorerTitle,
            View::DockerContainers => Str::Containers,
            View::DockerImages => Str::Images,
            View::DockerVolumes => Str::Volumes,
            View::DockerNetworks => Str::Networks,
        }
    }

    fn icon(self) -> AppIcon {
        match self {
            View::JsonFormatter => AppIcon::Json,
            View::EncoderDecoder => AppIcon::Binary,
            View::ApiExplorer => AppIcon::Globe,
            View::DockerContainers => AppIcon::Container,
            View::DockerImages => AppIcon::Layers,
            View::DockerVolumes => AppIcon::HardDrive,
            View::DockerNetworks => AppIcon::Network,
        }
    }

    /// The Docker page a Docker view names, if it is one.
    fn docker_page(self) -> Option<DockerPage> {
        match self {
            View::DockerContainers => Some(DockerPage::Containers),
            View::DockerImages => Some(DockerPage::Images),
            View::DockerVolumes => Some(DockerPage::Volumes),
            View::DockerNetworks => Some(DockerPage::Networks),
            _ => None,
        }
    }

    fn is_docker(self) -> bool {
        self.docker_page().is_some()
    }
}

pub struct Layout {
    collapsible: SidebarCollapsible,
    collapsed: bool,
    active: View,
    json_formatter: Entity<JsonFormatter>,
    encoder_decoder: Entity<EncoderDecoder>,
    api_explorer: Entity<ApiExplorer>,
    docker: Entity<DockerView>,
}

impl Layout {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            collapsible: SidebarCollapsible::Icon,
            collapsed: false,
            active: View::JsonFormatter,
            json_formatter: cx.new(|cx| JsonFormatter::new(window, cx)),
            encoder_decoder: cx.new(|cx| EncoderDecoder::new(window, cx)),
            api_explorer: cx.new(|cx| ApiExplorer::new(window, cx)),
            docker: cx.new(|cx| DockerView::new(window, cx)),
        }
    }

    /// The full sidebar menu: the flat Tools, then the expandable Docker section.
    fn menu(&self, cx: &mut Context<Self>) -> SidebarMenu {
        let mut items: Vec<SidebarMenuItem> =
            View::TOOLS.iter().map(|view| self.tool_item(*view, cx)).collect();
        items.push(self.docker_item(cx));
        SidebarMenu::new().children(items)
    }

    /// A flat, top-level tool row.
    fn tool_item(&self, view: View, cx: &mut Context<Self>) -> SidebarMenuItem {
        let layout = cx.entity();
        SidebarMenuItem::new(t(view.title(), cx))
            .icon(view.icon().view())
            .active(self.active == view)
            .on_click(move |_, _, cx| {
                layout.update(cx, |this, cx| {
                    this.active = view;
                    cx.notify();
                });
            })
    }

    /// The expandable Docker section. It stays open by default and toggles on
    /// click; its four children select the Docker pages. The section's own
    /// open/collapsed state lives in the sidebar widget's keyed state, so it
    /// survives re-renders and which child is active is preserved independently.
    fn docker_item(&self, cx: &mut Context<Self>) -> SidebarMenuItem {
        let children: Vec<SidebarMenuItem> =
            View::DOCKER.iter().map(|view| self.docker_child(*view, cx)).collect();
        SidebarMenuItem::new(t(Str::Docker, cx))
            .icon(AppIcon::Container.view())
            // Active whenever any Docker page is showing, so the section reads as
            // selected even when collapsed.
            .active(self.active.is_docker())
            .default_open(true)
            .click_to_toggle(true)
            .children(children)
    }

    /// One Docker child row: selects the page and points the Docker view at it.
    fn docker_child(&self, view: View, cx: &mut Context<Self>) -> SidebarMenuItem {
        let layout = cx.entity();
        let Some(page) = view.docker_page() else {
            unreachable!("docker_child called with a non-Docker view");
        };
        SidebarMenuItem::new(t(view.title(), cx))
            .icon(view.icon().view())
            .active(self.active == view)
            .on_click(move |_, _, cx| {
                layout.update(cx, |this, cx| {
                    this.active = view;
                    this.docker.update(cx, |docker, cx| docker.set_page(page, cx));
                    cx.notify();
                });
            })
    }
}

impl Render for Layout {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let icon_collapsed = self.collapsed && self.collapsible == SidebarCollapsible::Icon;

        h_flex()
            .size_full()
            .bg(cx.theme().background)
            .child(
                Sidebar::new("side-bar")
                    .collapsible(self.collapsible)
                    .collapsed(self.collapsed)
                    .w(px(240.))
                    // "Dodo" is the product name and stays untranslated.
                    .header(SidebarHeader::new().child("Dodo"))
                    .child(SidebarGroup::new(t(Str::Tools, cx)).child(self.menu(cx)))
                    .footer(
                        SidebarFooter::new().child(
                            Button::new("open-settings")
                                .ghost()
                                .w_full()
                                .justify_start()
                                .child(
                                    h_flex()
                                        .gap_2()
                                        .child(AppIcon::Settings.view())
                                        .when(!icon_collapsed, |this| {
                                            this.child(t(Str::Settings, cx))
                                        }),
                                )
                                .on_click(|_, window, cx| settings::open(window, cx)),
                        ),
                    ),
            )
            .child(
                v_flex()
                    .h_full()
                    .flex_1()
                    .min_w_0()
                    .gap_4()
                    .p_4()
                    .child(
                        h_flex()
                            .items_center()
                            .gap_3()
                            .child(
                                Button::new("toggle-sidebar")
                                    .child(
                                        (if icon_collapsed {
                                            AppIcon::PanelLeftOpen
                                        } else {
                                            AppIcon::PanelLeftClose
                                        })
                                        .view(),
                                    )
                                    .ghost()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.collapsed = !this.collapsed;
                                        cx.notify();
                                    })),
                            )
                            .child(div().font_bold().child(t(self.active.title(), cx))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .map(|this| match self.active {
                                View::JsonFormatter => this.child(self.json_formatter.clone()),
                                View::EncoderDecoder => this.child(self.encoder_decoder.clone()),
                                View::ApiExplorer => this.child(self.api_explorer.clone()),
                                View::DockerContainers
                                | View::DockerImages
                                | View::DockerVolumes
                                | View::DockerNetworks => this.child(self.docker.clone()),
                            }),
                    ),
            )
    }
}
