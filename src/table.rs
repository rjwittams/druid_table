use std::marker::PhantomData;
use std::ops::DerefMut;

use crate::TableSelection::NoSelection;
use druid::kurbo::Line;
use druid::piet::{FontBuilder, IntoBrush, PietFont, Text, TextLayout, TextLayoutBuilder};
use druid::widget::prelude::*;
use druid::widget::{Align, CrossAxisAlignment, Flex, Scroll, ScrollTo, SCROLL_TO};
use druid::{theme, Affine, BoxConstraints, Color, Data, Env, Event, EventCtx, KeyOrValue,
            LayoutCtx, Lens, LifeCycle, LifeCycleCtx, PaintCtx, Point, Rect, Selector,
            Size, UpdateCtx, Widget, WidgetExt, Cursor};
use im::Vector;
use std::collections::BTreeMap;
use float_ord::FloatOrd;

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

struct TableColumn<T: Data, CR: CellRender<T>> {
    header: String,
    cell_render: CR,
    phantom_: PhantomData<T>,
}

impl<T: Data, CR: CellRender<T>> CellRender<T> for TableColumn<T, CR> {
    fn paint(&mut self, ctx: &mut PaintCtx, row_idx: usize, col_idx: usize, data: &T, env: &Env) {
        self.cell_render.paint(ctx, row_idx, col_idx, data, env)
    }
}

pub trait ItemsLen {
    fn len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub trait ItemsUse: ItemsLen {
    type Item: Data;
    fn use_item<V>(&self, idx: usize, f: impl FnOnce(&Self::Item) -> V) -> Option<V>;
}

pub trait TableRows: ItemsUse + Data {}

impl<T> TableRows for T where T: ItemsUse + Data {}

impl<T: Clone> ItemsLen for Vector<T> {
    fn len(&self) -> usize {
        Vector::len(self)
    }
}

impl<RowData: Data> ItemsUse for Vector<RowData> {
    type Item = RowData;
    fn use_item<V>(&self, idx: usize, f: impl FnOnce(&RowData) -> V) -> Option<V> {
        self.get(idx).map(move |x| f(x))
    }
}

impl<T> ItemsLen for Vec<T> {
    fn len(&self) -> usize {
        Vec::len(self)
    }
}

impl<RowData: Data> ItemsUse for Vec<RowData> {
    type Item = RowData;
    fn use_item<V>(&self, idx: usize, f: impl FnOnce(&RowData) -> V) -> Option<V> {
        self.get(idx).map(move |x| f(x))
    }
}

#[derive(Debug, Clone)]
pub struct SingleCell {
    row: usize,
    col: usize,
}

impl SingleCell {
    fn new(row: usize, col: usize) -> SingleCell {
        SingleCell { row, col }
    }
}

#[derive(Debug, Clone)]
pub enum IndicesSelection {
    NoSelection,
    Single(usize),
    //Many(Vec<usize>),
    //Range(from, to)
}

impl IndicesSelection {
    fn index_selected(&self, idx: usize) -> bool {
        match self {
            IndicesSelection::Single(sel_idx) if *sel_idx == idx => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone)]
pub enum TableSelection {
    NoSelection,
    SingleCell(SingleCell),
    //  SingleColumn
    //  SingleRow
    //  Range
    //  Discontiguous
}

#[derive(Debug, PartialEq)]
pub enum SelectionStatus {
    NotSelected,
    Primary,
    AlsoSelected,
}

impl From<SelectionStatus> for bool {
    fn from(ss: SelectionStatus) -> Self {
        ss != SelectionStatus::NotSelected
    }
}

impl From<SingleCell> for TableSelection {
    fn from(sc: SingleCell) -> Self {
        TableSelection::SingleCell(sc)
    }
}

impl TableSelection {
    fn to_column_selection(&self) -> IndicesSelection {
        match self {
            TableSelection::NoSelection => IndicesSelection::NoSelection,
            TableSelection::SingleCell(SingleCell { col, .. }) => IndicesSelection::Single(*col),
        }
    }

