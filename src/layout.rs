use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::sidebar::{
    Sidebar, SidebarCollapsible, SidebarGroup, SidebarHeader, SidebarItem, SidebarMenuItem,
};
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, Collapsible, StyledExt as _, h_flex, v_flex};

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

/// A tool row that names itself while the sidebar is collapsed to icons.
///
/// **Wrapping is the only way to get that tooltip, and there is no risk of a
/// second one.** `SidebarMenuItem` at the pinned revision has no tooltip of its
/// own — `sidebar/menu.rs` has neither the field nor the builder — and
/// `SidebarMenu::children` accepts nothing but a `SidebarMenuItem`, so it
/// cannot be added from inside the menu either. `SidebarItem` is public though,
/// and `SidebarGroup` takes any implementation of it, so the rows go into the
/// group directly, each inside a `div` carrying the tooltip. `SidebarGroup`
/// already stacks its children with the same `gap_2` `SidebarMenu` used, so
/// dropping `SidebarMenu` changes nothing that is drawn.
///
/// This is still one flat row per tool — [`SidebarMenuItem::children`] stays
/// unused, for the reason [`View`] gives.
#[derive(Clone)]
struct ToolItem {
    item: SidebarMenuItem,
    /// The row's own translated title, the very string its label shows. A
    /// tooltip that read differently from the label would be a second string
    /// to translate and a second thing to keep in step.
    title: SharedString,
    collapsed: bool,
}

impl Collapsible for ToolItem {
    fn is_collapsed(&self) -> bool {
        self.collapsed
    }

    fn collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = collapsed;
        self
    }
}

impl SidebarItem for ToolItem {
    fn render(
        self,
        id: impl Into<ElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> impl IntoElement {
        let id = id.into();
        let collapsed = self.collapsed;
        let title = self.title;

        div()
            // Its own id, not the row's: `tooltip` comes from
            // `StatefulInteractiveElement`, so the wrapper has to be stateful,
            // and reusing the row's id for the element above it reads like a
            // mistake even though the two paths differ.
            .id(SharedString::from(format!("tool-tip-{id}")))
            .w_full()
            .when(collapsed, |this| {
                this.tooltip(move |window, cx| Tooltip::new(title.clone()).build(window, cx))
            })
            .child(self.item.collapsed(collapsed).render(id, window, cx))
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
    // Translated once: collapsed it is the tooltip, expanded it is the label.
    // The tooltip is never a second string written for the purpose.
    let label = t(label, cx);

    Button::new(id)
        .ghost()
        .w_full()
        .map(|this| {
            if icon_collapsed {
                this.px_0().tooltip(label.clone())
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
                    this.child(div().flex_shrink_0().whitespace_nowrap().child(label))
                }),
        )
}

pub struct Layout {
    collapsible: SidebarCollapsible,
    /// Whether the sidebar is showing icons only. **Starts `true`** — see
    /// [`Layout::new`] — and is not persisted, so every launch opens on the
    /// icon rail whatever the last session did.
    collapsed: bool,
    active: View,
    json_formatter: Entity<JsonFormatter>,
    encoder_decoder: Entity<EncoderDecoder>,
    api_explorer: Entity<ApiExplorer>,
    docker: Entity<DockerView>,
    database: Entity<DatabaseView>,
}

impl Layout {
    /// dodo opens on the **icon rail**, not the labelled sidebar: the tools are
    /// five fixed entries a user learns once, and the pane they are choosing
    /// between is the whole point of the window, so 240px of permanent chrome
    /// is a poor default. The toggle in the pane header is unchanged, and every
    /// collapsed icon carries its title as a tooltip, which is what keeps the
    /// rail readable to someone who has not learned it yet.
    ///
    /// The choice is **not persisted**, deliberately: dodo's six saved files are
    /// listed in `CLAUDE.md`, adding a seventh is a decision of its own, and a
    /// sidebar that opens the same way every time is at least predictable.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            collapsible: SidebarCollapsible::Icon,
            collapsed: true,
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
    fn menu(&self, cx: &mut Context<Self>) -> [ToolItem; View::ALL.len()] {
        View::ALL.map(|view| self.tool_item(view, cx))
    }

    /// A flat, top-level tool row. Docker is one of these like any other: the
    /// click enters the section on whichever page its rail last had selected,
    /// which resumes that page's polling; every other tool pauses it.
    fn tool_item(&self, view: View, cx: &mut Context<Self>) -> ToolItem {
        let layout = cx.entity();
        let title = t(view.title(), cx);
        let item = SidebarMenuItem::new(title.clone())
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
            });

        ToolItem {
            item,
            title,
            // `SidebarGroup` hands every child its own collapsed state on the
            // way to rendering it, so this is only a starting value.
            collapsed: false,
        }
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
                    .child(SidebarGroup::new(t(Str::Tools, cx)).children(self.menu(cx)))
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

    use gpui_component::Collapsible as _;
    use gpui_component::sidebar::SidebarMenuItem;

    use super::{ToolItem, View, pane_title};
    use crate::docker::DockerPage;
    use crate::i18n::Str;

    fn title_of(view: View, page: DockerPage) -> Discriminant<Str> {
        discriminant(&pane_title(view, page))
    }

    /// The source of one item, from its signature down to the next line that
    /// starts a new top-level item. Enough to ask what a given function does
    /// without depending on how it is formatted inside.
    fn item_source<'a>(source: &'a str, signature: &str) -> &'a str {
        let start = source
            .find(signature)
            .unwrap_or_else(|| panic!("`{signature}` is gone from src/layout.rs"));
        let body = &source[start..];
        match body.find("\n}\n") {
            Some(end) => &body[..end],
            None => body,
        }
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
    fn a_tool_row_takes_the_collapsed_state_the_group_hands_it() {
        // `SidebarGroup` calls `collapsed(..)` on each child on its way to
        // rendering it, and `ToolItem` decides whether to show a tooltip from
        // that same flag. Dropping it on the floor would leave every collapsed
        // icon anonymous, and nothing else would fail.
        let item = ToolItem {
            item: SidebarMenuItem::new("JSON Formatter"),
            title: "JSON Formatter".into(),
            collapsed: false,
        };

        assert!(!item.is_collapsed());
        assert!(item.clone().collapsed(true).is_collapsed());
        assert!(!item.collapsed(true).collapsed(false).is_collapsed());
    }

    #[test]
    fn every_icon_on_the_collapsed_rail_can_still_name_itself() {
        // A source scan, because a tooltip cannot be driven from a test on
        // macOS: `Root::new` dereferences a real `NSView`, so there is no
        // window to hover. Both call sites are checked because they reach the
        // tooltip by different routes — the tool rows through `ToolItem`,
        // which exists only for this, and the footer through `Button::tooltip`.
        let source = include_str!("layout.rs");

        assert!(
            item_source(source, "fn footer_button(").contains(".tooltip("),
            "the collapsed footer buttons show no label, so they must show a tooltip",
        );
        assert!(
            item_source(source, "impl SidebarItem for ToolItem").contains(".tooltip("),
            "the collapsed tool rows show no label, so they must show a tooltip",
        );
        // …and the extraction really is bounded, or the two above would pass
        // on any file that mentions a tooltip anywhere.
        assert!(!item_source(source, "fn pane_title(").contains(".tooltip("));
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
