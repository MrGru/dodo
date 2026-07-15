use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::sidebar::{
    Sidebar, SidebarCollapsible, SidebarFooter, SidebarGroup, SidebarHeader, SidebarMenu,
    SidebarMenuItem, SidebarToggleButton,
};
use gpui_component::{ActiveTheme, Selectable, Sizable, StyledExt, h_flex, v_flex};

use crate::app_icon::AppIcon;

pub struct Layout {
    collapsible: SidebarCollapsible,
    collapsed: bool,
}

impl Layout {
    pub fn new() -> Self {
        Self {
            collapsible: SidebarCollapsible::Icon,
            collapsed: false,
        }
    }

    fn menu() -> SidebarMenu {
        SidebarMenu::new()
            .children([SidebarMenuItem::new("Json formatter").icon(AppIcon::Json.view())])
    }

    fn description(&self) -> &'static str {
        match self.collapsible {
            SidebarCollapsible::Icon => {
                "The sidebar collapses to icon width, matching shadcn's collapsible=\"icon\" behavior."
            }
            SidebarCollapsible::Offcanvas => {
                "The sidebar releases its layout width when collapsed and keeps hidden controls out of keyboard navigation, matching shadcn's collapsible=\"offcanvas\" behavior."
            }
            SidebarCollapsible::None => {
                "The sidebar ignores the collapsed state and remains expanded, matching shadcn's collapsible=\"none\" behavior."
            }
        }
    }

    fn mode_button(
        &mut self,
        id: &'static str,
        label: &'static str,
        mode: SidebarCollapsible,
        cx: &mut Context<Self>,
    ) -> Button {
        Button::new(id)
            .label(label)
            .small()
            .selected(self.collapsible == mode)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.collapsible = mode;
                cx.notify();
            }))
    }
}

impl Render for Layout {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let icon_collapsed = self.collapsed && self.collapsible == SidebarCollapsible::Icon;
        let show_toggle = self.collapsible != SidebarCollapsible::None;

        h_flex()
            .size_full()
            .bg(cx.theme().background)
            .child(
                Sidebar::new("side-bar")
                    .collapsible(self.collapsible)
                    .collapsed(self.collapsed)
                    .w(px(240.))
                    .header(SidebarHeader::new().child("Dodo"))
                    .child(SidebarGroup::new("General").child(Self::menu()))
                    .footer(
                        SidebarFooter::new().child(
                            h_flex()
                                .gap_2()
                                .child(AppIcon::Settings.view())
                                .when(!icon_collapsed, |this| this.child("Settings")),
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
                            .when(show_toggle, |this| {
                                this.child(
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
                            })
                            .child(div().font_bold().child("Sidebar collapsible modes")),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(div().text_sm().child("Mode:"))
                            .child(self.mode_button(
                                "mode-icon",
                                "Icon",
                                SidebarCollapsible::Icon,
                                cx,
                            ))
                            .child(self.mode_button(
                                "mode-offcanvas",
                                "Offcanvas",
                                SidebarCollapsible::Offcanvas,
                                cx,
                            ))
                            .child(self.mode_button(
                                "mode-none",
                                "None",
                                SidebarCollapsible::None,
                                cx,
                            )),
                    )
                    .child(
                        div()
                            .flex_1()
                            .rounded(cx.theme().radius)
                            .border_1()
                            .border_color(cx.theme().border)
                            .p_5()
                            .child(self.description()),
                    ),
            )
    }
}
