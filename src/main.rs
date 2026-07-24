mod api_explorer;
mod app;
mod app_icon;
mod assets;
mod build_info;
mod docker;
mod encoder_decoder;
mod i18n;
/// Guards the rule that `i18n` only enforces halfway; test-only.
#[cfg(test)]
mod i18n_lint;
mod json_formatter;
mod layout;
mod settings;

use gpui::*;
use gpui_component::*;

use crate::{app::DodoApp, assets::Assets};

fn main() {
    if print_build_metadata_and_exit() {
        return;
    }

    let app = gpui_platform::application().with_assets(Assets);

    app.run(move |cx| {
        // This must be called before using any GPUI Component features.
        gpui_component::init(cx);
        // Registers the vendored themes; needs the registry `init` just created.
        settings::init(cx);
        // Binds the API Explorer's send shortcut. Like `settings::init`, it has
        // to run after `gpui_component::init` to win the key-binding tie.
        api_explorer::init(cx);
        // Binds the Docker list pages' keyboard navigation, scoped to the Docker
        // view. Same post-`init` ordering rule as the two above.
        docker::init(cx);

        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(900.), px(620.)), cx)),
            ..Default::default()
        };

        cx.spawn(async move |cx| {
            cx.open_window(window_options, |window, cx| {
                let view = cx.new(|cx| DodoApp::new(window, cx));
                // This first level on the window, should be a Root.
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("Failed to open window");
        })
        .detach();
    });
}

/// dodo's only command-line surface: `--version` / `-V` and `--build-info`
/// print the metadata `build.rs` embedded and exit, before any GPUI state or
/// window exists.
///
/// This is what lets CI prove a packaged binary actually runs — a GUI app
/// cannot be launched on a headless runner, so the release workflow executes
/// this path instead (see `docs/release.md` for exactly what that does and
/// does not prove).
///
/// Returns `true` when it handled the arguments and `main` should stop.
/// Anything else — no arguments, an unrecognised argument, the arguments macOS
/// passes to a bundled `.app` — falls through to the window, so normal launch
/// behaviour is unchanged.
fn print_build_metadata_and_exit() -> bool {
    match std::env::args().nth(1).as_deref() {
        Some("--version" | "-V") => println!("{}", build_info::VERSION_INFO.short()),
        Some("--build-info") => println!("{}", build_info::VERSION_INFO.long()),
        _ => return false,
    }
    true
}
