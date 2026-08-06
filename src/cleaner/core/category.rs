#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CleanerSection {
    SmartCare,
    Cleanup,
    Applications,
    Advanced,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Ord, PartialOrd)]
pub enum CleanerCategory {
    SystemJunk,
    UserCache,
    MailFiles,
    TrashBins,
    LargeOldFiles,
    InstalledApps,
    OrphanedFiles,
    AiApps,
    XcodeJunk,
    HomebrewCache,
    NodeToolingCache,
    DockerCache,
    UniversalBinaries,
    LanguageFiles,
}

impl CleanerSection {
    pub const ALL: [CleanerSection; 4] = [
        CleanerSection::SmartCare,
        CleanerSection::Cleanup,
        CleanerSection::Applications,
        CleanerSection::Advanced,
    ];
}

impl CleanerCategory {
    pub const ALL: [CleanerCategory; 14] = [
        CleanerCategory::SystemJunk,
        CleanerCategory::UserCache,
        CleanerCategory::MailFiles,
        CleanerCategory::TrashBins,
        CleanerCategory::LargeOldFiles,
        CleanerCategory::InstalledApps,
        CleanerCategory::OrphanedFiles,
        CleanerCategory::AiApps,
        CleanerCategory::XcodeJunk,
        CleanerCategory::HomebrewCache,
        CleanerCategory::NodeToolingCache,
        CleanerCategory::DockerCache,
        CleanerCategory::UniversalBinaries,
        CleanerCategory::LanguageFiles,
    ];

    pub fn section(self) -> CleanerSection {
        match self {
            CleanerCategory::SystemJunk
            | CleanerCategory::UserCache
            | CleanerCategory::MailFiles
            | CleanerCategory::TrashBins
            | CleanerCategory::LargeOldFiles => CleanerSection::Cleanup,
            CleanerCategory::InstalledApps | CleanerCategory::OrphanedFiles => {
                CleanerSection::Applications
            }
            CleanerCategory::AiApps
            | CleanerCategory::XcodeJunk
            | CleanerCategory::HomebrewCache
            | CleanerCategory::NodeToolingCache
            | CleanerCategory::DockerCache
            | CleanerCategory::UniversalBinaries
            | CleanerCategory::LanguageFiles => CleanerSection::Advanced,
        }
    }

    pub fn categories_for(section: CleanerSection) -> impl Iterator<Item = CleanerCategory> {
        CleanerCategory::ALL
            .into_iter()
            .filter(move |category| category.section() == section)
    }
}
