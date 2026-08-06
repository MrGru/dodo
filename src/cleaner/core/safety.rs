//! Where deletion is allowed to reach, and where it must never.
//!
//! Both types are pending for the whole module: round 1 has no destructive
//! cleanup path, so nothing builds a policy to check against. The allow comes
//! off with the first deletion.
#![allow(dead_code)]

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
