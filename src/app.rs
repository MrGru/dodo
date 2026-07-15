use gpui::*;

use crate::layout::Layout;

pub struct DodoApp {
    layout: Entity<Layout>,
}

impl DodoApp {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            layout: cx.new(|_| Layout::new()),
        }
    }
}

impl Render for DodoApp {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.layout.clone()
    }
}
