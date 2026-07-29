//! Reading the files a request sends, and guessing what they are.
//!
//! # Where this runs
//!
//! This is the **only** part of request assembly that touches the filesystem,
//! and it is reached exclusively through [`prepare`], which
//! `state::tab::RequestTabState::send` runs on the background executor. Nothing
//! here may be called from a render or from a click handler: a file on a slow
//! volume would stall the frame.
//!
//! [`prepare`]: crate::api_explorer::services::http::prepare
//!
//! # Why the whole file is read into memory
//!
//! `reqwest::blocking` can stream a body, but the transport is a
//! `Transport::execute` that returns once and the retry, timing and size
//! reporting around it all assume a finished buffer. Reading the file whole
//! keeps that contract; [`MAX_UPLOAD_BYTES`] is what stops a mistaken pick of a
//! disk image from becoming a swap storm. A streaming upload path is a real
//! feature — it changes `Transport` — and is deliberately not smuggled in here.

use std::fs;
use std::path::Path;

use crate::api_explorer::services::TransportError;

/// The largest file this build will put in a request body.
///
/// 64 MiB is far above any realistic form upload and far below the point where
/// buffering it costs a user their session. Over it the request fails naming
/// the file and the limit, which is the honest answer; silently truncating an
/// upload would corrupt what the server stores.
pub const MAX_UPLOAD_BYTES: u64 = 64 * 1024 * 1024;

/// What a file with no recognised extension is sent as. The RFC 2046 default,
/// and what every HTTP client falls back to.
pub const OCTET_STREAM: &str = "application/octet-stream";

/// The extensions worth recognising, mapped to the media type a server expects.
///
/// Deliberately a short hand-written table rather than a mime-guessing
/// dependency: the wrong answer here is only ever a `Content-Type` a server may
/// re-sniff anyway, and the list covers what people actually attach to an API
/// request. Extensions are matched lowercased.
const MEDIA_TYPES: &[(&str, &str)] = &[
    ("json", "application/json"),
    ("xml", "application/xml"),
    ("txt", "text/plain"),
    ("csv", "text/csv"),
    ("html", "text/html"),
    ("htm", "text/html"),
    ("css", "text/css"),
    ("js", "text/javascript"),
    ("md", "text/markdown"),
    ("yaml", "application/yaml"),
    ("yml", "application/yaml"),
    ("pdf", "application/pdf"),
    ("zip", "application/zip"),
    ("gz", "application/gzip"),
    ("tar", "application/x-tar"),
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("svg", "image/svg+xml"),
    ("ico", "image/x-icon"),
    ("bmp", "image/bmp"),
    ("mp3", "audio/mpeg"),
    ("wav", "audio/wav"),
    ("mp4", "video/mp4"),
    ("webm", "video/webm"),
    ("mov", "video/quicktime"),
    ("wasm", "application/wasm"),
];

/// The media type `path`'s extension implies, or [`OCTET_STREAM`].
pub fn media_type_of(path: &Path) -> &'static str {
    let Some(extension) = path.extension().and_then(|ext| ext.to_str()) else {
        return OCTET_STREAM;
    };
    let extension = extension.to_ascii_lowercase();
    MEDIA_TYPES
        .iter()
        .find(|(candidate, _)| *candidate == extension)
        .map_or(OCTET_STREAM, |(_, media_type)| *media_type)
}

/// The name a multipart part or a `Content-Disposition` should carry for
/// `path`.
///
/// A path with no final component — `/`, or an empty string — falls back to
/// `file`, because a part with no filename reads to some servers as a text
/// part.
pub fn file_name_of(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("file")
        .to_string()
}

/// Reads `path` whole, refusing anything past [`MAX_UPLOAD_BYTES`].
///
/// The size is checked from the directory entry before a byte is read, so an
/// absurd pick costs one `stat` rather than a long read that is then thrown
/// away. Both failures name the path: a saved request whose file has since
/// moved is the common case, and "which file?" is the only question the user
/// will have.
pub fn read_file(path: &Path) -> Result<Vec<u8>, TransportError> {
    let metadata = fs::metadata(path).map_err(|err| TransportError::FileUnreadable {
        path: path.display().to_string(),
        detail: err.to_string(),
    })?;

    if metadata.len() > MAX_UPLOAD_BYTES {
        return Err(TransportError::FileTooLarge {
            path: path.display().to_string(),
            limit_mb: MAX_UPLOAD_BYTES / (1024 * 1024),
        });
    }

    fs::read(path).map_err(|err| TransportError::FileUnreadable {
        path: path.display().to_string(),
        detail: err.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{MAX_UPLOAD_BYTES, OCTET_STREAM, file_name_of, media_type_of, read_file};
    use crate::api_explorer::services::TransportError;

    /// A scratch file that removes itself. Named after the test so parallel
    /// runs cannot collide.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str, bytes: &[u8]) -> Self {
            let path = std::env::temp_dir().join(format!("dodo-upload-test-{name}"));
            std::fs::write(&path, bytes).expect("scratch file is writable");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn a_known_extension_names_its_media_type() {
        assert_eq!(media_type_of(Path::new("/tmp/a.png")), "image/png");
        assert_eq!(media_type_of(Path::new("/tmp/a.json")), "application/json");
        // Case is not part of an extension.
        assert_eq!(media_type_of(Path::new("/tmp/A.JPEG")), "image/jpeg");
    }

    #[test]
    fn anything_unrecognised_is_octet_stream() {
        assert_eq!(media_type_of(Path::new("/tmp/a.qqq")), OCTET_STREAM);
        assert_eq!(media_type_of(Path::new("/tmp/no-extension")), OCTET_STREAM);
        // A dotfile is a name, not an extension.
        assert_eq!(media_type_of(Path::new("/tmp/.gitignore")), OCTET_STREAM);
    }

    #[test]
    fn the_file_name_is_the_last_component() {
        assert_eq!(file_name_of(Path::new("/a/b/photo.png")), "photo.png");
        assert_eq!(file_name_of(Path::new("photo.png")), "photo.png");
        assert_eq!(file_name_of(Path::new("/")), "file");
        assert_eq!(file_name_of(Path::new("")), "file");
    }

    #[test]
    fn a_file_is_read_whole() {
        let scratch = Scratch::new("read-whole", b"abc\x00def");
        assert_eq!(read_file(scratch.path()).expect("reads"), b"abc\x00def");
    }

    #[test]
    fn a_missing_file_names_itself_rather_than_sending_nothing() {
        let missing = std::env::temp_dir().join("dodo-upload-test-does-not-exist");
        match read_file(&missing).expect_err("a missing file cannot be sent") {
            TransportError::FileUnreadable { path, detail } => {
                assert_eq!(path, missing.display().to_string());
                assert!(!detail.is_empty(), "the OS reason was dropped");
            }
            other => panic!("expected FileUnreadable, got {other:?}"),
        }
    }

    #[test]
    fn a_directory_is_reported_rather_than_read() {
        // `metadata` succeeds on a directory, so this exercises the read arm.
        let directory = std::env::temp_dir();
        assert!(matches!(
            read_file(&directory),
            Err(TransportError::FileUnreadable { .. })
        ));
    }

    #[test]
    fn the_cap_is_stated_in_the_units_the_message_uses() {
        assert_eq!(MAX_UPLOAD_BYTES / (1024 * 1024), 64);
    }
}
