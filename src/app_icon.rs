use gpui::SharedString;
use gpui_component::{Icon, IconNamed};

#[derive(Clone, Copy)]
pub enum AppIcon {
    Binary,
    Json,
    Palette,
    Settings,
    PanelLeftClose,
    PanelLeftOpen,
}

impl IconNamed for AppIcon {
    fn path(self) -> SharedString {
        match self {
            Self::Binary => "icons/binary.svg",
            Self::Json => "icons/json.svg",
            Self::Palette => "icons/palatte.svg",
            Self::Settings => "icons/settings.svg",
            Self::PanelLeftClose => "icons/panel-left-close.svg",
            Self::PanelLeftOpen => "icons/panel-left-open.svg",
        }
        .into()
    }
}

impl AppIcon {
    pub fn view(self) -> Icon {
        Icon::new(self)
    }
}
