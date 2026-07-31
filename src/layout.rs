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
use crate::updater;

/// Which tool is currently shown in the main pane. Selecting a sidebar item
/// switches the active view.
///
/// Every tool is one flat row, Docker included: its four pages moved onto the
/// tab rail inside [`DockerView`] because a nested sidebar group renders no
/// children at all once the sidebar collapses to icons, which made those pages
/// unreachable.
///
/// Adding a tool means: a variant here, a row in [`View::ALL`], an arm in
/// [`View::title`]/[`View::icon`], a field on [`Layout`] holding the view
/// entity, and an arm in the main-pane `match` of [`Layout::render`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum View {
    JsonFormatter,
    EncoderDecoder,
    ApiExplorer,
    Docker,
}

impl View {
    /// Every tool, in sidebar order.
    const ALL: [View; 4] = [
        View::JsonFormatter,
        View::EncoderDecoder,
        View::ApiExplorer,
        View::Docker,
    ];

    /// The tool's own name — what the sidebar row reads. The main pane's title
    /// goes through [`pane_title`] instead, because Docker titles itself after
    /// the rail's selected page.
    fn title(self) -> Str {
        match self {
            View::JsonFormatter => Str::JsonFormatterTitle,
            View::EncoderDecoder => Str::EncoderDecoderTitle,
            View::ApiExplorer => Str::ApiExplorerTitle,
            View::Docker => Str::Docker,
        }
    }

    fn icon(self) -> AppIcon {
        match self {
            View::JsonFormatter => AppIcon::Json,
            View::EncoderDecoder => AppIcon::Binary,
            View::ApiExplorer => AppIcon::Globe,
            View::Docker => AppIcon::Container,
        }
    }
}

