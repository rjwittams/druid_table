use druid::kurbo::Circle;
use druid::piet::RadialGradient;
use druid::widget::prelude::*;
use druid::widget::{Flex, Padding, Label, TextBox};
use druid_table::{Scroll, ScrollParent, ScrollOffsetWrapper};
use druid::{AppLauncher, Data, Insets, LocalizedString, Rect, WindowDesc, WidgetExt};

pub fn main() {
    let window = WindowDesc::new(build_widget)
        .title(LocalizedString::new("scroll-demo-window-title").with_placeholder("Scroll demo"));
    AppLauncher::with_window(window)
        .use_simple_logger()
        .launch("Thing".into())
        .expect("launch failed");
}

fn build_widget() -> impl Widget<String> {
    let mut row = Flex::row();

    for i in 0..4 {
        let mut col = Flex::column();

        for j in 0..100 {
            if i == j {
                col.add_child(Padding::new(3.0,
                                           TextBox::new()));
            } else {
                col.add_child(Padding::new(3.0,
                     Label::new(move |d: &String, _env: &_| { format!("Label {}, {}, {}", i, j, d) })));
            };
        }
        let scr = Scroll::new(col);

        row.add_child(scr);
    }
    ScrollParent::new(row)
}