use gpui_kit::*;
use rust_embed::RustEmbed;
use std::borrow::Cow;

/// Dodo's asset source, embedded from `./assets`.
///
/// The GPUI Kit asset bundle is intentionally not linked. The small set of
/// upstream-compatible SVGs Dodo needs lives beside Dodo's own icons, so every
/// path this application resolves remains explicit and owned here.
#[derive(RustEmbed)]
#[folder = "./assets"]
#[include = "icons/**/*.svg"]
#[include = "themes/**/*.json"]
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path.is_empty() {
            return Ok(None);
        }

        if let Some(file) = Self::get(path) {
            return Ok(Some(file.data));
        }

        Ok(None)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(Self::iter()
            .filter_map(|p| p.starts_with(path).then(|| p.into()))
            .collect())
    }
}

impl Assets {
    /// The vendored theme files under `assets/themes`, as raw JSON.
    ///
    /// Each file is a `ThemeSet` (one or more named themes); see
    /// `crate::settings::init` for how they reach the `ThemeRegistry`.
    pub fn themes() -> impl Iterator<Item = (SharedString, Cow<'static, [u8]>)> {
        Self::iter().filter_map(|path| {
            path.starts_with("themes/")
                .then(|| Self::get(&path).map(|file| (path.clone().into(), file.data)))
                .flatten()
        })
    }
}
