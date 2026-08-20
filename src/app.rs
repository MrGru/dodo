use gpui::*;
use gpui_component::Root;

use crate::layout::Layout;

pub struct DodoApp {
    layout: Entity<Layout>,
}

impl DodoApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            layout: cx.new(|cx| Layout::new(window, cx)),
        }
    }
}

impl Render for DodoApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // `Root` itself renders none of the overlay layers: `window.open_dialog`
        // only pushes onto `Root::active_dialogs`, and the dialog is built and
        // painted solely by `Root::render_dialog_layer`. That call belongs to
        // the first-level view under `Root` — us — so without it a dialog opens
        // in state but never appears on screen.
        let dialog_layer = Root::render_dialog_layer(window, cx);
        // Flow Canvas reports a refused/corrupt document and an unreadable
        // image through notifications. Like dialogs, pushing one only changes
        // Root's state; this layer is what actually paints it.
        let notification_layer = Root::render_notification_layer(window, cx);

        div()
            .size_full()
            .child(self.layout.clone())
            .children(dialog_layer)
            .children(notification_layer)
    }
}
