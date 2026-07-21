use gpui::SharedString;
use gpui_component::{Icon, IconNamed};

/// The icons bundled under `assets/icons`.
///
/// Registering a variant here is also what makes the SVG reachable by path, so
/// a variant may exist purely to satisfy a gpui-component widget that asks for
/// the equivalent `IconName` (which resolves to the same `icons/<name>.svg`).
#[derive(Clone, Copy)]
pub enum AppIcon {
    Binary,
    Json,
    Palette,
    /// Used by the Settings dialog's search box, via the library's
    /// `IconName::Search`.
    #[allow(dead_code)]
    Search,
    Settings,
    Sliders,
    PanelLeftClose,
    PanelLeftOpen,
}

impl IconNamed for AppIcon {
    fn path(self) -> SharedString {
        match self {
            Self::Binary => "icons/binary.svg",
            Self::Json => "icons/json.svg",
            Self::Palette => "icons/palatte.svg",
            Self::Search => "icons/search.svg",
            Self::Settings => "icons/settings.svg",
            Self::Sliders => "icons/sliders.svg",
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
