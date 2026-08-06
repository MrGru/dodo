use std::path::Path;

use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSArray, NSURL};

pub fn reveal_in_finder(path: &Path) -> Result<(), String> {
    let Some(url) = NSURL::from_path(path, path.is_dir(), None) else {
        return Err(format!("could not convert {} to file URL", path.display()));
    };
    let urls = NSArray::from_retained_slice(&[url]);
    NSWorkspace::sharedWorkspace().activateFileViewerSelectingURLs(&urls);
    Ok(())
}
