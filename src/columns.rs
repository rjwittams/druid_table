use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

use crate::data::SortDirection;
use druid::kurbo::Line;
use druid::piet::{FontBuilder, PietFont, Text, TextLayout, TextLayoutBuilder};
use druid::widget::prelude::*;
use druid::{theme, Color, Data, Env, KeyOrValue, Lens, PaintCtx};
use std::cmp::Ordering;
use std::fmt;
use std::fmt::{Debug, Formatter};

pub trait CellDelegate<RowData>: CellRender<RowData> + DataCompare<RowData> {}

impl<T> CellRender<T> for Box<dyn CellDelegate<T>> {
    fn init(&mut self, ctx: &mut PaintCtx, env: &Env) {
        self.deref_mut().init(ctx, env)
    }
    fn paint(&self, ctx: &mut PaintCtx, row_idx: usize, col_idx: usize, data: &T, env: &Env) {
        self.deref().paint(ctx, row_idx, col_idx, data, env);
    }
}

impl<T> DataCompare<T> for Box<dyn CellDelegate<T>> {
    fn compare(&self, a: &T, b: &T) -> Ordering {
        self.deref().compare(a, b)
    }
}

impl<RowData, T> CellDelegate<RowData> for T where T: CellRender<RowData> + DataCompare<RowData> {}

pub trait CellRender<T> {
    fn init(&mut self, ctx: &mut PaintCtx, env: &Env); // Use to cache resources like fonts
    fn paint(&self, ctx: &mut PaintCtx, row_idx: usize, col_idx: usize, data: &T, env: &Env);
}

impl<T, CR: CellRender<T>> CellRender<T> for Vec<CR> {
    fn init(&mut self, ctx: &mut PaintCtx, env: &Env) {
        for col in self {
            col.init(ctx, env)
        }
    }

    fn paint(&self, ctx: &mut PaintCtx, row_idx: usize, col_idx: usize, data: &T, env: &Env) {
        if let Some(cell_render) = self.get(col_idx) {
            cell_render.paint(ctx, row_idx, col_idx, data, env)
        }
    }
}