    fn get_cell_status(&self, row_idx: usize, col_idx: usize) -> SelectionStatus {
        match self {
            TableSelection::SingleCell(sc) if row_idx == sc.row && col_idx == sc.col => {
                SelectionStatus::Primary
            }
            _ => SelectionStatus::NotSelected,
        }
    }
}

pub const SELECT_INDICES: Selector<IndicesSelection> =
    Selector::new("druid-builtin.table.select-indices");

type SelectionHandler = dyn Fn(&mut EventCtx, &TableSelection);


pub trait AxisMeasure: Clone {
    fn border(&self)->f64;
    fn set_axis_properties(&mut self, border: f64, len: usize);
    fn total_pixel_length(&self) -> f64;
    fn index_from_pixel(&self, pixel: f64) -> Option<usize>;
    fn index_range_from_pixels(&self, p0: f64, p1: f64) -> (usize, usize);
    fn first_pixel_from_index(&self, idx: usize) -> Option<f64>;
    fn pixels_length_for_index(&self, idx: usize) -> Option<f64>;
    fn set_far_pixel_for_idx(&mut self, idx: usize, pixel: f64) -> f64;
    fn set_pixel_length_for_idx(&mut self, idx: usize, length: f64) -> f64;
    fn can_resize(&self, idx: usize)->bool;

