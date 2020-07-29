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
use std::cell::RefCell;
use std::rc::Rc;

pub trait CellRender<T> {
    fn paint(&mut self, ctx: &mut PaintCtx, row_idx: usize, col_idx: usize, data: &T, env: &Env);
}

impl<T> CellRender<T> for Box<dyn CellRender<T>> {
    fn paint(&mut self, ctx: &mut PaintCtx, row_idx: usize, col_idx: usize, data: &T, env: &Env) {
        self.deref_mut().paint(ctx, row_idx, col_idx, data, env);
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

pub struct Cells<RowData: Data, TableData: Data>(pub Rc<RefCell<TableConfig<RowData, TableData>>>);

pub trait TableRows<RowData: Data>: Data {
    fn len(&self) -> usize;
    fn use_row<V>(&self, idx: usize, f: impl FnOnce(&RowData) -> V) -> Option<V>;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<RowData: Data> TableRows<RowData> for Vector<RowData> {
    fn len(&self) -> usize {
        self.len()
    }

    fn use_row<V>(&self, idx: usize, f: impl FnOnce(&RowData) -> V) -> Option<V> {
        self.get(idx).map(move |x| f(x))
    }
}

#[derive(Debug)]
pub struct SingleCell {
    row: usize,
    col: usize,
}

impl SingleCell {
    fn new(row: usize, col: usize) -> SingleCell {
        SingleCell { row, col }
    }
}

#[derive(Debug)]
pub enum IndicesSelection {
    NoSelection,
    Single(usize),
    //Many(Vec<usize>),
    //Range(from, to)
}

#[derive(Debug)]
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
    AlsoSelected
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
    fn contains_column(&self, col_idx: usize) -> bool {
        match self {
            NoSelection => false,
            TableSelection::SingleCell(sc) => sc.col == col_idx,
        }
    }

    fn to_column_selection(&self) -> IndicesSelection {
        match self {
            TableSelection::NoSelection => IndicesSelection::NoSelection,
            TableSelection::SingleCell(SingleCell { col, .. }) => IndicesSelection::Single(*col),
        }
    }

    fn get_cell_status(&self, row_idx: usize, col_idx: usize)->SelectionStatus {
        match self{
            TableSelection::SingleCell(sc) if row_idx == sc.row && col_idx == sc.col => SelectionStatus::Primary,
            _=>SelectionStatus::NotSelected
        }
    }
}

pub const SELECT_INDICES: Selector<IndicesSelection> =
    Selector::new("druid-builtin.table.select-indices");

type SelectionHandler = dyn Fn(&mut EventCtx, &TableSelection);

pub struct TableConfig<RowData: Data, TableData: Data> {
    table_columns: Vec<TableColumn<RowData, Box<dyn CellRender<RowData>>>>,
    column_header_render: Box<dyn CellRender<String>>,
    header_background: KeyOrValue<Color>,
    cells_background: KeyOrValue<Color>,
    cells_border: KeyOrValue<Color>,
    cell_border_thickness: KeyOrValue<f64>,
    cell_padding: KeyOrValue<f64>,
    phantom_td: PhantomData<TableData>,
    selection: TableSelection,
    selection_handlers: Vec<Box<SelectionHandler>>,
}

impl<RowData: Data, TableData: TableRows<RowData>> Default for TableConfig<RowData, TableData> {
    fn default() -> Self {
        Self::new()
    }
}

impl<RowData: Data, TableData: TableRows<RowData>> TableConfig<RowData, TableData> {
    pub fn new() -> TableConfig<RowData, TableData> {
        TableConfig {
            table_columns: Vec::<TableColumn<RowData, Box<dyn CellRender<RowData>>>>::new(),
            column_header_render: Box::new(TextCell::new().text_color(theme::PRIMARY_LIGHT)),
            header_background: theme::BACKGROUND_LIGHT.into(),
            cells_background: theme::LABEL_COLOR.into(),
            cells_border: theme::BORDER_LIGHT.into(),
            cell_border_thickness: 1.0.into(),
            cell_padding: 2.0.into(),
            phantom_td: PhantomData::default(),
            selection: NoSelection,
            selection_handlers: Vec::new(),
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
        let shared_config = Rc::new(RefCell::new(self));

        let column_headers_id = WidgetId::next();
        let column_headers_scroll_id = WidgetId::next();

        let headings = ColumnHeadings::new(Rc::clone(&shared_config)).with_id(column_headers_id);

        let ch_scroll = Scroll::new(headings).with_id(column_headers_scroll_id);
        let mut cells_scroll = Scroll::new(Cells(Rc::clone(&shared_config)));
        cells_scroll.add_scroll_handler(move |ctxt, pos| {
            ctxt.submit_command(SCROLL_TO.with(ScrollTo::x(pos.x)), column_headers_scroll_id);
        });

        shared_config
            .borrow_mut()
            .add_selection_handler(move |ctxt, table_sel| {
                let column_sel = table_sel.to_column_selection();
                ctxt.submit_command(SELECT_INDICES.with(column_sel), column_headers_id);
            });

        Flex::column()
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .with_child(ch_scroll)
            .with_flex_child(cells_scroll, 1.)
            .center()
    }

    fn columns(&self) -> usize {
        self.table_columns.len()
    }

    fn cell_size(&self, _data: &TableData, _env: &Env) -> Size {
        Size::new(100., 40.) // TODO: Size policies (measure or fixed).
                             // Callers of this will need to delegate a lot more to handle measured cells.
    }

    //TODO: Measure content or fixed sizes per axis
    fn full_cell_size(&self, _data: &TableData, env: &Env) -> Size {
        let border_thickness = self.cell_border_thickness.resolve(env);
        let cs = self.cell_size(_data, env);
        Size::new(border_thickness + cs.width, border_thickness + cs.height)
    }

    fn find_cell(&self, data: &TableData, env: &Env, pos: &Point) -> Option<SingleCell> {
        let (r, c) = self.find_cell_coords(data, env, pos);
        Some(SingleCell::new(r?, c?))
    }

    fn find_cell_coords(
        &self,
        data: &TableData,
        env: &Env,
        pos: &Point,
    ) -> (Option<usize>, Option<usize>) {
        let cs = self.full_cell_size(data, env); //TODO: Need vectors of border positions
        let col = (pos.x / cs.width).floor() as usize;
        let row = (pos.y / cs.height).floor() as usize;
        (
            if row < data.len() { Some(row) } else { None },
            if col < self.columns() {
                Some(col)
            } else {
                None
            },
        )
    }

    fn set_selected(&mut self, ctx: &mut EventCtx, selection: TableSelection) {
        self.selection = selection;
        for sh in &self.selection_handlers {
            sh(ctx, &self.selection)
        }
    }

    pub fn add_selection_handler(
        &mut self,
        selection_handler: impl Fn(&mut EventCtx, &TableSelection) + 'static,
    ) {
        self.selection_handlers.push(Box::new(selection_handler));
    }
}

pub struct ColumnHeadings<RowData: Data, TableData: Data> {
    config: Rc<RefCell<TableConfig<RowData, TableData>>>,
}

impl<RowData: Data, TableData: Data> ColumnHeadings<RowData, TableData> {
    fn new(
        config: Rc<RefCell<TableConfig<RowData, TableData>>>,
    ) -> ColumnHeadings<RowData, TableData> {
        ColumnHeadings { config }
    }
}

impl<RowData: Data, TableData: TableRows<RowData>> Widget<TableData>
    for ColumnHeadings<RowData, TableData>
{
    fn event(&mut self, _ctx: &mut EventCtx, _event: &Event, _data: &mut TableData, _env: &Env) {
        if let Event::Command(ref cmd) = _event {
            if cmd.is(SELECT_INDICES) {
                if let Some(_sel) = cmd.get(SELECT_INDICES) {
                    // Todo store own selecions?
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
        let table_config: &TableConfig<RowData, TableData> = &self.config.borrow();
        let cell_size = table_config.full_cell_size(data, env);
        bc.constrain(Size::new(
            cell_size.width * (table_config.columns() as f64),
            cell_size.height,
        ))
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &TableData, env: &Env) {
        let rect = ctx.region().to_rect();
        let config: &mut TableConfig<RowData, TableData> = &mut self.config.borrow_mut();

        ctx.fill(rect, &config.header_background.resolve(env));

        let cell_size = config.cell_size(data, env);
        let border_thickness = config.cell_border_thickness.resolve(env);
        let border = config.cells_border.resolve(env);
        let selected_border = Color::rgb(0xFF, 0, 0);
        let padding = config.cell_padding.resolve(env);

        let (_, start_col) = config.find_cell_coords(data, env, &rect.origin());
        let (_, end_col) =
            config.find_cell_coords(data, env, &(rect.origin() + rect.size().to_vec2()));

        let start_col = start_col.unwrap_or(0);
        let end_col = end_col.unwrap_or(config.columns() - 1);

        let selection = &config.selection;
        let header_render = &mut config.column_header_render;

        let row_top = 0.;
        let mut cell_left = (start_col as f64) * (cell_size.width + border_thickness);
        for col_idx in start_col..=end_col {
            let cell_rect = Rect::from_origin_size(Point::new(cell_left, row_top), cell_size);
            let padded_rect = cell_rect.inset(-padding);
            if let Some(col) = config.table_columns.get_mut(col_idx) {
                ctx.with_save(|ctx| {
                    let layout_origin = padded_rect.origin().to_vec2();
                    ctx.clip(padded_rect);
                    ctx.transform(Affine::translate(layout_origin));
                    ctx.with_child_ctx(padded_rect, |ctxt| {
                        header_render.paint(ctxt, 0, col_idx, &col.header, env);
                    });
                });
                if selection.contains_column(col_idx) {
                    ctx.stroke(padded_rect, &selected_border, 2.);
                } else {
                    ctx.stroke_bottom_left_border(border_thickness, &border, &cell_rect);
                }
            }
            cell_left = cell_rect.x1 + border_thickness;
        }
    }
}

impl<RowData: Data, TableData: Data> Widget<TableData> for Cells<RowData, TableData>
where
    TableData: TableRows<RowData>,
{
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, _data: &mut TableData, _env: &Env) {
        if let Event::MouseDown(me) = event {
            let mut config = self.0.borrow_mut();

            if let Some(cell) = config.find_cell(_data, _env, &me.pos) {
                config.set_selected(ctx, cell.into());
                ctx.request_paint();
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
        bc.debug_check("TableCells");
        let table_config: &TableConfig<RowData, TableData> = &self.0.borrow();
        let cell_size = table_config.full_cell_size(data, env);
        bc.constrain(Size::new(
            cell_size.width * (table_config.columns() as f64),
            cell_size.height * (data.len() as f64),
        ))
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &TableData, env: &Env) {
        let mut config = self.0.borrow_mut();
        let rect = ctx.region().to_rect();

        ctx.fill(rect, &config.cells_background.resolve(env));

        let cell_size = config.cell_size(data, env);
        let border = config.cells_border.resolve(env);
        let border_thickness = config.cell_border_thickness.resolve(env);
        let padding = config.cell_padding.resolve(env);

        let (start_row, start_col) = config.find_cell_coords(data, env, &rect.origin());
        let (end_row, end_col) =
            config.find_cell_coords(data, env, &(rect.origin() + rect.size().to_vec2()));

        let start_row = start_row.unwrap_or(0);
        let end_row = end_row.unwrap_or(data.len() - 1);
        let start_col = start_col.unwrap_or(0);
        let end_col = end_col.unwrap_or(config.columns() - 1);

        let mut row_top = (start_row as f64) * (cell_size.height + border_thickness);

        for row_idx in start_row..=end_row {
            data.use_row(row_idx, |row| {
                let mut cell_left = (start_col as f64) * (cell_size.width + border_thickness);

                for col_idx in start_col..=end_col {
                    let selected = config.selection.get_cell_status(row_idx, col_idx);

                    let col = &mut config.table_columns[col_idx];
                    let cell_rect =
                        Rect::from_origin_size(Point::new(cell_left, row_top), cell_size);
                    let padded_rect = cell_rect.inset(-padding);

                    ctx.with_save(|ctx| {
                        let layout_origin = padded_rect.origin().to_vec2();
                        ctx.clip(padded_rect);
                        ctx.transform(Affine::translate(layout_origin));
                        ctx.with_child_ctx(padded_rect, |ctxt| {
                            col.cell_render.paint(ctxt, row_idx, col_idx, row, env);
                        });
                    });
                    if selected.into() {
                        ctx.stroke(cell_rect, &Color::rgb(0, 0, 0xFF), border_thickness);
                    } else {
                        ctx.stroke_bottom_left_border(border_thickness, &border, &cell_rect);
                    }

                    cell_left = cell_rect.x1 + border_thickness;
                }

                row_top += cell_size.height + border_thickness;
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
