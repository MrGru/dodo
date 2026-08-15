//! The English column of the shell.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::Settings => "Settings".into(),
        Text::General => "General".into(),
        Text::Appearance => "Appearance".into(),
        Text::Language => "Language".into(),
        Text::LanguageDescription => {
                "The language used for the app's own labels.".into()
            }
        Text::Theme => "Theme".into(),
        Text::ThemeDescription => {
                "The colour scheme of the whole app.".into()
            }
        Text::FontSize => "Font size".into(),
        Text::FontSizeDescription => "The base text size of the app.".into(),
        Text::BorderRadius => "Border radius".into(),
        Text::BorderRadiusDescription => {
                "How rounded buttons, inputs and panels are.".into()
            }
        Text::Large => "Large".into(),
        Text::Medium => "Medium".into(),
        Text::Small => "Small".into(),
        Text::SearchSettingsPlaceholder => {
                "Search settings, then press Enter to jump".into()
            }
        Text::NoSettingsMatch => "No setting matches that search.".into(),
        Text::Tools => "Tools".into(),
        Text::JsonFormatterTitle => "Json formatter".into(),
        Text::EncoderDecoderTitle => "Encoder / Decoder".into(),
        Text::ApiExplorerTitle => "API Explorer".into(),
        Text::CleanerTitle => "Cleaner".into(),
        Text::RunScripts => "Run scripts".into(),
        Text::RunScriptsDescription => {
                "Whether the API Explorer runs the scripts a request carries. A script that \
                 arrived in an imported collection is code from someone else."
                    .into()
            }
        Text::CheckForUpdates => "Check for updates".into(),
        Text::DatabaseTitle => "Database".into(),
        Text::QuickNavigation => "Quick navigation".into(),
        Text::QuickNavEnabled => "Paste to navigate".into(),
        Text::QuickNavEnabledDescription => {
                "With no input focused, Cmd+V, Ctrl+V or p reads the clipboard and opens the tool \
                 that can handle it. Press Esc inside an input to leave it first."
                    .into()
            }
        Text::QuickNavGateDescription => {
                "Optional. dodo already has a real parser for this format and uses it; a pattern \
                 here only narrows what is offered to it. Leave it empty to try the parser on \
                 everything."
                    .into()
            }
        Text::QuickNavShapeDescription => {
                "The shape a candidate must have. Leave it empty for the built-in one; either way \
                 the text still has to decode before dodo will jump."
                    .into()
            }
        Text::QuickNavStorageProblem => "Saved settings".into(),
        Text::SessionStorageProblem => "Saved session".into(),
        Text::Features => "Features".into(),
        Text::FeaturesDescription => {
                "Choose which tools the sidebar lists, and in what order. Drag a row by its \
                 handle, or use the arrows."
                    .into()
            }
        Text::FeatureShowInSidebar => "Show in the sidebar".into(),
        Text::FeatureDragToReorder => "Drag to reorder".into(),
        Text::FeatureMoveUp => "Move up".into(),
        Text::FeatureMoveDown => "Move down".into(),
        Text::InputMethod => "Input method".into(),
        Text::StartWithOs => "Start with OS".into(),
        Text::StartWithOsDescription => {
                "Start Dodo in the tray when you sign in. macOS requires macOS 13+ and a bundled Dodo.app; Windows adds a per-user Startup Apps entry.".into()
            }
        Text::StartWithOsChecking => "Checking status…".into(),
        Text::StartWithOsStatusUnknown => "Status unavailable".into(),
    }
}
