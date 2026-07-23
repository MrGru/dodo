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

    // API Explorer. The five below ship as our own SVGs; the rest resolve
    // through `Assets`' fallback to `gpui_component_assets`, which already
    // carries them — see `src/assets.rs`.
    Clock,
    Import,
    Save,
    Send,
    SquareCode,
    ArrowDown,
    ArrowUp,
    ChevronDown,
    ChevronRight,
    Close,
    Copy,
    Ellipsis,
    File,
    Folder,
    FolderOpen,
    Globe,
    HardDrive,
    PanelBottom,
    Plus,
    Trash,

    // Docker module. `container`, `layers`, `refresh-cw`, `filter`, `square`
    // and `rotate-ccw` ship as our own SVGs; the rest resolve through `Assets`'
    // fallback to `gpui_component_assets`.
    Container,
    Layers,
    Network,
    Inbox,
    AlertTriangle,
    Refresh,
    Filter,
    Play,
    Stop,
    Restart,
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
            Self::Clock => "icons/clock.svg",
            Self::Import => "icons/import.svg",
            Self::Save => "icons/save.svg",
            Self::Send => "icons/send.svg",
            Self::SquareCode => "icons/square-code.svg",
            Self::ArrowDown => "icons/arrow-down.svg",
            Self::ArrowUp => "icons/arrow-up.svg",
            Self::ChevronDown => "icons/chevron-down.svg",
            Self::ChevronRight => "icons/chevron-right.svg",
            Self::Close => "icons/close.svg",
            Self::Copy => "icons/copy.svg",
            Self::Ellipsis => "icons/ellipsis-vertical.svg",
            Self::File => "icons/file.svg",
            Self::Folder => "icons/folder.svg",
            Self::FolderOpen => "icons/folder-open.svg",
            Self::Globe => "icons/globe.svg",
            Self::HardDrive => "icons/hard-drive.svg",
            Self::PanelBottom => "icons/panel-bottom.svg",
            Self::Plus => "icons/plus.svg",
            Self::Trash => "icons/delete.svg",
            Self::Container => "icons/container.svg",
            Self::Layers => "icons/layers.svg",
            Self::Network => "icons/network.svg",
            Self::Inbox => "icons/inbox.svg",
            Self::AlertTriangle => "icons/triangle-alert.svg",
            Self::Refresh => "icons/refresh-cw.svg",
            Self::Filter => "icons/filter.svg",
            Self::Play => "icons/play.svg",
            Self::Stop => "icons/square.svg",
            Self::Restart => "icons/rotate-ccw.svg",
        }
        .into()
    }
}

impl AppIcon {
    pub fn view(self) -> Icon {
        Icon::new(self)
    }
}
