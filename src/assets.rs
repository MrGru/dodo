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

#[cfg(test)]
mod tests {
    // Import by name, not `use super::*`: this module's `gpui_kit::*` glob
    // re-exports gpui's `test` proc macro, which would shadow std's `#[test]`
    // and blow the recursion limit. See the `dodo-build-validate` skill.
    use super::Assets;

    /// gpui-component widgets resolve their own glyphs through *our* asset
    /// source: a checked `Checkbox` draws `IconName::Check`, whose `.path()` is
    /// `icons/check.svg`. Because dodo embeds only the SVGs it needs (see the
    /// `#[include]` above), a widget icon we don't ship comes back `None` and
    /// the glyph silently vanishes — the checked box with no tick. Each path
    /// here is a kit `IconName` a component dodo actually renders resolves at
    /// runtime: `check` (Checkbox / clipboard), `loader` (Spinner, and the
    /// loading `Button` that draws one), `undo-2` (the settings page reset
    /// button). Adding one to this list without adding the file must fail here,
    /// not on screen.
    #[test]
    fn kit_widget_icons_are_embedded() {
        for path in ["icons/check.svg", "icons/loader.svg", "icons/undo-2.svg"] {
            assert!(
                Assets::get(path).is_some(),
                "{path} is a gpui-component widget icon dodo renders but does \
                 not embed — copy it from gpui-kit-assets into assets/icons/"
            );
        }
    }
}
