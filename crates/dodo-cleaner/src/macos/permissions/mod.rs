use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSString, NSURL};

use crate::core::permissions::{PermissionError, PermissionService, PermissionState};
use crate::macos::platform;

pub fn default_service() -> Arc<dyn PermissionService> {
    Arc::new(MacPermissionService)
}

struct MacPermissionService;

impl PermissionService for MacPermissionService {
    fn check_full_disk_access(&self) -> Result<PermissionState, PermissionError> {
        check_full_disk_access_for(std::env::var_os("HOME").map(PathBuf::from))
    }

    fn trigger_tcc_registration(&self) -> Result<(), PermissionError> {
        let _ = self.check_full_disk_access()?;
        Ok(())
    }

    fn open_full_disk_access_settings(&self) -> Result<(), PermissionError> {
        let url = NSURL::URLWithString(&NSString::from_str(
            "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles",
        ))
        .ok_or_else(|| PermissionError::CheckFailed("invalid Settings deep link".into()))?;
        if NSWorkspace::sharedWorkspace().openURL(&url) {
            Ok(())
        } else {
            Err(PermissionError::CheckFailed(
                "could not open Full Disk Access settings".into(),
            ))
        }
    }

    fn reveal_application_bundle(&self) -> Result<(), PermissionError> {
        let path = std::env::current_exe()
            .map_err(|error| PermissionError::CheckFailed(error.to_string()))?;
        let reveal = bundle_root(path.as_path()).unwrap_or(path);
        platform::reveal_in_finder(reveal.as_path()).map_err(PermissionError::CheckFailed)
    }
}

fn check_full_disk_access_for(home: Option<PathBuf>) -> Result<PermissionState, PermissionError> {
    let Some(home) = home else {
        return Ok(PermissionState::Unknown);
    };
    let probes = [
        home.join("Library").join("Mail"),
        home.join("Library").join("Safari"),
        home.join("Library").join("Containers"),
    ];

    let mut saw_existing_probe = false;
    let mut saw_access_denied = false;
    let mut saw_granted = false;

    for probe in probes {
        match probe_access(probe.as_path()) {
            ProbeOutcome::Missing => {}
            ProbeOutcome::Granted => {
                saw_existing_probe = true;
                saw_granted = true;
            }
            ProbeOutcome::Denied => {
                saw_existing_probe = true;
                saw_access_denied = true;
            }
            ProbeOutcome::Unknown(error) => {
                return Err(PermissionError::CheckFailed(format!(
                    "{}: {error}",
                    probe.display()
                )));
            }
        }
    }

    if saw_granted {
        Ok(PermissionState::Granted)
    } else if saw_access_denied {
        Ok(PermissionState::Denied)
    } else if saw_existing_probe {
        Ok(PermissionState::Restricted)
    } else {
        Ok(PermissionState::Unknown)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ProbeOutcome {
    Missing,
    Granted,
    Denied,
    Unknown(String),
}

fn probe_access(path: &Path) -> ProbeOutcome {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return ProbeOutcome::Missing,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            return ProbeOutcome::Denied;
        }
        Err(error) => return ProbeOutcome::Unknown(error.to_string()),
    };

    if metadata.is_dir() {
        match fs::read_dir(path) {
            Ok(mut entries) => {
                let _ = entries.next();
                ProbeOutcome::Granted
            }
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                ProbeOutcome::Denied
            }
            Err(error) => ProbeOutcome::Unknown(error.to_string()),
        }
    } else {
        match File::open(path) {
            Ok(_) => ProbeOutcome::Granted,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                ProbeOutcome::Denied
            }
            Err(error) => ProbeOutcome::Unknown(error.to_string()),
        }
    }
}

fn bundle_root(executable: &Path) -> Option<PathBuf> {
    let mut ancestors = executable.ancestors();
    let _ = ancestors.next();
    let macos = ancestors.next()?;
    let contents = ancestors.next()?;
    let bundle = ancestors.next()?;
    if macos.file_name().is_some_and(|name| name == "MacOS")
        && contents.file_name().is_some_and(|name| name == "Contents")
        && bundle
            .extension()
            .is_some_and(|extension| extension == "app")
    {
        Some(bundle.to_path_buf())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::{ProbeOutcome, bundle_root, check_full_disk_access_for, probe_access};
    use crate::core::permissions::PermissionState;

    #[test]
    fn missing_probes_yield_unknown() {
        let temp =
            std::env::temp_dir().join(format!("dodo-cleaner-permission-{}", std::process::id()));
        fs::create_dir_all(&temp).expect("creates temp home");
        assert_eq!(
            check_full_disk_access_for(Some(temp.clone())).expect("checks"),
            PermissionState::Unknown
        );
        fs::remove_dir_all(&temp).expect("removes temp home");
    }

    #[test]
    fn accessible_probe_yields_granted() {
        let temp = std::env::temp_dir().join(format!(
            "dodo-cleaner-permission-granted-{}",
            std::process::id()
        ));
        let mail = temp.join("Library").join("Mail");
        fs::create_dir_all(&mail).expect("creates mail probe");
        fs::write(mail.join("probe"), b"x").expect("writes probe");
        assert_eq!(
            check_full_disk_access_for(Some(temp.clone())).expect("checks"),
            PermissionState::Granted
        );
        fs::remove_dir_all(&temp).expect("removes temp home");
    }

    #[test]
    fn probe_access_grants_directory_reads() {
        let temp = std::env::temp_dir().join(format!("dodo-cleaner-probe-{}", std::process::id()));
        fs::create_dir_all(&temp).expect("creates temp dir");
        assert_eq!(probe_access(temp.as_path()), ProbeOutcome::Granted);
        fs::remove_dir_all(&temp).expect("removes temp dir");
    }

    #[test]
    fn bundle_root_detects_app_layout() {
        let bundle = bundle_root(Path::new("/Applications/Dodo.app/Contents/MacOS/dodo"));
        assert_eq!(bundle, Some(PathBuf::from("/Applications/Dodo.app")));
    }
}
