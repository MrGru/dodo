use std::path::PathBuf;

use crate::cleaner::core::category::CleanerCategory;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DeletionPolicy {
    pub allowed_roots: Vec<AllowedRoot>,
    pub protected_paths: Vec<PathBuf>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AllowedRoot {
    pub path: PathBuf,
    pub allow_root_itself: bool,
    pub allowed_categories: Vec<CleanerCategory>,
}