/// The heading above the main pane. Docker names the page its rail has selected
/// — the sidebar row says "Docker", the heading says "Containers" — so the
/// header keeps telling the user which of the four they are looking at, exactly
/// as it did when the four were sidebar children.
fn pane_title(view: View, docker_page: DockerPage) -> Str {
    match view {
        View::Docker => docker_page.title(),
        other => other.title(),
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

    /// The sidebar menu: one flat row per tool, no nesting. Nesting is what the
    /// icon-collapsed sidebar cannot render, so there is none.
    fn menu(&self, cx: &mut Context<Self>) -> SidebarMenu {
        SidebarMenu::new().children(View::ALL.map(|view| self.tool_item(view, cx)))
    }

    /// A flat, top-level tool row. Docker is one of these like any other: the
    /// click enters the section on whichever page its rail last had selected,
    /// which resumes that page's polling; every other tool pauses it.
    fn tool_item(&self, view: View, cx: &mut Context<Self>) -> SidebarMenuItem {
        let layout = cx.entity();
        SidebarMenuItem::new(t(view.title(), cx))
            .icon(view.icon().view())
            .active(self.active == view)
            .on_click(move |_, _, cx| {
                layout.update(cx, |this, cx| {
                    this.active = view;
                    this.docker.update(cx, |docker, cx| match view {
                        View::Docker => docker.activate(cx),
                        _ => docker.set_section_active(false, cx),
                    });
                    cx.notify();
                });
            })
    }
}

impl Render for Layout {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let icon_collapsed = self.collapsed && self.collapsible == SidebarCollapsible::Icon;
        let title = pane_title(self.active, self.docker.read(cx).page());

        h_flex()
            .size_full()
            .bg(cx.theme().background)
            .child(
                Sidebar::new("side-bar")
                    .collapsible(self.collapsible)
                    .collapsed(self.collapsed)
                    .w(px(240.))
                    .header(
                        SidebarHeader::new().child(
                            h_flex()
                                .gap_2()
                                .child(AppIcon::Dodo.view())
                                // Collapsed to icons the header keeps the mark
                                // and drops the word, the same treatment the
                                // Settings button below uses. "Dodo" is the
                                // product name and stays untranslated.
                                .when(!icon_collapsed, |this| this.child("Dodo")),
                        ),
                    )
                    .child(SidebarGroup::new(t(Str::Tools, cx)).child(self.menu(cx)))
                    .footer(
                        // `SidebarFooter` is an `h_flex`, so two `w_full`
                        // buttons handed to it directly would sit side by side
                        // and fight over a 240px sidebar. The stack is what
                        // keeps each one a full-width row, as the lone Settings
                        // button always was.
                        SidebarFooter::new().child(
                            v_flex()
                                .w_full()
                                .gap_1()
                                .child(
                                    // Beside Settings rather than inside it: this is
                                    // an action, not a preference, and the one
                                    // preference it carries ("check automatically")
                                    // lives in the dialog it opens.
                                    Button::new("check-for-updates")
                                        .ghost()
                                        .w_full()
                                        .justify_start()
                                        .child(
                                            h_flex().gap_2().child(AppIcon::Download.view()).when(
                                                !icon_collapsed,
                                                |this| {
                                                    // Fixed-length label in a
                                                    // 240px-wide sidebar: without
                                                    // these it wraps to two lines
                                                    // and pushes the footer taller.
                                                    this.child(
                                                        div()
                                                            .flex_shrink_0()
                                                            .whitespace_nowrap()
                                                            .child(t(Str::CheckForUpdates, cx)),
                                                    )
                                                },
                                            ),
                                        )
                                        .on_click(|_, window, cx| updater::open(window, cx)),
                                )
                                .child(
                                    Button::new("open-settings")
                                        .ghost()
                                        .w_full()
                                        .justify_start()
                                        .child(
                                            h_flex().gap_2().child(AppIcon::Settings.view()).when(
                                                !icon_collapsed,
                                                |this| {
                                                    this.child(
                                                        div()
                                                            .flex_shrink_0()
                                                            .whitespace_nowrap()
                                                            .child(t(Str::Settings, cx)),
                                                    )
                                                },
                                            ),
                                        )
                                        .on_click(|_, window, cx| settings::open(window, cx)),
                                ),
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
                            .child(div().font_bold().child(t(title, cx))),
                    )
                    .child(div().flex_1().min_h_0().map(|this| match self.active {
                        View::JsonFormatter => this.child(self.json_formatter.clone()),
                        View::EncoderDecoder => this.child(self.encoder_decoder.clone()),
                        View::ApiExplorer => this.child(self.api_explorer.clone()),
                        View::Docker => this.child(self.docker.clone()),
                    })),
            )
    }
}

#[cfg(test)]
mod tests {
    use std::mem::{Discriminant, discriminant};

    use super::{View, pane_title};
    use crate::docker::DockerPage;
    use crate::i18n::Str;

    fn title_of(view: View, page: DockerPage) -> Discriminant<Str> {
        discriminant(&pane_title(view, page))
    }

    #[test]
    fn the_sidebar_lists_every_tool_once_with_docker_flat_and_last() {
        assert_eq!(
            View::ALL,
            [
                View::JsonFormatter,
                View::EncoderDecoder,
                View::ApiExplorer,
                View::Docker,
            ]
        );
        // One row per tool: Docker is a single entry, not a group of five.
        assert_eq!(View::ALL.len(), 4);
    }

    #[test]
    fn the_docker_row_reads_docker_while_the_pane_reads_the_page() {
        assert_eq!(
            discriminant(&View::Docker.title()),
            discriminant(&Str::Docker)
        );

        for (page, expected) in [
            (DockerPage::Containers, Str::Containers),
            (DockerPage::Images, Str::Images),
            (DockerPage::Volumes, Str::Volumes),
            (DockerPage::Networks, Str::Networks),
        ] {
            assert_eq!(
                title_of(View::Docker, page),
                discriminant(&expected),
                "the pane heading must follow the rail's selected page",
            );
        }
    }

    #[test]
    fn the_other_tools_ignore_the_docker_page() {
        for view in [View::JsonFormatter, View::EncoderDecoder, View::ApiExplorer] {
            for page in DockerPage::ALL {
                assert_eq!(title_of(view, page), discriminant(&view.title()));
            }
        }
    }
}
