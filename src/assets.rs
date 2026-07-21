use anyhow::anyhow;
use gpui::*;
use rust_embed::RustEmbed;
use std::borrow::Cow;

/// An asset source that loads assets from the `./assets` folder.
///
/// Anything not found here falls back to [`gpui_component_assets::Assets`], the
/// icon set `gpui_component::IconName` is generated from — library widgets
/// (dropdown carets, menu check marks, the dialog close button) request those
/// paths without us naming them, so the fallback keeps them from silently
/// rendering blank. Icons we ship ourselves shadow the library's by path.
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

        gpui_component_assets::Assets
            .load(path)
            .map_err(|_| anyhow!("could not find asset at path \"{path}\""))
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