#[derive(Clone)]
pub struct Wrapped<T, U, W, I> {
    inner: I,
    wrapper: W,
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

pub trait DataCompare<Item> {
    fn compare(&self, a: &Item, b: &Item) -> Ordering;
}

impl<T, U, L, CR> CellRender<T> for LensWrapped<T, U, L, CR>
where
    T: Data,
    U: Data,
    L: Lens<T, U>,
    CR: CellRender<U>,
{
    fn init(&mut self, ctx: &mut PaintCtx, env: &Env) {
        self.0.inner.init(ctx, env)
    }

    fn paint(&self, ctx: &mut PaintCtx, row_idx: usize, col_idx: usize, data: &T, env: &Env) {
        let inner = &self.0.inner;
        self.0.wrapper.with(data, |inner_data| {
            inner.paint(ctx, row_idx, col_idx, inner_data, env);
        })
    }
}

impl<T, U, L, DC> DataCompare<T> for LensWrapped<T, U, L, DC>
where
    T: Data,
    U: Data,
    L: Lens<T, U>,
    DC: DataCompare<U>,
{
    fn compare(&self, a: &T, b: &T) -> Ordering {
        self.0.wrapper.with(a, |a| {
            self.0.wrapper.with(b, |b| self.0.inner.compare(a, b))
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
    fn init(&mut self, ctx: &mut PaintCtx, env: &Env) {
        self.0.inner.init(ctx, env)
    }

    fn paint(&self, ctx: &mut PaintCtx, row_idx: usize, col_idx: usize, data: &T, env: &Env) {
        let inner = &self.0.inner;
        let inner_data = (self.0.wrapper)(data);
        inner.paint(ctx, row_idx, col_idx, &inner_data, env);
    }
}

impl<T, U, F, DC> DataCompare<T> for FuncWrapped<T, U, F, DC>
where
    T: Data,
    U: Data,
    F: Fn(&T) -> U,
    DC: DataCompare<U>,
{
    fn compare(&self, a: &T, b: &T) -> Ordering {
        let a = (self.0.wrapper)(a);
        let b = (self.0.wrapper)(b);
        self.0.inner.compare(&a, &b)
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

    fn resolve_font(&self, ctx: &mut PaintCtx, env: &Env) -> PietFont {
        let font: PietFont = ctx
            .text()
            .new_font_by_name(self.font_name.resolve(env), self.font_size.resolve(env))
            .build()
            .unwrap();
        font
    }

    fn paint_impl(&self, ctx: &mut PaintCtx, data: &String, env: &Env, font: &PietFont) {
        let layout = ctx
            .text()
            .new_text_layout(font, &data, std::f64::INFINITY)
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

impl Default for TextCell {
    fn default() -> Self {
        TextCell::new()
    }
}

impl CellRender<String> for TextCell {
    fn init(&mut self, ctx: &mut PaintCtx, env: &Env) {
        if self.cached_font.is_none() {
            let font = self.resolve_font(ctx, env);
            self.cached_font = Some(font);
        }
    }

    fn paint(
        &self,
        ctx: &mut PaintCtx,
        _row_idx: usize,
        _col_idx: usize,
        data: &String,
        env: &Env,
    ) {
        if let Some(font) = &self.cached_font {
            self.paint_impl(ctx, &data, env, font);
        } else {
            log::warn!("Font not cached, are you missing a call to init");
            let font = self.resolve_font(ctx, env);
            ctx.stroke(
                Line::new((0., 0.), (100., 100.)),
                &Color::rgb(0xff, 0, 0),
                2.,
            );
            self.paint_impl(ctx, &data, env, &font);
        }
    }
}

impl DataCompare<String> for TextCell {
    fn compare(&self, a: &String, b: &String) -> Ordering {
        a.cmp(b)
    }
}

pub struct TableColumn<T: Data, CD: CellDelegate<T>> {
    pub(crate) header: String,
    cell_delegate: CD,
    pub(crate) width: TableColumnWidth,
    pub(crate) sort_order: Option<usize>,
    pub(crate) sort_fixed: bool,
    pub(crate) sort_dir: Option<SortDirection>,
    phantom_: PhantomData<T>,
}

impl<T: Data, CD: CellDelegate<T>> Debug for TableColumn<T, CD> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("TableColumn")
            .field("header", &self.header)
            .finish()
    }
}

pub struct TableColumnWidth {
    initial: Option<KeyOrValue<f64>>,
    min: Option<KeyOrValue<f64>>,
    max: Option<KeyOrValue<f64>>,
}

impl Default for TableColumnWidth {
    fn default() -> Self {
        TableColumnWidth {
            initial: Some(50.0.into()), // Could be in a 'theme' I guess.
            min: Some(20.0.into()),
            max: None,
        }
    }
}

impl From<f64> for TableColumnWidth {
    fn from(num: f64) -> Self {
        let mut tc = TableColumnWidth::default();
        tc.initial = Some(num.into());
        tc
    }
}

impl<T1, T2, T3> From<(T1, T2, T3)> for TableColumnWidth
where
    T1: Into<KeyOrValue<f64>>,
    T2: Into<KeyOrValue<f64>>,
    T3: Into<KeyOrValue<f64>>,
{
    fn from((initial, min, max): (T1, T2, T3)) -> Self {
        TableColumnWidth {
            initial: Some(initial.into()),
            min: Some(min.into()),
            max: Some(max.into()),
        }
    }
}

pub fn column<T: Data, CD: CellDelegate<T> + 'static>(
    header: impl Into<String>,
    cell_delegate: CD,
) -> TableColumn<T, Box<dyn CellDelegate<T>>> {
    TableColumn::new(header, Box::new(cell_delegate))
}

impl<T: Data, CD: CellDelegate<T>> TableColumn<T, CD> {
    pub fn new(header: impl Into<String>, cell_delegate: CD) -> Self {
        TableColumn {
            header: header.into(),
            cell_delegate,
            sort_order: Default::default(),
            sort_fixed: false,
            sort_dir: None,
            width: Default::default(),
            phantom_: PhantomData::default(),
        }
    }

    pub fn width<W: Into<TableColumnWidth>>(mut self, width: W) -> Self {
        self.width = width.into();
        self
    }

    pub fn sort<S: Into<SortDirection>>(mut self, sort: S) -> Self {
        self.sort_dir = Some(sort.into());
        self
    }

    pub fn fix_sort(mut self) -> Self {
        self.sort_fixed = true;
        self
    }
}

impl<T: Data, CR: CellDelegate<T>> CellRender<T> for TableColumn<T, CR> {
    fn init(&mut self, ctx: &mut PaintCtx, env: &Env) {
        self.cell_delegate.init(ctx, env)
    }

    fn paint(&self, ctx: &mut PaintCtx, row_idx: usize, col_idx: usize, data: &T, env: &Env) {
        self.cell_delegate.paint(ctx, row_idx, col_idx, data, env)
    }
}

impl<T: Data, CR: CellDelegate<T>> DataCompare<T> for TableColumn<T, CR> {
    fn compare(&self, a: &T, b: &T) -> Ordering {
        self.cell_delegate.compare(a, b)
    }
}
