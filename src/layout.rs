use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::sidebar::{
    Sidebar, SidebarCollapsible, SidebarGroup, SidebarHeader, SidebarMenu, SidebarMenuItem,
};
use gpui_component::{ActiveTheme, StyledExt as _, h_flex, v_flex};

use crate::api_explorer::ApiExplorer;
use crate::app_icon::AppIcon;
use crate::database::DatabaseView;
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
    Database,
}

impl View {
    /// Every tool, in sidebar order.
    const ALL: [View; 5] = [
        View::JsonFormatter,
        View::EncoderDecoder,
        View::ApiExplorer,
        View::Docker,
        View::Database,
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
            View::Database => Str::DatabaseTitle,
        }
    }

    fn icon(self) -> AppIcon {
        match self {
            View::JsonFormatter => AppIcon::Json,
            View::EncoderDecoder => AppIcon::Binary,
            View::ApiExplorer => AppIcon::Globe,
            View::Docker => AppIcon::Container,
            View::Database => AppIcon::Database,
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

/// One sidebar-footer row: the icon, and the label beside it when the sidebar
/// is wide enough to have one.
///
/// **Lining the collapsed icon up with the tool icons above is arithmetic, not
/// taste**, and it is worth writing down because two of the three numbers come
/// from inside the widget library (`sidebar/mod.rs`, `sidebar/menu.rs` and
/// `button/button.rs` in the pinned checkout):
///
/// * Collapsed the rail is 48px and `Sidebar` insets the menu (`#inner`'s
///   `p_2`) and the footer (`px_2`) by the same 8px, so each gets the same
///   31px-wide box. A menu row fills that box and centres its icon in it, so
///   the footer button has to fill it too — hence `w_full` and **`px_0`**,
///   because `Button`'s own `px_4` is wider than the whole box and pushes its
///   contents out past the right-hand edge.
/// * Expanded both are inset 12px (`px_3`) and a menu row puts its icon 8px
///   further in (`p_2`), so `px_2` on the button lands the footer icon in the
///   same column, and the label in the same column as the row labels.
///
/// Two things this deliberately does *not* use, having read what they do:
///
/// * **`SidebarFooter`** — it adds a second `p_2` the menu rows do not have,
///   which halves the collapsed box to 15px and leaves no way to reach the
///   rail's centre, plus a hover highlight spanning the whole footer that a
///   menu row has no counterpart for. `Sidebar::footer` takes any element, so
///   the `v_flex` goes in directly. It is still a stack rather than two loose
///   buttons: the sidebar's own footer wrapper is an `h_flex`, so siblings
///   would sit side by side.
/// * **`.justify_start()`** — which these buttons used to carry, and which
///   cannot align anything here: `Button` wraps its children in an
///   `h_flex().size_full().justify_center()`, so the outer justification never
///   reaches the icon. The child's own `w_full` is what left-aligns the
///   expanded row.
fn footer_button(
    id: &'static str,
    icon: AppIcon,
    label: Str,
    icon_collapsed: bool,
    cx: &App,
) -> Button {
    Button::new(id)
        .ghost()
        .w_full()
        .map(|this| {
            if icon_collapsed {
                this.px_0()
            } else {
                this.px_2()
            }
        })
        .child(
            h_flex()
                .gap_2()
                .when(!icon_collapsed, |this| this.w_full())
                .child(icon.view())
                .when(!icon_collapsed, |this| {
                    // Fixed-length label in a 240px-wide sidebar: without these
                    // it wraps to two lines and pushes the footer taller.
                    this.child(
                        div()
                            .flex_shrink_0()
                            .whitespace_nowrap()
                            .child(t(label, cx)),
                    )
                }),
        )
}

pub struct Layout {
    collapsible: SidebarCollapsible,
    collapsed: bool,
    active: View,
    json_formatter: Entity<JsonFormatter>,
    encoder_decoder: Entity<EncoderDecoder>,
    api_explorer: Entity<ApiExplorer>,
    docker: Entity<DockerView>,
    database: Entity<DatabaseView>,
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
            database: cx.new(|cx| DatabaseView::new(window, cx)),
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
                        // A plain stack, not a `SidebarFooter` — see
                        // [`footer_button`] for why, and for where its two
                        // paddings come from. `gap_2` is the menu's own row
                        // gap, so collapsed the icons keep the same rhythm all
                        // the way down the rail.
                        v_flex()
                            .w_full()
                            .gap_2()
                            .child(
                                // Beside Settings rather than inside it: this is
                                // an action, not a preference, and the one
                                // preference it carries ("check automatically")
                                // lives in the dialog it opens.
                                footer_button(
                                    "check-for-updates",
                                    AppIcon::Download,
                                    Str::CheckForUpdates,
                                    icon_collapsed,
                                    cx,
                                )
                                .on_click(|_, window, cx| updater::open(window, cx)),
                            )
                            .child(
                                footer_button(
                                    "open-settings",
                                    AppIcon::Settings,
                                    Str::Settings,
                                    icon_collapsed,
                                    cx,
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
                            .child(div().font_bold().child(t(title, cx))),
                    )
                    .child(div().flex_1().min_h_0().map(|this| match self.active {
                        View::JsonFormatter => this.child(self.json_formatter.clone()),
                        View::EncoderDecoder => this.child(self.encoder_decoder.clone()),
                        View::ApiExplorer => this.child(self.api_explorer.clone()),
                        View::Docker => this.child(self.docker.clone()),
                        View::Database => this.child(self.database.clone()),
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
                View::Database,
            ]
        );
        // One row per tool: Docker and Database are each a single entry, not a
        // group of children — an icon-collapsed sidebar renders no children at
        // all, which is what made Docker's four pages unreachable.
        assert_eq!(View::ALL.len(), 5);
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
        for view in [
            View::JsonFormatter,
            View::EncoderDecoder,
            View::ApiExplorer,
            View::Database,
        ] {
            for page in DockerPage::ALL {
                assert_eq!(title_of(view, page), discriminant(&view.title()));
            }
        }
    }
}
