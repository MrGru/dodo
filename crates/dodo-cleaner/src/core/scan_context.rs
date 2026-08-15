use std::ffi::OsString;
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
            user_home: resolve_home(std::env::var_os("HOME"), std::env::var_os("USERPROFILE")),
        }
    }
}

/// `HOME` is unset for an ordinary Windows session — Windows uses
/// `USERPROFILE` instead, and nothing sets `HOME` for a GUI app launched the
/// normal way. Every Windows-only scanner root (Downloads, the browser cache
/// roots, the per-user Recycle Bin) is built from [`ScanContext::user_home`],
/// so without this fallback it would always resolve to `None` there and every
/// one of those roots would silently vanish. `HOME` still wins when both are
/// set, matching every other platform.
fn resolve_home(home: Option<OsString>, user_profile: Option<OsString>) -> Option<PathBuf> {
    home.or(user_profile).map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::resolve_home;

    #[test]
    fn home_is_preferred_when_both_are_set() {
        assert_eq!(
            resolve_home(
                Some(OsString::from("/Users/ada")),
                Some(OsString::from("C:\\Users\\ada"))
            ),
            Some(std::path::PathBuf::from("/Users/ada"))
        );
    }

    #[test]
    fn user_profile_is_the_windows_fallback() {
        assert_eq!(
            resolve_home(None, Some(OsString::from("C:\\Users\\ada"))),
            Some(std::path::PathBuf::from("C:\\Users\\ada"))
        );
    }

    #[test]
    fn neither_set_is_none() {
        assert_eq!(resolve_home(None, None), None);
    }
}
