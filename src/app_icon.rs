use gpui::SharedString;
use gpui_component::{Icon, IconNamed};

/// The icons bundled under `assets/icons`.
///
/// Registering a variant here is also what makes the SVG reachable by path, so
/// a variant may exist purely to satisfy a gpui-component widget that asks for
/// the equivalent `IconName` (which resolves to the same `icons/<name>.svg`).
#[derive(Clone, Copy)]
pub enum AppIcon {
    /// The product mark, shown in the sidebar header. A flat silhouette rather
    /// than a Lucide outline on purpose: gpui paints an SVG as an alpha mask
    /// tinted with the element's text colour, so a brand mark reads at 16px
    /// only as solid coverage. Derived from `assets/branding`, which is
    /// packaged but never embedded — see `src/assets.rs`.
    Dodo,
    Binary,
    Json,
    Cleaner,
    Palette,
    /// Used by the Settings dialog's search box, via the library's
    /// `IconName::Search`.
    #[allow(dead_code)]
    Search,
    Settings,
    Sliders,
    PanelLeftClose,
    PanelLeftOpen,

    // API Explorer. The five below, plus `trash`, ship as our own SVGs; the
    // rest resolve through `Assets`' fallback to `gpui_component_assets`, which
    // already carries them — see `src/assets.rs`.
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
    /// The Delete action on every Docker list page (and the API Explorer's
    /// collection deletes). Ships as our own `icons/trash.svg`: the library's
    /// `delete.svg` — which this used to resolve to through `Assets`' fallback —
    /// draws a backspace key, not a waste bin, so a destructive row action read
    /// as "clear the field".
    Trash,
    /// A passed and a failed row in the API Explorer's Tests tab. Both resolve
    /// through `Assets`' fallback to `gpui_component_assets`.
    CircleCheck,
    CircleX,

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
    /// The Inspect placeholder action on the round-3 pages. Resolves through
    /// `Assets`' fallback to `gpui_component_assets`.
    Eye,

    /// The updater. Ships as our own `icons/download.svg`: the library carries
    /// no download glyph, and `arrow-down` reads as "sort descending" beside a
    /// list of tools.
    Download,

    // Database Explorer. `database`, `table`, `columns` and `key` ship as our
    // own SVGs — the library's icon set has no data-shaped glyph at all — and
    // the rest resolve through `Assets`' fallback to `gpui_component_assets`.
    /// The sidebar row, and a database node in the object tree.
    Database,
    /// The engine marks on a connection's root row in the object tree. Our own
    /// SVGs — the library has nothing product-shaped — and deliberately drawn
    /// as ordinary outline glyphs in the set's Lucide-ish style rather than as
    /// either vendor's registered logo: gpui paints an SVG as an alpha mask
    /// tinted with the element's text colour, so a two-colour brand mark could
    /// not survive the trip anyway, and a monochrome trace of one would be a
    /// trademark dodo has no licence to use.
    PostgreSql,
    Sqlite,
    /// A table node. Also the result grid's own empty state.
    Table,
    /// A column node, and the Columns group.
    Columns,
    /// A constraint node, and the Constraints group.
    Key,
    /// An index node. `sort-ascending` rather than a key: an index is an
    /// ordering, and the library has no better glyph for one.
    SortAscending,
    /// The password-storage notice, which is never hidden.
    Info,
    /// The other half of the password reveal toggle.
    EyeOff,

    /// The drag handle on each row of the Features settings page. Ships as our
    /// own `icons/grip-vertical.svg`: the library's set has no handle glyph at
    /// all, and every candidate in it (`ellipsis-vertical`, `menu`) already
    /// means something else in dodo. Six dots is the conventional mark for
    /// "this row can be dragged", and it survives being painted as an alpha
    /// mask because it is nothing but coverage.
    GripVertical,

    /// The Input method tool's sidebar row. Ships as our own
    /// `icons/keyboard.svg`: neither the library's set nor ours had a keyboard
    /// at all, and the two glyphs that were reached for instead each say
    /// something else — `globe` is the API Explorer's row, and `key` is a
    /// database constraint. A keyboard is what the thing *is*, so it survives
    /// being read at 16px on the collapsed rail with no label beside it.
    Keyboard,

    // Cleaner's section and category rows. All six resolve through `Assets`'
    // fallback to `gpui_component_assets` — no new SVG files were needed,
    // the vendored Lucide-ish set already had a glyph for each of these.
    /// The Smart Care section, and Docker's own Overview-style rows
    /// elsewhere: a scan-everything overview reads naturally as a dashboard.
    ChartPie,
    /// The Applications section.
    LayoutDashboard,
    /// The AI Apps category.
    Bot,
    /// The Node Tooling Cache category: npm/yarn/pnpm/bun are all
    /// terminal-driven tools, and the set has no package-manager glyph.
    SquareTerminal,
    /// The Universal Binaries category — an architecture is a CPU property.
    Cpu,
    /// The Installed Apps category's row icon, kept distinct from the
    /// Applications section's own `LayoutDashboard` above.
    Building2,
}

impl IconNamed for AppIcon {
    fn path(self) -> SharedString {
        match self {
            Self::Dodo => "icons/dodo.svg",
            Self::Binary => "icons/binary.svg",
            Self::Json => "icons/json.svg",
            Self::Cleaner => "icons/cleaner.svg",
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
            Self::Trash => "icons/trash.svg",
            Self::CircleCheck => "icons/circle-check.svg",
            Self::CircleX => "icons/circle-x.svg",
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
            Self::Eye => "icons/eye.svg",
            Self::Download => "icons/download.svg",
            Self::Database => "icons/database.svg",
            Self::PostgreSql => "icons/postgresql.svg",
            Self::Sqlite => "icons/sqlite.svg",
            Self::Table => "icons/table.svg",
            Self::Columns => "icons/columns.svg",
            Self::Key => "icons/key.svg",
            Self::SortAscending => "icons/sort-ascending.svg",
            Self::Info => "icons/info.svg",
            Self::EyeOff => "icons/eye-off.svg",
            Self::GripVertical => "icons/grip-vertical.svg",
            Self::Keyboard => "icons/keyboard.svg",
            Self::ChartPie => "icons/chart-pie.svg",
            Self::LayoutDashboard => "icons/layout-dashboard.svg",
            Self::Bot => "icons/bot.svg",
            Self::SquareTerminal => "icons/square-terminal.svg",
            Self::Cpu => "icons/cpu.svg",
            Self::Building2 => "icons/building-2.svg",
        }
        .into()
    }
}

impl AppIcon {
    pub fn view(self) -> Icon {
        Icon::new(self)
    }
}
