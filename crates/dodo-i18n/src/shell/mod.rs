//! The shell around the tools: the Settings dialog, the sidebar and the
//! Features page.
//!
//! `en` and `vi` each render every variant below; the compiler names any
//! string a language has not been given.

pub(crate) mod en;
pub(crate) mod vi;

#[cfg(test)]
pub(crate) mod samples;

/// The strings this area owns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Text {
    // Settings dialog.
    Settings,
    General,
    Appearance,
    Language,
    LanguageDescription,
    Theme,
    ThemeDescription,
    FontSize,
    FontSizeDescription,
    BorderRadius,
    BorderRadiusDescription,
    Large,
    Medium,
    Small,
    SearchSettingsPlaceholder,
    NoSettingsMatch,

    // Sidebar.
    Tools,
    JsonFormatterTitle,
    EncoderDecoderTitle,
    ApiExplorerTitle,
    CleanerTitle,
    DiagramTitle,

    // API Explorer — the consent gate and its setting.
    RunScripts,
    RunScriptsDescription,

    // The in-app updater: the sidebar affordance and the dialog.
    CheckForUpdates,

    // Database Explorer. Product names — PostgreSQL, SQLite — are proper nouns
    // and live in `database::models::engine`, untranslated, the same treatment
    // "Dodo" gets. Identifiers a server reports (a table's name, a column's
    // type) are data and never reach this enum at all.
    DatabaseTitle,

    // Quick navigation: the settings page, and what a jump reports.
    QuickNavigation,
    QuickNavEnabled,
    QuickNavEnabledDescription,
    QuickNavGateDescription,
    QuickNavShapeDescription,
    QuickNavStorageProblem,

    // Session restoration: what `session.json` can go wrong with.
    SessionStorageProblem,

    // The Features settings page: which tools the sidebar lists, and in what
    // order.
    Features,
    FeaturesDescription,
    FeatureShowInSidebar,
    FeatureDragToReorder,
    FeatureMoveUp,
    FeatureMoveDown,

    // The Input method tool's sidebar title; the pane's text lives in the
    // `input_method` area.
    InputMethod,

    // Close-to-tray and OS startup.
    StartWithOs,
    StartWithOsDescription,
    StartWithOsChecking,
    StartWithOsStatusUnknown,
}
