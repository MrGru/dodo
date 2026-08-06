use std::path::{Path, PathBuf};

use objc2_foundation::{NSFileManager, NSURL};

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TrashReceipt {
    pub original_path: PathBuf,
    pub trashed_path: Option<PathBuf>,
}

pub fn move_to_trash(path: &Path) -> Result<TrashReceipt, String> {
    let Some(url) = NSURL::from_path(path, path.is_dir(), None) else {
        return Err(format!("could not convert {} to file URL", path.display()));
    };
    let mut resulting_url = None;
    NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&url, Some(&mut resulting_url))
        .map_err(|err| err.to_string())?;
    Ok(TrashReceipt {
        original_path: path.to_path_buf(),
        trashed_path: resulting_url.and_then(|url| url.to_file_path()),
    })
}
