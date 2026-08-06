use std::path::PathBuf;
use std::time::SystemTime;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ScanContext {
    pub started_at: SystemTime,
    pub user_home: Option<PathBuf>,
}

impl ScanContext {
    pub fn new() -> Self {
        Self {
            started_at: SystemTime::now(),
            user_home: std::env::var_os("HOME").map(PathBuf::from),
        }
    }
}
