//! One sample per [`Text`] variant, for the language tests.
//!
//! `samples!` also emits an exhaustive `match` over [`Text`], so a variant
//! with no entry here is a compile error.

use crate::tests::{Sample, plain};

use super::Text;

samples! {
    plain Settings;
    plain General;
    plain Appearance;
    plain Language;
    plain LanguageDescription;
    plain Theme;
    plain ThemeDescription;
    plain FontSize;
    plain FontSizeDescription;
    plain BorderRadius;
    plain BorderRadiusDescription;
    plain Large;
    plain Medium;
    plain Small;
    plain SearchSettingsPlaceholder;
    plain NoSettingsMatch;
    plain Tools;
    plain JsonFormatterTitle;
    plain EncoderDecoderTitle;
    plain ApiExplorerTitle;
    plain CleanerTitle;
    plain DiagramTitle;
    plain RunScripts;
    plain RunScriptsDescription;
    plain CheckForUpdates;
    plain DatabaseTitle;
    plain QuickNavigation;
    plain QuickNavEnabled;
    plain QuickNavEnabledDescription;
    plain QuickNavGateDescription;
    plain QuickNavShapeDescription;
    plain QuickNavStorageProblem;
    plain SessionStorageProblem;
    plain Features;
    plain FeaturesDescription;
    plain FeatureShowInSidebar;
    plain FeatureDragToReorder;
    plain FeatureMoveUp;
    plain FeatureMoveDown;
    plain InputMethod;
    plain StartWithOs;
    plain StartWithOsDescription;
    plain StartWithOsChecking;
    plain StartWithOsStatusUnknown;
}