    fn pixel_near_border(&self, pixel: f64) -> Option<usize> {
        let idx = self.index_from_pixel(pixel)?;
        let idx_border_middle = self.first_pixel_from_index(idx).unwrap_or(0.) - self.border() / 2.;
        let next_border_middle = self.first_pixel_from_index(idx + 1).unwrap_or(self.total_pixel_length()) - self.border() / 2.;
        if f64::abs(pixel - idx_border_middle ) < MOUSE_MOVE_EPSILON {
            Some(idx)
        }else if f64::abs(pixel - next_border_middle ) < MOUSE_MOVE_EPSILON {
            Some(idx + 1)
        }else{
            None
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FixedSizeAxis {
    pixels_per_unit: f64,
    border: f64,
    len: usize,
}

impl FixedSizeAxis {
    pub fn new(pixels_per_unit: f64) -> Self {
        FixedSizeAxis {
            pixels_per_unit,
            border: 0.,
            len: 0,
        }
    }

    fn full_pixels_per_unit(&self) -> f64 {
        // TODO: Priv
        self.pixels_per_unit + self.border
    }
}

const MOUSE_MOVE_EPSILON: f64 = 3.;

impl AxisMeasure for FixedSizeAxis {
    fn border(&self)->f64 {
        self.border
    }

    fn set_axis_properties(&mut self, border: f64, len: usize) {
        self.border = border;
        self.len = len;
    }

    fn total_pixel_length(&self) -> f64 {
        self.full_pixels_per_unit() * (self.len as f64)
    }

    fn index_from_pixel(&self, pixel: f64) -> Option<usize> {
        let index = (pixel / self.full_pixels_per_unit()).floor() as usize;
        if index < self.len {
            Some(index)
        } else {
            None
        }
    }

    fn index_range_from_pixels(&self, p0: f64, p1: f64) -> (usize, usize) {
        let start = self.index_from_pixel(p0);
        let end = self.index_from_pixel(p1);

        let start = start.unwrap_or(0);
        let end = end.unwrap_or(self.len - 1);
        (start, end)
    }

    fn first_pixel_from_index(&self, idx: usize) -> Option<f64> {
        if idx < self.len {
            Some((idx as f64) * self.full_pixels_per_unit())
        }else{
            None
        }
    }

    fn pixels_length_for_index(&self, idx: usize) -> Option<f64> {
        if idx < self.len {
            Some(self.pixels_per_unit)
        }else{
            None
        }
    }

    fn set_far_pixel_for_idx(&mut self, _idx: usize, _pixel: f64) -> f64 {
        self.pixels_per_unit
    }

    fn set_pixel_length_for_idx(&mut self, _idx: usize, _length: f64) -> f64 {
        self.pixels_per_unit
    }

    fn can_resize(&self, _idx: usize) -> bool {
        false
    }
}

#[derive(Clone)]
struct StoredAxisMeasure{
    pixel_lengths: Vec<f64>,
    first_pixels : BTreeMap<usize, f64>,
    pixels_to_index: BTreeMap<FloatOrd<f64>, usize>,
    default_pixels: f64,
    border: f64,
    total_pixel_length: f64
}

impl StoredAxisMeasure{
    pub fn new(default_pixels: f64) -> Self {
        StoredAxisMeasure {
            pixel_lengths: Default::default(),
            first_pixels: Default::default(),
            pixels_to_index: Default::default(),
            default_pixels,
            border: 0.,
            total_pixel_length: 0.
        }
    }

    fn build_maps(&mut self) {
        let mut cur = 0.;
        self.first_pixels.clear();
        self.pixels_to_index.clear();
        for (idx, pixels) in self.pixel_lengths.iter().enumerate() {
            self.first_pixels.insert(idx, cur);
            self.pixels_to_index.insert(FloatOrd(cur), idx);
            cur += pixels + self.border;
        }
        self.total_pixel_length = cur;
    }
}


impl AxisMeasure for StoredAxisMeasure{
    fn border(&self) -> f64 {
        self.border
    }

    fn set_axis_properties(&mut self, border: f64, len: usize) {
        self.border = border;
        self.pixel_lengths = vec![self.default_pixels; len];
        // TODO: handle resize
        self.build_maps()
    }

    fn total_pixel_length(&self) -> f64 {
        self.total_pixel_length
    }

    fn index_from_pixel(&self, pixel: f64) -> Option<usize> {
        self.pixels_to_index.range( .. FloatOrd(pixel) ).next_back().map(|(_, v)| *v)
    }

    fn index_range_from_pixels(&self, p0: f64, p1: f64) -> (usize, usize) {
        let mut iter = self.pixels_to_index.range( FloatOrd(p0)..FloatOrd(p1) ).map(|(_,v)|*v);
        let (start, end) = (iter.next(), iter.next_back());

        let start = start.map(|i| if i == 0 {0 } else { i - 1} ).unwrap_or(0);
        let end = end.unwrap_or(self.pixel_lengths.len() - 1);
        (start, end)
    }

    fn first_pixel_from_index(&self, idx: usize) -> Option<f64> {
        self.first_pixels.get(&idx).copied()
    }

    fn pixels_length_for_index(&self, idx: usize) -> Option<f64> {
        self.pixel_lengths.get(idx).copied()
    }

    fn set_far_pixel_for_idx(&mut self, idx: usize, pixel: f64) -> f64 {
        let length = f64::max(0.,  pixel - *self.first_pixels.get(&idx).unwrap_or(&0.));
        self.set_pixel_length_for_idx(idx, length)
    }

    fn set_pixel_length_for_idx(&mut self, idx: usize, length: f64) -> f64 {
        self.pixel_lengths[idx] = length;
        self.build_maps(); // TODO : modify efficiently instead of rebuilding
        return length
    }

    fn can_resize(&self, _idx: usize) -> bool {
        true
    }
}


#[derive(Clone)]
pub struct TableConfig {
    pub header_height: KeyOrValue<f64>,
    pub header_background: KeyOrValue<Color>,
    pub cells_background: KeyOrValue<Color>,
    pub cells_border: KeyOrValue<Color>,
    pub cell_border_thickness: KeyOrValue<f64>,
    pub cell_padding: KeyOrValue<f64>,
}

pub struct ResolvedTableConfig {
    header_height: f64,
    header_background: Color,
    cells_background: Color,
    cells_border: Color,
    cell_border_thickness: f64,
    cell_padding: f64,
}

impl Default for TableConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl TableConfig {
    pub fn new() -> TableConfig {
        TableConfig {
            header_height: DEFAULT_HEADER_HEIGHT.into(),
            header_background: theme::BACKGROUND_LIGHT.into(),
            cells_background: theme::LABEL_COLOR.into(),
            cells_border: theme::BORDER_LIGHT.into(),
            cell_border_thickness: 1.0.into(),
            cell_padding: 2.0.into(),
        }
    }

    fn resolve(&self, env: &Env) -> ResolvedTableConfig {
        ResolvedTableConfig {
            header_height: self.header_height.resolve(env),
            header_background: self.header_background.resolve(env),
            cells_background: self.cells_background.resolve(env),
            cells_border: self.cells_border.resolve(env),
            cell_border_thickness: self.cell_border_thickness.resolve(env),
            cell_padding: self.cell_padding.resolve(env),
        }
    }
}

pub struct TableBuilder<RowData: Data, TableData: Data> {
    table_columns: Vec<TableColumn<RowData, Box<dyn CellRender<RowData>>>>,
    column_header_render: Box<dyn CellRender<String>>,
    table_config: TableConfig,
    phantom_td: PhantomData<TableData>,
}

impl<RowData: Data, TableData: TableRows<Item = RowData>> Default
    for TableBuilder<RowData, TableData>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<RowData: Data, TableData: TableRows<Item = RowData>> TableBuilder<RowData, TableData> {
    pub fn new() -> TableBuilder<RowData, TableData> {
        TableBuilder {
            table_columns: Vec::<TableColumn<RowData, Box<dyn CellRender<RowData>>>>::new(),
            column_header_render: Box::new(TextCell::new().text_color(theme::PRIMARY_LIGHT)),
            table_config: TableConfig::new(),
            phantom_td: PhantomData::default(),
        }
    }

    pub fn with_column<CR: CellRender<RowData> + 'static>(
        mut self,
        header: impl Into<String>,
        cell_render: CR,
    ) -> Self {
        self.add_column(header, cell_render);
        self
    }

    pub fn add_column<CR: CellRender<RowData> + 'static>(
        &mut self,
        header: impl Into<String>,
        cell_render: CR,
    ) {
        self.table_columns.push(TableColumn {
            header: header.into(),
            cell_render: Box::new(cell_render),
            phantom_: PhantomData::default(),
        });
    }

    pub fn build_widget(self) -> Align<TableData> {
        let column_headers: Vec<String> = self
            .table_columns
            .iter()
            .map(|tc| tc.header.clone())
            .collect();

        let column_measure = StoredAxisMeasure::new(100.);
        let row_measure = FixedSizeAxis::new(30.);
        build_table(
            column_headers,
            self.table_columns,
            row_measure,
            column_measure,
            self.column_header_render,
            self.table_config,
        )
    }
}

pub fn build_table<
    RowData: Data,
    TableData: TableRows<Item = RowData>,
    RowMeasure: AxisMeasure + 'static,
    ColumnHeader: Data,
    ColumnMeasure: AxisMeasure + 'static,
    ColumnHeaders: ItemsUse<Item = ColumnHeader> + 'static,
    ColumnHeaderRender: CellRender<ColumnHeader> + 'static,
    CellAreaRender: CellRender<RowData> + ItemsLen + 'static,
>(
    column_headers: ColumnHeaders,
    cell_area_render: CellAreaRender,
    row_measure: RowMeasure,
    column_measure: ColumnMeasure,
    column_header_render: ColumnHeaderRender,
    table_config: TableConfig,
) -> Align<TableData> {
    let column_headers_id = WidgetId::next();
    let column_headers_scroll_id = WidgetId::next();
    let cells_id = WidgetId::next();

    let mut headings = ColumnHeadings::new(
        table_config.clone(),
        column_measure.clone(),
        column_headers,
        column_header_render,
    );
    headings.add_axis_measure_adjustment_handler(move |ctx, adj|{
        log::info!("Column change {:?}", adj);
        ctx.submit_command( ADJUST_AXIS_MEASURE.with(*adj), cells_id);
    });

    let ch_scroll = Scroll::new(headings.with_id(column_headers_id) ).with_id(column_headers_scroll_id);
    let mut cells = Cells::new(table_config, column_measure.clone(), row_measure, cell_area_render);
    cells.add_selection_handler(move |ctx, table_sel| {
        let column_sel = table_sel.to_column_selection();
        ctx.submit_command(SELECT_INDICES.with(column_sel), column_headers_id);
    });

    let mut cells_scroll = Scroll::new(cells.with_id(cells_id));
    cells_scroll.add_scroll_handler(move |ctx, pos| {
        ctx.submit_command(SCROLL_TO.with(ScrollTo::x(pos.x)), column_headers_scroll_id);
    });

    Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .with_child(ch_scroll)
        .with_flex_child(cells_scroll, 1.)
        .center()
}

#[derive(Debug, Clone, Copy)]
pub enum TableAxis{
    Rows,
    Columns
}

#[derive(Debug, Clone, Copy)]
pub enum AxisMeasureAdjustment{
    LengthChanged(TableAxis, usize, f64)
}

pub const ADJUST_AXIS_MEASURE: Selector<AxisMeasureAdjustment> =
    Selector::new("druid-builtin.table.adjust-measure");

type AxisMeasureAdjustmentHandler = dyn Fn(&mut EventCtx, &AxisMeasureAdjustment);

pub struct ColumnHeadings<TableData, ColumnHeader, ColumnHeaders, Render, ColumnMeasure>
where
    TableData: Data,
    ColumnHeader: Data,
    ColumnHeaders: ItemsUse<Item = ColumnHeader>,
    Render: CellRender<ColumnHeader>,
    ColumnMeasure: AxisMeasure,
{
    config: TableConfig,
    resolved_config: Option<ResolvedTableConfig>,
    column_measure: ColumnMeasure,
    column_headers: ColumnHeaders,
    column_header_render: Render,
    dragging: Option<usize>,
    selection: IndicesSelection,
    phantom_td: PhantomData<TableData>,
    phantom_ch: PhantomData<ColumnHeader>,
    measure_adjustment_handlers: Vec<Box<AxisMeasureAdjustmentHandler>>
}

impl<TableData, ColumnHeader, ColumnHeaders, Render, ColumnMeasure>
    ColumnHeadings<TableData, ColumnHeader, ColumnHeaders, Render, ColumnMeasure>
where
    TableData: Data,
    ColumnHeader: Data,
    ColumnHeaders: ItemsUse<Item = ColumnHeader>,
    Render: CellRender<ColumnHeader>,
    ColumnMeasure: AxisMeasure,
{
    fn new(
        config: TableConfig,
        column_measure: ColumnMeasure,
        column_headers: ColumnHeaders,
        column_header_render: Render,
    ) -> ColumnHeadings<TableData, ColumnHeader, ColumnHeaders, Render, ColumnMeasure> {
        ColumnHeadings {
            config,
            resolved_config: None,
            column_measure,
            column_headers,
            column_header_render,
            dragging: None,
            selection: IndicesSelection::NoSelection,
            phantom_td: PhantomData::default(),
            phantom_ch: PhantomData::default(),
            measure_adjustment_handlers: Default::default()
        }
    }

    fn add_axis_measure_adjustment_handler(&mut self, handler: impl Fn(&mut EventCtx, &AxisMeasureAdjustment) + 'static){
        self.measure_adjustment_handlers.push(Box::new(handler))
    }

    fn set_column_width(&mut self, ctx: &mut EventCtx, idx: usize, pixel: f64) {
        let width = self.column_measure.set_far_pixel_for_idx(idx, pixel);
        let adjustment = AxisMeasureAdjustment::LengthChanged(TableAxis::Columns, idx, width);
        for handler in &self.measure_adjustment_handlers {
            (handler)(ctx, &adjustment)
        }
        ctx.request_layout();
    }

}

const DEFAULT_HEADER_HEIGHT: f64 = 25.0;


impl<TableData, ColumnHeader, ColumnHeaders, Render, ColumnMeasure> Widget<TableData>
    for ColumnHeadings<TableData, ColumnHeader, ColumnHeaders, Render, ColumnMeasure>
where
    TableData: Data,
    ColumnHeader: Data,
    ColumnHeaders: ItemsUse<Item = ColumnHeader>,
    Render: CellRender<ColumnHeader>,
    ColumnMeasure: AxisMeasure,
{
    fn event(&mut self, ctx: &mut EventCtx, _event: &Event, _data: &mut TableData, _env: &Env) {
        match _event {
            Event::Command(ref cmd) => {
                if cmd.is(SELECT_INDICES) {
                    if let Some(index_selections) = cmd.get(SELECT_INDICES) {
                        self.selection = index_selections.clone();
                        ctx.request_paint()
                    }
                }
            },
            Event::MouseMove(me) => {
                if let Some(idx) = self.dragging{
                    self.set_column_width(ctx,  idx, me.pos.x);
                    if me.buttons.is_empty(){
                        self.dragging = None;
                    }
                } else {
                    let mut cursor = &Cursor::Arrow;
                    if let Some(idx) = self.column_measure.pixel_near_border(me.pos.x) {
                        if idx > 0 && self.column_measure.can_resize(idx - 1) {
                            cursor = &Cursor::ResizeLeftRight;
                            ctx.set_handled()
                        }
                    }
                    ctx.set_cursor(cursor);
                }
            },
            Event::MouseDown(me) => {
                if let Some(idx) = self.column_measure.pixel_near_border(me.pos.x) {
                    if idx > 0 && self.column_measure.can_resize(idx - 1) {
                        self.dragging = Some(idx - 1)
                    }
                }
            },
            Event::MouseUp(me) => {
                if let Some(idx) = self.dragging{
                    self.set_column_width(ctx, idx, me.pos.x);
                    self.dragging = None;
                }
            }
            _ => {}
        }
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        _data: &TableData,
        _env: &Env,
    ) {
        if let LifeCycle::WidgetAdded = event {
            let rtc = self.config.resolve(_env);
            self.column_measure
                .set_axis_properties(rtc.cell_border_thickness, self.column_headers.len());
            self.resolved_config = Some(rtc);
        }
    }

    fn update(
        &mut self,
        _ctx: &mut UpdateCtx,
        _old_data: &TableData,
        _data: &TableData,
        _env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        _data: &TableData,
        _env: &Env,
    ) -> Size {
        bc.debug_check("ColumnHeadings");
        let height = if let Some(rc) = &self.resolved_config {
            rc.header_height
        } else {
            DEFAULT_HEADER_HEIGHT
        };

        bc.constrain(Size::new(self.column_measure.total_pixel_length(), height))
    }

    fn paint(&mut self, ctx: &mut PaintCtx, _data: &TableData, env: &Env) {
        if let Some(rtc) = &self.resolved_config {
            let rect = ctx.region().to_rect();

            ctx.fill(rect, &rtc.header_background);

            let selected_border = Color::rgb(0xFF, 0, 0);
            let (start_col, end_col) = self
                .column_measure
                .index_range_from_pixels(rect.x0, rect.x1);

            let header_render = &mut self.column_header_render;

            for col_idx in start_col..=end_col {
                let cell_rect = Rect::from_origin_size(
                    Point::new(self.column_measure.first_pixel_from_index(col_idx).unwrap_or(0.), 0.),
                    Size::new(
                        self.column_measure.pixels_length_for_index(col_idx).unwrap_or(0.),
                        rtc.header_height,
                    ),
                );
                let padded_rect = cell_rect.inset(-rtc.cell_padding);
                self.column_headers.use_item(col_idx, |col_name| {
                    ctx.with_save(|ctx| {
                        let layout_origin = padded_rect.origin().to_vec2();
                        ctx.clip(padded_rect);
                        ctx.transform(Affine::translate(layout_origin));
                        ctx.with_child_ctx(padded_rect, |ctxt| {
                            header_render.paint(ctxt, 0, col_idx, col_name, env);
                        });
                    });
                });
                if self.selection.index_selected(col_idx) {
                    ctx.stroke(padded_rect, &selected_border, 2.);
                } else {
                    ctx.stroke_bottom_left_border(
                        &cell_rect,
                        &rtc.cells_border,
                        rtc.cell_border_thickness,
                    );
                }
            }
        }
    }
}


pub struct Cells<RowData, TableData, Render, RowMeasure, ColumnMeasure>
where
    RowData: Data,
    TableData: Data,
    Render: CellRender<RowData> + ItemsLen,
    ColumnMeasure: AxisMeasure,
    RowMeasure: AxisMeasure,
{
    config: TableConfig,
    resolved_config: Option<ResolvedTableConfig>,
    selection: TableSelection,
    selection_handlers: Vec<Box<SelectionHandler>>,
    column_measure: ColumnMeasure,
    row_measure: RowMeasure,
    cell_render: Render,
    phantom_rd: PhantomData<RowData>,
    phantom_td: PhantomData<TableData>,
}

impl<RowData, TableData, Render, RowMeasure, ColumnMeasure>
    Cells<RowData, TableData, Render, RowMeasure, ColumnMeasure>
where
    RowData: Data,
    TableData: Data,
    Render: CellRender<RowData> + ItemsLen,
    ColumnMeasure: AxisMeasure,
    RowMeasure: AxisMeasure,
{
    fn new(
        config: TableConfig,
        column_measure: ColumnMeasure,
        row_measure: RowMeasure,
        cell_render: Render,
    ) -> Cells<RowData, TableData, Render, RowMeasure, ColumnMeasure> {
        Cells {
            config,
            resolved_config: None,
            selection_handlers: Vec::new(),
            selection: NoSelection,
            column_measure,
            row_measure,
            cell_render,
            phantom_rd: PhantomData::default(),
            phantom_td: PhantomData::default(),
        }
    }

    pub fn add_selection_handler(
        &mut self,
        selection_handler: impl Fn(&mut EventCtx, &TableSelection) + 'static,
    ) {
        self.selection_handlers.push(Box::new(selection_handler));
    }

    fn set_selection(&mut self, ctx: &mut EventCtx, selection: TableSelection) {
        self.selection = selection;
        for sh in &self.selection_handlers {
            sh(ctx, &self.selection)
        }
        ctx.request_paint();
    }

    fn find_cell(&self, pos: &Point) -> Option<SingleCell> {
        let (r, c) = (
            self.row_measure.index_from_pixel(pos.y),
            self.column_measure.index_from_pixel(pos.x),
        );
        Some(SingleCell::new(r?, c?))
    }
}

impl<RowData, TableData, Render, RowMeasure, ColumnMeasure> Widget<TableData>
    for Cells<RowData, TableData, Render, RowMeasure, ColumnMeasure>
where
    RowData: Data,
    TableData: TableRows<Item = RowData>,
    Render: CellRender<RowData> + ItemsLen,
    ColumnMeasure: AxisMeasure,
    RowMeasure: AxisMeasure,
{
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, _data: &mut TableData, _env: &Env) {
        let mut new_selection: Option<TableSelection> = None;

        match event {
            Event::MouseDown(me) => {
                if let Some(cell) = self.find_cell(&me.pos) {
                    new_selection = Some(cell.into())
                    // TODO: Modifier keys ask current selection to add this cell
                }
            }
            Event::Command(cmd) => {
                if cmd.is(ADJUST_AXIS_MEASURE){
                   if let Some(AxisMeasureAdjustment::LengthChanged(TableAxis::Columns, idx, length)) = cmd.get(ADJUST_AXIS_MEASURE){
                       self.column_measure.set_pixel_length_for_idx(*idx, *length);
                       ctx.request_layout();
                   }
                }
            }
            _=>()
        }

        if let Some(sel) = new_selection {
            self.set_selection(ctx, sel);
        }
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        _data: &TableData,
        _env: &Env,
    ) {
        if let LifeCycle::WidgetAdded = event {
            let rtc = self.config.resolve(_env);
            self.column_measure
                .set_axis_properties(rtc.cell_border_thickness, self.cell_render.len());
            self.row_measure
                .set_axis_properties(rtc.cell_border_thickness, _data.len());
            self.resolved_config = Some(rtc);
        }
    }

