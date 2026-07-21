use gpui::*;

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
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.layout.clone()
    }
}
