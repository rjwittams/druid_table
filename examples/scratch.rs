use druid::kurbo as k;
use druid::widget::prelude::RenderContext;
use druid::Color;
use druid::{PaintCtx, Selector};
use std::any::{Any, TypeId};

enum AugmentationToken {
    Plain(),
}

trait Augmentation: 'static {}

trait AnemicWidget {}

trait Augmentable {
    fn get_augmentation<Aug: Augmentation>(&self) -> Option<&Aug>;
}

struct AugmentableWidget<W: AnemicWidget> {
    w: W,
}

impl<W: AnemicWidget> Augmentable for AugmentableWidget<W> {
    fn get_augmentation<Aug: Augmentation>(&self) -> Option<&Aug> {
        None
    }
}

struct Augmented<T, A: Augmentation> {
    t: T,
    a: A,
}

impl<T, A: Augmentation> Augmented<T, A> {
    pub fn new(t: T, a: A) -> Self {
        Augmented { t, a }
    }
}

impl<T, A: Augmentation> Augmented<T, A> {
    fn augment<A1: Augmentation>(self, a: A1) -> Augmented<Self, A1> {
        Augmented::new(self, a)
    }
}

impl<T: Augmentable, A: Augmentation> Augmentable for Augmented<T, A> {
    fn get_augmentation<Target: Augmentation>(&self) -> Option<&Target> {
        let aug = &self.a as &dyn Any;
        aug.downcast_ref::<Target>()
            .or_else(|| self.t.get_augmentation())
    }
}

trait ThingExt: AnemicWidget + Sized {
    fn augment<A: Augmentation>(self, a: A) -> Augmented<AugmentableWidget<Self>, A> {
        Augmented::new(AugmentableWidget { w: self }, a)
    }

    fn boxed(self) -> Box<dyn AnemicWidget>
    where
        Self: 'static,
    {
        Box::new(self)
    }
}

impl<T: AnemicWidget> ThingExt for T {}

#[derive(Default)]
struct Circle;
impl AnemicWidget for Circle {}

struct Draggable;
impl Augmentation for Draggable {}

struct FlexAmount(f64);
impl Augmentation for FlexAmount {}

struct TabName {
    name: String,
}

impl TabName {
    pub fn new(name: String) -> Self {
        TabName { name }
    }
}
impl Augmentation for TabName {}

fn main() {
    let just_tab = Circle::default().augment(TabName::new("Tab 1".into()));

    if let Some(t) = just_tab.get_augmentation::<TabName>() {
        println!("Tab name {}", t.name);
    }

    let tab_then_drag = Circle::default()
        .augment(TabName::new("Tab 2".into()))
        .augment(Draggable);

    let t_opt: Option<&TabName> = tab_then_drag.get_augmentation();
    if let Some(t) = t_opt {
        println!("Tab name {}", t.name);
    }

    let drag_then_tab = Circle::default()
        .augment(Draggable)
        .augment(TabName::new("Tab 3".into()));

    let t: &TabName = drag_then_tab.get_augmentation().unwrap();
    println!("Tab name {}", t.name);

    let tab_three = Circle::default()
        .augment(TabName::new("First".into()))
        .augment(TabName::new("Second".into()));

    let t: &TabName = tab_three.get_augmentation().unwrap();
    println!("Tab name {}", t.name);
}
