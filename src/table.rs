use std::marker::PhantomData;
use std::ops::DerefMut;

use crate::TableSelection::NoSelection;
use druid::kurbo::Line;
use druid::piet::{FontBuilder, IntoBrush, PietFont, Text, TextLayout, TextLayoutBuilder};
use druid::widget::prelude::*;
use druid::widget::{Align, CrossAxisAlignment, Flex, Scroll, ScrollTo, SCROLL_TO};
use druid::{
    theme, Affine, BoxConstraints, Color, Data, Env, Event, EventCtx, KeyOrValue, LayoutCtx, Lens,
    LifeCycle, LifeCycleCtx, PaintCtx, Point, Rect, Selector, Size, UpdateCtx, Widget, WidgetExt,
};
use im::Vector;

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

pub struct LensWrapCR<U, L, W> {
    inner: W,
    lens: L,
    // The following is a workaround for otherwise getting E0207.
    phantom: PhantomData<U>,
}

impl<U, L, W> LensWrapCR<U, L, W> {
    fn new(inner: W, lens: L) -> LensWrapCR<U, L, W> {
        LensWrapCR {
            inner,
            lens,
            phantom: PhantomData::default(),
        }
    }
}

pub trait CellRenderExt<T: Data>: CellRender<T> + Sized + 'static {
    fn lens<S: Data, L: Lens<S, T>>(self, lens: L) -> LensWrapCR<T, L, Self> {
        LensWrapCR::new(self, lens)
    }
}

impl<T: Data, CR: CellRender<T> + 'static> CellRenderExt<T> for CR {}

