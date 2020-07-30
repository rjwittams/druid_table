use std::marker::PhantomData;
use std::ops::DerefMut;

use druid::piet::{FontBuilder, PietFont, Text, TextLayout, TextLayoutBuilder};
use druid::widget::prelude::*;
use druid::{theme, Color, Data, Env, KeyOrValue, Lens, PaintCtx};

pub trait CellRender<T> {
    fn paint(&mut self, ctx: &mut PaintCtx, row_idx: usize, col_idx: usize, data: &T, env: &Env);
}

impl<T> CellRender<T> for Box<dyn CellRender<T>> {
    fn paint(&mut self, ctx: &mut PaintCtx, row_idx: usize, col_idx: usize, data: &T, env: &Env) {
        self.deref_mut().paint(ctx, row_idx, col_idx, data, env);
    }
}

impl<T, CR: CellRender<T>> CellRender<T> for Vec<CR> {
    fn paint(&mut self, ctx: &mut PaintCtx, row_idx: usize, col_idx: usize, data: &T, env: &Env) {
        if let Some(cell_render) = self.get_mut(col_idx) {
            cell_render.paint(ctx, row_idx, col_idx, data, env)
        }
    }
}

#[derive(Clone)]
pub struct Wrapped<T, U, W, I> {
    inner: I,
    wrapper: W,
    // The following is a workaround for otherwise getting E0207.
    phantom_u: PhantomData<U>,
    phantom_t: PhantomData<T>,
}

pub struct LensWrapped<T, U, W, I>(Wrapped<T, U, W, I>)
where
    W: Lens<T, U>;

#[derive(Clone)]
pub struct FuncWrapped<T, U, W, I>(Wrapped<T, U, W, I>)
where
    W: Fn(&T) -> U;

impl<T, U, W, I> Wrapped<T, U, W, I> {
    fn new(inner: I, wrapper: W) -> Wrapped<T, U, W, I> {
        Wrapped {
            inner,
            wrapper,
            phantom_u: PhantomData::default(),
            phantom_t: PhantomData::default(),
        }
    }
}

pub trait CellRenderExt<T: Data>: CellRender<T> + Sized + 'static {
    fn lens<S: Data, L: Lens<S, T>>(self, lens: L) -> LensWrapped<S, T, L, Self> {
        LensWrapped(Wrapped::new(self, lens))
    }

    fn on_result_of<S: Data, F: Fn(&S) -> T>(self, f: F) -> FuncWrapped<S, T, F, Self> {
        FuncWrapped(Wrapped::new(self, f))
    }
}

impl<T: Data, CR: CellRender<T> + 'static> CellRenderExt<T> for CR {}

impl<T, U, L, CR> CellRender<T> for LensWrapped<T, U, L, CR>
where
    T: Data,
    U: Data,
    L: Lens<T, U>,
    CR: CellRender<U>,
{
    fn paint(&mut self, ctx: &mut PaintCtx, row_idx: usize, col_idx: usize, data: &T, env: &Env) {
        let inner = &mut self.0.inner;
        self.0.wrapper.with(data, |inner_data| {
            inner.paint(ctx, row_idx, col_idx, inner_data, env);
        })
    }
}

impl<T, U, F, CR> CellRender<T> for FuncWrapped<T, U, F, CR>
where
    T: Data,
    U: Data,
    F: Fn(&T) -> U,
    CR: CellRender<U>,
{
    fn paint(&mut self, ctx: &mut PaintCtx, row_idx: usize, col_idx: usize, data: &T, env: &Env) {
        let inner = &mut self.0.inner;
        let inner_data = (self.0.wrapper)(data);
        inner.paint(ctx, row_idx, col_idx, &inner_data, env);
    }
}

#[derive(Clone)]
pub struct TextCell {
    text_color: KeyOrValue<Color>,
    font_name: KeyOrValue<&'static str>,
    font_size: KeyOrValue<f64>,
    cached_font: Option<PietFont>,
}

impl TextCell {
    pub fn new() -> Self {
        TextCell {
            text_color: Color::BLACK.into(),
            font_name: theme::FONT_NAME.into(),
            font_size: theme::TEXT_SIZE_NORMAL.into(),
            cached_font: None,
        }
    }

    pub fn text_color(mut self, text_color: impl Into<KeyOrValue<Color>>) -> TextCell {
        self.text_color = text_color.into();
        self
    }

    pub fn font_name(mut self, font_name: impl Into<KeyOrValue<&'static str>>) -> TextCell {
        self.font_name = font_name.into();
        self
    }

    pub fn font_size(mut self, font_size: impl Into<KeyOrValue<f64>>) -> TextCell {
        self.font_size = font_size.into();
        self
    }
}

impl Default for TextCell {
    fn default() -> Self {
        TextCell::new()
    }
}

impl CellRender<String> for TextCell {
    fn paint(
        &mut self,
        ctx: &mut PaintCtx,
        _row_idx: usize,
        _col_idx: usize,
        data: &String,
        env: &Env,
    ) {
        if self.cached_font.is_none() {
            let font: PietFont = ctx
                .text()
                .new_font_by_name(self.font_name.resolve(env), self.font_size.resolve(env))
                .build()
                .unwrap();
            self.cached_font = Some(font);
        }

        let layout = ctx
            .text()
            .new_text_layout(
                self.cached_font.as_ref().unwrap(),
                &data,
                std::f64::INFINITY,
            )
            .build()
            .unwrap();

        let fill_color = self.text_color.resolve(env);
        ctx.draw_text(
            &layout,
            (0.0, layout.line_metric(0).unwrap().height),
            &fill_color,
        );
    }
}

pub(crate) struct TableColumn<T: Data, CR: CellRender<T>> {
    pub(crate) header: String,
    cell_render: CR,
    phantom_: PhantomData<T>,
}

impl<T: Data, CR: CellRender<T>> TableColumn<T, CR> {
    pub fn new(header: String, cell_render: CR) -> Self {
        TableColumn {
            header,
            cell_render,
            phantom_: PhantomData::default(),
        }
    }
}

impl<T: Data, CR: CellRender<T>> CellRender<T> for TableColumn<T, CR> {
    fn paint(&mut self, ctx: &mut PaintCtx, row_idx: usize, col_idx: usize, data: &T, env: &Env) {
        self.cell_render.paint(ctx, row_idx, col_idx, data, env)
    }
}
