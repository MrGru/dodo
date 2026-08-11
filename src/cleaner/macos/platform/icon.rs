use std::path::Path;

use objc2_app_kit::NSWorkspace;
use objc2_foundation::NSString;

/// Looks up a bundle icon while the Cleaner scanner is already off the UI
/// thread. TIFF is directly understood by GPUI's asynchronous image decoder.
pub fn application_icon_tiff(path: &Path) -> Option<Vec<u8>> {
    let path = path.to_str()?;
    NSWorkspace::sharedWorkspace()
        .iconForFile(&NSString::from_str(path))
        .TIFFRepresentation()
        .map(|data| data.to_vec())
}