impl<T, U, L, CR> CellRender<T> for LensWrapCR<U, L, CR>
where
    T: Data,
    U: Data,
    L: Lens<T, U>,
    CR: CellRender<U>,
{
    fn paint(&mut self, ctx: &mut PaintCtx, row_idx: usize, col_idx: usize, data: &T, env: &Env) {
        let inner = &mut self.inner;
        self.lens.with(data, |inner_data| {
            inner.paint(ctx, row_idx, col_idx, inner_data, env);
        })
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

        // Here's where we actually use the UI state
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

pub trait TableLen: Data {
    fn len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub trait TableRows<RowData: Data>: TableLen {
    fn use_row<V>(&self, idx: usize, f: impl FnOnce(&RowData) -> V) -> Option<V>;
}

impl<T: Data> TableLen for Vector<T> {
    fn len(&self) -> usize {
        self.len()
    }

    fn is_empty(&self) -> bool {
        self.is_empty()
    }
}

impl<RowData: Data> TableRows<RowData> for Vector<RowData> {
    fn use_row<V>(&self, idx: usize, f: impl FnOnce(&RowData) -> V) -> Option<V> {
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

#[derive(Clone)]
pub struct TableConfig {
    header_background: KeyOrValue<Color>,
    cells_background: KeyOrValue<Color>,
    cells_border: KeyOrValue<Color>,
    cell_border_thickness: KeyOrValue<f64>,
    cell_padding: KeyOrValue<f64>,
}

struct FixedSizeAxis {
    pixels_per_unit: f64,
    border: f64,
    len: usize,
}

impl FixedSizeAxis {
    fn new(pixels_per_unit: f64, border: f64, len: usize) -> FixedSizeAxis {
        FixedSizeAxis {
            pixels_per_unit,
            border,
            len,
        }
    }

    fn full_pixels_per_unit(&self) -> f64 {
        // TODO: Priv
        self.pixels_per_unit + self.border
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

    fn first_pixel_from_index(&self, idx: usize) -> f64 {
        (idx as f64) * self.full_pixels_per_unit()
    }

    fn pixels_for_idx(&self, _idx: usize) -> f64 {
        self.pixels_per_unit
    }
}

pub struct ResolvedTableConfig {
    rows: FixedSizeAxis,
    columns: FixedSizeAxis,
    header_background: Color,
    cells_background: Color,
    cells_border: Color,
    cell_border_thickness: f64,
    cell_padding: f64,
}

impl TableConfig {
    fn new() -> TableConfig {
        TableConfig {
            header_background: theme::BACKGROUND_LIGHT.into(),
            cells_background: theme::LABEL_COLOR.into(),
            cells_border: theme::BORDER_LIGHT.into(),
            cell_border_thickness: 1.0.into(),
            cell_padding: 2.0.into(),
        }
    }

    fn resolve(&self, rows: usize, columns: usize, env: &Env) -> ResolvedTableConfig {
        let border = self.cell_border_thickness.resolve(env);

        ResolvedTableConfig {
            rows: FixedSizeAxis::new(40., border, rows),
            columns: FixedSizeAxis::new(100., border, columns),
            header_background: self.header_background.resolve(env),
            cells_background: self.cells_background.resolve(env),
            cells_border: self.cells_border.resolve(env),
            cell_border_thickness: self.cell_border_thickness.resolve(env),
            cell_padding: self.cell_padding.resolve(env),
        }
    }
}

impl ResolvedTableConfig {
    fn cell_size(&self) -> Size {
        Size::new(self.columns.pixels_per_unit, self.rows.pixels_per_unit) // TODO: Size policies (measure or fixed).
                                                                           // Callers of this will need to delegate a lot more to handle measured cells.
    }

    fn find_cell(&self, pos: &Point) -> Option<SingleCell> {
        let (r, c) = self.find_cell_coords(pos);
        Some(SingleCell::new(r?, c?))
    }

    fn find_cell_coords(&self, pos: &Point) -> (Option<usize>, Option<usize>) {
        (
            self.rows.index_from_pixel(pos.y),
            self.columns.index_from_pixel(pos.x),
        )
    }

    fn ch_get_desired_size(&self) -> Size {
        Size::new(
            self.columns.total_pixel_length(),
            self.rows.full_pixels_per_unit(),
        )
    }
}

pub struct TableBuilder<RowData: Data, TableData: Data> {
    table_columns: Vec<TableColumn<RowData, Box<dyn CellRender<RowData>>>>,
    column_header_render: Box<dyn CellRender<String>>,
    table_config: TableConfig,
    phantom_td: PhantomData<TableData>,
}

impl<RowData: Data, TableData: TableRows<RowData>> Default for TableBuilder<RowData, TableData> {
    fn default() -> Self {
        Self::new()
    }
}

impl<RowData: Data, TableData: TableRows<RowData>> TableBuilder<RowData, TableData> {
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
        let column_names = self
            .table_columns
            .iter()
            .map(|tc| tc.header.clone())
            .collect();

        let column_headers_id = WidgetId::next();
        let column_headers_scroll_id = WidgetId::next();

        let headings = ColumnHeadings::new(
            self.table_config.clone(),
            column_names,
            self.column_header_render,
        )
        .with_id(column_headers_id);

        let ch_scroll = Scroll::new(headings).with_id(column_headers_scroll_id);
        let mut cells = Cells::new(
            self.table_config.clone(),
            self.table_columns.len(),
            self.table_columns,
        );
        cells.add_selection_handler(move |ctxt, table_sel| {
            let column_sel = table_sel.to_column_selection();
            ctxt.submit_command(SELECT_INDICES.with(column_sel), column_headers_id);
        });

        let mut cells_scroll = Scroll::new(cells);
        cells_scroll.add_scroll_handler(move |ctxt, pos| {
            ctxt.submit_command(SCROLL_TO.with(ScrollTo::x(pos.x)), column_headers_scroll_id);
        });

        Flex::column()
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .with_child(ch_scroll)
            .with_flex_child(cells_scroll, 1.)
            .center()
    }
}

pub struct ColumnHeadings<TableData: Data, ColumnHeader: Data, CR: CellRender<ColumnHeader>> {
    config: TableConfig,
    column_headers: Vec<ColumnHeader>,
    column_header_render: CR,
    selection: IndicesSelection,
    phantom_td: PhantomData<TableData>,
}

impl<TableData: Data, ColumnHeader: Data, CR: CellRender<ColumnHeader>>
    ColumnHeadings<TableData, ColumnHeader, CR>
{
    fn new(
        config: TableConfig,
        column_names: Vec<ColumnHeader>,
        column_header_render: CR,
    ) -> ColumnHeadings<TableData, ColumnHeader, CR> {
        ColumnHeadings {
            config,
            column_headers: column_names,
            column_header_render,
            selection: IndicesSelection::NoSelection,
            phantom_td: PhantomData::default(),
        }
    }
}

impl<TableData: TableLen, ColumnHeader: Data, CR: CellRender<ColumnHeader>> Widget<TableData>
    for ColumnHeadings<TableData, ColumnHeader, CR>
{
    fn event(&mut self, _ctx: &mut EventCtx, _event: &Event, _data: &mut TableData, _env: &Env) {
        if let Event::Command(ref cmd) = _event {
            if cmd.is(SELECT_INDICES) {
                if let Some(index_selections) = cmd.get(SELECT_INDICES) {
                    self.selection = index_selections.clone();
                    _ctx.request_paint()
                }
            }
        }
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut LifeCycleCtx,
        _event: &LifeCycle,
        _data: &TableData,
        _env: &Env,
    ) {
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
        data: &TableData,
        env: &Env,
    ) -> Size {
        bc.debug_check("ColumnHeadings");
        let rtc = self
            .config
            .resolve(data.len(), self.column_headers.len(), env);

        bc.constrain(rtc.ch_get_desired_size())
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &TableData, env: &Env) {
        let rect = ctx.region().to_rect();
        let rtc = self
            .config
            .resolve(data.len(), self.column_headers.len(), env);

        ctx.fill(rect, &rtc.header_background);

        let selected_border = Color::rgb(0xFF, 0, 0);
        let (start_col, end_col) = rtc.columns.index_range_from_pixels(rect.x0, rect.x1);

        let header_render = &mut self.column_header_render;

        for col_idx in start_col..=end_col {
            let cell_rect = Rect::from_origin_size(
                Point::new(rtc.columns.first_pixel_from_index(col_idx), 0.),
                Size::new(
                    rtc.columns.pixels_for_idx(col_idx),
                    rtc.rows.pixels_per_unit, // TODO separate column header height
                ),
            );
            let padded_rect = cell_rect.inset(-rtc.cell_padding);
            if let Some(col_name) = self.column_headers.get(col_idx) {
                ctx.with_save(|ctx| {
                    let layout_origin = padded_rect.origin().to_vec2();
                    ctx.clip(padded_rect);
                    ctx.transform(Affine::translate(layout_origin));
                    ctx.with_child_ctx(padded_rect, |ctxt| {
                        header_render.paint(ctxt, 0, col_idx, col_name, env);
                    });
                });
            }
            if self.selection.index_selected(col_idx) {
                ctx.stroke(padded_rect, &selected_border, 2.);
            } else {
                ctx.stroke_bottom_left_border(
                    rtc.cell_border_thickness,
                    &rtc.cells_border,
                    &cell_rect,
                );
            }
        }
    }
}

pub struct Cells<RowData: Data, TableData: Data, CR: CellRender<RowData>> {
    config: TableConfig,
    selection: TableSelection,
    selection_handlers: Vec<Box<SelectionHandler>>,
    columns: usize,
    cell_render: CR,
    phantom_rd: PhantomData<RowData>,
    phantom_td: PhantomData<TableData>,
}

impl<RowData: Data, TableData: Data, CR: CellRender<RowData>> Cells<RowData, TableData, CR> {
    fn new(config: TableConfig, columns: usize, cell_render: CR) -> Cells<RowData, TableData, CR> {
        Cells {
            config,
            selection_handlers: Vec::new(),
            selection: NoSelection,
            columns,
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
}

impl<RowData, TableData, CR> Widget<TableData> for Cells<RowData, TableData, CR>
where
    TableData: TableRows<RowData>,
    RowData: Data,
    CR: CellRender<RowData>,
{
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut TableData, env: &Env) {
        let mut new_selection: Option<TableSelection> = None;

        if let Event::MouseDown(me) = event {
            let rtc = self.config.resolve(data.len(), self.columns, env);
            if let Some(cell) = rtc.find_cell(&me.pos) {
                new_selection = Some(cell.into())
                // TODO: Modifier keys ask current selection to add this cell
            }
        }

        if let Some(sel) = new_selection {
            self.set_selection(ctx, sel);
        }
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut LifeCycleCtx,
        _event: &LifeCycle,
        _data: &TableData,
        _env: &Env,
    ) {
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
        data: &TableData,
        env: &Env,
    ) -> Size {
        bc.debug_check("TableCells");
        let table_config = self.config.resolve(data.len(), self.columns, env);
        bc.constrain(Size::new(
            table_config.columns.total_pixel_length(),
            table_config.rows.total_pixel_length(),
        ))
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &TableData, env: &Env) {
        let rtc = self.config.resolve(data.len(), self.columns, env);
        let rect = ctx.region().to_rect();

        ctx.fill(rect, &rtc.cells_background);

        let (start_row, end_row) = rtc.rows.index_range_from_pixels(rect.y0, rect.y1);
        let (start_col, end_col) = rtc.columns.index_range_from_pixels(rect.x0, rect.x1);

        for row_idx in start_row..=end_row {
            let row_top = rtc.rows.first_pixel_from_index(row_idx);
            let cell_size = rtc.cell_size();
            data.use_row(row_idx, |row| {
                for col_idx in start_col..=end_col {
                    let cell_left = rtc.columns.first_pixel_from_index(col_idx);
                    let selected = (&self.selection).get_cell_status(row_idx, col_idx);

                    let cell_rect =
                        Rect::from_origin_size(Point::new(cell_left, row_top), cell_size);
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
                            rtc.cell_border_thickness,
                            &rtc.cells_border,
                            &cell_rect,
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
        border_thickness: f64,
        border: &impl IntoBrush<Self>,
        cell_rect: &Rect,
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
