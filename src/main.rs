mod api_explorer;
mod app;
mod app_icon;
mod assets;
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
