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
use crate::encoder_decoder::EncoderDecoder;
use crate::i18n::{Str, t};
use crate::json_formatter::JsonFormatter;
use crate::settings;

/// Which tool is currently shown in the main pane. Selecting a sidebar item
/// switches the active view.
///
/// Adding a tool means: a variant here, a row in [`View::ALL`], an arm in
/// [`View::title`]/[`View::icon`], a field on [`Layout`] holding the view
/// entity, and an arm in the main-pane `match` of [`Layout::render`].
#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    JsonFormatter,
    EncoderDecoder,
    ApiExplorer,
}

impl View {
    const ALL: [View; 3] = [
        View::JsonFormatter,
        View::EncoderDecoder,
        View::ApiExplorer,
    ];

    fn title(self) -> Str {
        match self {
            View::JsonFormatter => Str::JsonFormatterTitle,
            View::EncoderDecoder => Str::EncoderDecoderTitle,
            View::ApiExplorer => Str::ApiExplorerTitle,
        }
    }

    fn icon(self) -> AppIcon {
        match self {
            View::JsonFormatter => AppIcon::Json,
            View::EncoderDecoder => AppIcon::Binary,
            View::ApiExplorer => AppIcon::Globe,
        }
    }
}

pub struct Layout {
    collapsible: SidebarCollapsible,
    collapsed: bool,
    active: View,
    json_formatter: Entity<JsonFormatter>,
    encoder_decoder: Entity<EncoderDecoder>,
    api_explorer: Entity<ApiExplorer>,
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
        }
    }

    fn menu(&self, cx: &mut Context<Self>) -> SidebarMenu {
        SidebarMenu::new().children(View::ALL.map(|view| {
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
        }))
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
                            }),
                    ),
            )
    }
}