    fn update(
        &mut self,
        _ctx: &mut UpdateCtx,
        _old_data: &TableData,
        _data: &TableData,
        _env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        _data: &TableData,
        _env: &Env,
    ) -> Size {
        bc.debug_check("TableCells");
        bc.constrain(Size::new(
            self.column_measure.total_pixel_length(),
            self.row_measure.total_pixel_length(),
        ))
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &TableData, env: &Env) {
        let rtc = self.config.resolve(env);
        let rect = ctx.region().to_rect();

        ctx.fill(rect, &rtc.cells_background);

        let (start_row, end_row) = self.row_measure.index_range_from_pixels(rect.y0, rect.y1);
        let (start_col, end_col) = self
            .column_measure
            .index_range_from_pixels(rect.x0, rect.x1);

        for row_idx in start_row..=end_row {
            let row_top = self.row_measure.first_pixel_from_index(row_idx);

            data.use_item(row_idx, |row| {
                for col_idx in start_col..=end_col {
                    let cell_left = self.column_measure.first_pixel_from_index(col_idx);
                    let selected = (&self.selection).get_cell_status(row_idx, col_idx);

                    let cell_rect = Rect::from_origin_size(
                        Point::new(cell_left.unwrap_or(0.), row_top.unwrap_or(0.)),
                        Size::new(
                            self.column_measure.pixels_length_for_index(col_idx).unwrap_or(0.),
                            self.row_measure.pixels_length_for_index(row_idx).unwrap_or(0.),
                        ),
                    );
                    let padded_rect = cell_rect.inset(-rtc.cell_padding);

                    ctx.with_save(|ctx| {
                        let layout_origin = padded_rect.origin().to_vec2();
                        ctx.clip(padded_rect);
                        ctx.transform(Affine::translate(layout_origin));
                        ctx.with_child_ctx(padded_rect, |ctxt| {
                            self.cell_render.paint(ctxt, row_idx, col_idx, row, env);
                        });
                    });

                    if selected.into() {
                        ctx.stroke(
                            cell_rect,
                            &Color::rgb(0, 0, 0xFF),
                            rtc.cell_border_thickness,
                        );
                    } else {
                        ctx.stroke_bottom_left_border(
                            &cell_rect,
                            &rtc.cells_border,
                            rtc.cell_border_thickness,
                        );
                    }
                }
            });
        }
    }
}

trait TableRenderContextExt: RenderContext {
    fn stroke_bottom_left_border(
        &mut self,
        cell_rect: &Rect,
        border: &impl IntoBrush<Self>,
        border_thickness: f64,
    ) {
        self.stroke(
            Line::new(
                Point::new(cell_rect.x1, cell_rect.y0),
                Point::new(cell_rect.x1, cell_rect.y1),
            ),
            border,
            border_thickness,
        );
        self.stroke(
            Line::new(
                Point::new(cell_rect.x0, cell_rect.y1),
                Point::new(cell_rect.x1, cell_rect.y1),
            ),
            border,
            border_thickness,
        );
    }
}

impl<R: RenderContext> TableRenderContextExt for R {}

