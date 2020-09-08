use druid::kurbo::{Circle, Point, Rect, Size};
use druid::widget::{
    Button, CrossAxisAlignment, Flex, Label, LabelText, Parse, RadioGroup, Stepper, Tabs, TextBox,
    WidgetExt,
};
use druid::{
    theme, AppLauncher, BoxConstraints, Color, Data, Env, Event, EventCtx, LayoutCtx, Lens,
    LifeCycle, LifeCycleCtx, PaintCtx, UpdateCtx, Widget, WindowDesc,
};
use druid_table::{
    AnimationCurve, AnimationDirection, AnimationId, Animator, AxisName, BandScale,
    BandScaleFactory, CellRenderExt, DatumId, DrawableAxis, F64Range, LinearScale, LogIdx, Mark,
    MarkId, MarkOverrides, MarkProps, MarkShape, OffsetSource, SeriesId, SimpleCurve, StateName,
    TextMark, Vis, VisEvent, Visualization,
};
use im::Vector;
use std::collections::HashMap;

#[macro_use]
extern crate im;
use druid::widget::prelude::RenderContext;
use rand::rngs::ThreadRng;
use rand::Rng;
use std::time::Duration;

fn main_widget() -> impl Widget<AnimState> {
    use SimpleCurve::*;
    fn group<T: Data>(t: impl Into<LabelText<T>>, w: impl Widget<T> + 'static) -> impl Widget<T> {
        Flex::column()
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .with_child(
                Label::new(t)
                    .padding(5.)
                    .background(theme::PLACEHOLDER_COLOR)
                    .expand_width(),
            )
            .with_child(w)
            .border(Color::WHITE, 0.5)
            .padding(5.)
    }

    let controls = Flex::column()
        .with_child(group(
            "Curve",
            RadioGroup::new(vec![
                ("Linear", Linear),
                ("EaseIn", EaseIn),
                ("EaseOut", EaseOut),
                ("EaseInOut", EaseInOut),
                ("OutElastic", OutElastic),
                ("OutBounce", OutBounce),
                ("OutSine", OutSine),
            ])
            .lens(AnimState::curve),
        ))
        .with_child(group(
            "Duration ms",
            Parse::new(TextBox::new())
                .expand_width()
                .lens(AnimState::duration),
        ))
        .with_child(
            group(
                "Direction",
                RadioGroup::new(vec![
                    ("Normal", AnimationDirection::Normal),
                    ("Reverse", AnimationDirection::Reverse),
                    ("Alternate", AnimationDirection::Alternate),
                    ("AlternateReverse", AnimationDirection::AlternateReverse),
                ]),
            )
            .lens(AnimState::direction),
        )
        .with_child(
            group("Repeat", Parse::new(TextBox::new()))
                .expand_width()
                .lens(AnimState::repeat_limit),
        )
        .with_child(
            Button::new("Animate size").on_click(|_, state: &mut AnimState, _| {
                state.toggle_size = !state.toggle_size;
            }),
        )
        .with_flex_spacer(1.)
        .fix_width(200.0);

    Flex::row()
        .with_child(controls)
        .with_flex_child(AnimatedWidget::default(), 1.)
}

fn main() {
    let main_window = WindowDesc::new(main_widget)
        .title("Animation")
        .window_size((800.0, 500.0));

    // create the initial app state
    let initial_state = AnimState {
        curve: SimpleCurve::Linear,
        duration: Some(1000),
        direction: AnimationDirection::Normal,
        toggle_size: false,
        repeat_limit: Some(1),
    };

    // start the application
    AppLauncher::with_window(main_window)
        .use_simple_logger()
        .launch(initial_state)
        .expect("Failed to launch application");
}

#[derive(Clone, Data, Lens)]
struct AnimState {
    curve: SimpleCurve,
    duration: Option<usize>,
    direction: AnimationDirection,
    toggle_size: bool,
    repeat_limit: Option<usize>,
}

struct DrawState {
    circle: Circle,
    color: Color,
    max_radius: f64,
}

impl Default for DrawState {
    fn default() -> Self {
        Self {
            circle: Default::default(),
            color: Color::rgb(0xFF, 0, 0),
            max_radius: 0.0,
        }
    }
}

#[derive(Default)]
struct AnimatedWidget {
    animator: Animator,
    ids: (Option<AnimationId>, Option<AnimationId>),
    draw: DrawState,
}

impl AnimatedWidget {
    fn animate_size(&mut self, data: &AnimState) {
        self.ids.0 = Some(
            self.animator
                .new()
                .curve(data.curve)
                .repeat_limit(data.repeat_limit)
                .direction(data.direction)
                .duration(Duration::from_millis(1000))
                .id(),
        );
    }
}

impl Widget<AnimState> for AnimatedWidget {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut AnimState, env: &Env) {}

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &AnimState,
        env: &Env,
    ) {
        if let LifeCycle::WidgetAdded = event {
            self.animate_size(data);
            ctx.request_anim_frame()
        } else if let LifeCycle::AnimFrame(nanos) = event {
            // State split
            let (rad, alpha) = self.ids;
            let draw = &mut self.draw;
            let animator = &mut self.animator;

            animator.advance_by(*nanos as f64, |anim_ctx| {
                anim_ctx.with_animation(rad, |anim_ctx| {
                    draw.circle.radius = anim_ctx.progress() * draw.max_radius
                });
                anim_ctx.with_animation(alpha, |anim_ctx| {
                    draw.color = draw.color.clone().with_alpha(1. - anim_ctx.progress())
                })
            });

            ctx.request_paint();

            if self.animator.running() {
                ctx.request_anim_frame();
            }
        }
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: &AnimState, data: &AnimState, env: &Env) {
        if old_data.toggle_size != data.toggle_size {
            self.animate_size(data);
            ctx.request_anim_frame();
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &AnimState,
        env: &Env,
    ) -> Size {
        let size = bc.max();
        self.draw.circle.center = Point::new(size.width / 2., size.height / 2.);
        self.draw.max_radius = size.width.min(size.height) / 2.;
        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &AnimState, env: &Env) {
        ctx.fill(self.draw.circle, &self.draw.color)
    }
}
