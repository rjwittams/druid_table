use std::marker::PhantomData;

use druid::widget::prelude::*;
use druid::{
    Affine, BoxConstraints, Color, Data, Env, Event, EventCtx, KbKey, LayoutCtx, LifeCycle,
    LifeCycleCtx, PaintCtx, Point, Rect, Size, UpdateCtx, Widget,
};

use crate::axis_measure::{
    AxisMeasure, AxisMeasureAdjustment, AxisPair, LogIdx, TableAxis, VisIdx, VisOffset,
    ADJUST_AXIS_MEASURE,
};
use crate::columns::CellRender;
use crate::config::{ResolvedTableConfig, TableConfig};
use crate::data::{IndexedData, RemapSpec, Remapper};
use crate::headings::{HeaderClicked, HEADER_CLICKED};
use crate::render_ext::RenderContextExt;
use crate::selection::{
    CellDemap, SelectionHandler, SelectionStatus, SingleCell, SingleSlice, TableSelection,
    TableSelectionMod,
};
use crate::{IndexedItems, Remap};
use druid::platform_menus::win::file::new;
use std::iter::Map;
use std::ops::RangeInclusive;

pub trait CellsDelegate<TableData: IndexedData>:
    CellRender<TableData::Item> + Remapper<TableData>
where
    TableData::Item: Data,
{
    fn number_of_columns_in_data(&self, data: &TableData) -> usize;
}

pub struct Cells<RowData, TableData, CellDel, RowMeasure, ColumnMeasure>
where
    RowData: Data,
    TableData: IndexedData<Item = RowData, Idx = LogIdx>,
    CellDel: CellsDelegate<TableData>, // The length is the number of columns
    ColumnMeasure: AxisMeasure,
    RowMeasure: AxisMeasure,
{
    config: TableConfig,
    resolved_config: Option<ResolvedTableConfig>,
    selection: TableSelection,
    selection_handlers: Vec<Box<SelectionHandler>>,
    column_measure: ColumnMeasure,
    row_measure: RowMeasure,
    cell_delegate: CellDel,
    remap_spec_rows: RemapSpec,
    remap_rows: Remap,
    phantom_rd: PhantomData<RowData>,
    phantom_td: PhantomData<TableData>,
}

// A rect only makes sense in VisIdx - In LogIdx any list of points is possible due to remapping
#[derive(Debug)]
struct CellRect {
    start_row: VisIdx,
    end_row: VisIdx,
    start_col: VisIdx,
    end_col: VisIdx,
}

impl CellRect {
    fn new(
        (start_row, end_row): (VisIdx, VisIdx),
        (start_col, end_col): (VisIdx, VisIdx),
    ) -> CellRect {
        CellRect {
            start_row,
            end_row,
            start_col,
            end_col,
        }
    }

    fn rows(&self) -> Map<RangeInclusive<usize>, fn(usize) -> VisIdx> {
        VisIdx::range_inc_iter(self.start_row, self.end_row) // Todo work out how to support custom range
    }

    fn cols(&self) -> Map<RangeInclusive<usize>, fn(usize) -> VisIdx> {
        VisIdx::range_inc_iter(self.start_col, self.end_col)
    }
}

impl<RowData, TableData, CellDel, RowMeasure, ColumnMeasure>
    Cells<RowData, TableData, CellDel, RowMeasure, ColumnMeasure>
where
    RowData: Data,
    TableData: IndexedData<Item = RowData, Idx = LogIdx>,
    CellDel: CellsDelegate<TableData>,
    ColumnMeasure: AxisMeasure,
    RowMeasure: AxisMeasure,
{
    pub fn new(
        config: TableConfig,
        column_measure: ColumnMeasure,
        row_measure: RowMeasure,
        cells_delegate: CellDel,
    ) -> Cells<RowData, TableData, CellDel, RowMeasure, ColumnMeasure> {
        Cells {
            config,
            resolved_config: None,
            selection_handlers: Vec::new(),
            selection: TableSelection::NoSelection,
            column_measure,
            row_measure,
            remap_spec_rows: RemapSpec::default(),
            remap_rows: Remap::Pristine,
            cell_delegate: cells_delegate,
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
        log::info!("New selection {:?}", &self.selection);
        ctx.request_paint();
    }

    fn find_cell(&self, pos: &Point) -> Option<SingleCell> {
        let (r, c) = (
            self.row_measure.vis_idx_from_pixel(pos.y),
            self.column_measure.vis_idx_from_pixel(pos.x),
        );
        let log_row = r.and_then(|r| self.remap_rows.get_log_idx(r));
        let log_col = c.and_then(|c| Remap::Pristine.get_log_idx(c)); // TODO! Moving columns
        Some(SingleCell::new(
            AxisPair::new(r?, c?),
            AxisPair::new(log_row?, log_col?),
        ))
    }

    fn measured_size(&mut self) -> Size {
        Size::new(
            self.column_measure.total_pixel_length(),
            self.row_measure.total_pixel_length(),
        )
    }

    fn paint_cells(
        &self,
        ctx: &mut PaintCtx,
        data: &impl IndexedItems<Item = RowData, Idx = LogIdx>,
        env: &Env,
        rtc: &ResolvedTableConfig,
        rect: &CellRect,
    ) {
        for vis_row_idx in rect.rows() {
            let row_top = self.row_measure.first_pixel_from_vis(vis_row_idx);
            if let Some(log_row_idx) = self.remap_rows.get_log_idx(vis_row_idx) {
                data.with(log_row_idx, |row| {
                    self.paint_row(
                        ctx,
                        env,
                        &rtc,
                        &mut rect.cols(),
                        log_row_idx,
                        vis_row_idx,
                        row_top,
                        row,
                    )
                });
            }
        }
    }

    fn paint_row(
        &self,
        ctx: &mut PaintCtx,
        env: &Env,
        rtc: &ResolvedTableConfig,
        cols: &mut impl Iterator<Item = VisIdx>,
        log_row_idx: LogIdx,
        vis_row_idx: VisIdx,
        row_top: Option<f64>,
        row: &RowData,
    ) {
        for vis_col_idx in cols {
            if let Some(log_col_idx) = Remap::Pristine.get_log_idx(vis_col_idx) {
                let cell_left = self.column_measure.first_pixel_from_vis(vis_col_idx);
                let selected =
                    (&self.selection).get_cell_status(&AxisPair::new(vis_row_idx, vis_col_idx));

                let cell_rect = Rect::from_origin_size(
                    Point::new(cell_left.unwrap_or(0.), row_top.unwrap_or(0.)),
                    Size::new(
                        self.column_measure
                            .pixels_length_for_vis(vis_col_idx)
                            .unwrap_or(0.),
                        self.row_measure
                            .pixels_length_for_vis(vis_row_idx)
                            .unwrap_or(0.),
                    ),
                );
                let padded_rect = cell_rect.inset(-rtc.cell_padding);

                ctx.with_save(|ctx| {
                    let layout_origin = padded_rect.origin().to_vec2();
                    ctx.clip(padded_rect);
                    ctx.transform(Affine::translate(layout_origin));
                    ctx.with_child_ctx(padded_rect, |ctxt| {
                        self.cell_delegate
                            .paint(ctxt, log_row_idx, log_col_idx, row, env);
                    });
                });

                // TODO move selection painting out
                match selected {
                    SelectionStatus::Primary => ctx.stroke(
                        cell_rect,
                        &Color::rgb(0, 0, 0xFF),
                        rtc.cell_border_thickness,
                    ),
                    SelectionStatus::AlsoSelected => ctx.stroke(
                        cell_rect,
                        &Color::rgb(0, 0xCC, 0xCC),
                        rtc.cell_border_thickness,
                    ),
                    SelectionStatus::NotSelected => ctx.stroke_bottom_left_border(
                        &cell_rect,
                        &rtc.cells_border,
                        rtc.cell_border_thickness,
                    ),
                }
            } else {
                log::warn!("Could not find logical column for {:?}", vis_col_idx)
            }
        }
    }

    fn remap_for_axis(&self, axis: TableAxis) -> &Remap {
        match axis {
            TableAxis::Rows => &self.remap_rows,
            TableAxis::Columns => &Remap::Pristine,
        }
    }

    fn change_selection(&mut self, ctx: &mut EventCtx, f: &impl TableSelectionMod) {
        if let Some(new_sel) = f.new_selection(&self.selection) {
            self.set_selection(ctx, new_sel);
        }
    }
}

impl<RowData, TableData, ColDel, RowMeasure, ColumnMeasure> Widget<TableData>
    for Cells<RowData, TableData, ColDel, RowMeasure, ColumnMeasure>
where
    RowData: Data,
    TableData: IndexedData<Item = RowData, Idx = LogIdx>,
    ColDel: CellsDelegate<TableData>,
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
                if let Some(AxisMeasureAdjustment::LengthChanged(axis, idx, length)) =
                    cmd.get(ADJUST_AXIS_MEASURE)
                {
                    match axis {
                        TableAxis::Rows => self.row_measure.set_pixel_length_for_vis(*idx, *length),
                        TableAxis::Columns => {
                            self.column_measure.set_pixel_length_for_vis(*idx, *length)
                        }
                    };
                    ctx.request_layout();
                } else if let Some(HeaderClicked(axis, vis, section)) = cmd.get(HEADER_CLICKED) {
                    let vis_addr = AxisPair::new_for_axis(axis, *vis, Default::default());
                    self.get_log_cell(&vis_addr).map(|log_addr| {
                        new_selection = Some(TableSelection::SingleSlice(SingleSlice::new(
                            *axis,
                            SingleCell::new(vis_addr, log_addr),
                        )));
                    });
                }
            }
            Event::KeyDown(ke) => {
                log::info!("Key down {:?}", ke);
                match &ke.key {
                    KbKey::ArrowDown => {
                        new_selection =
                            self.selection
                                .move_focus(&TableAxis::Rows, VisOffset(1), self)
                    }
                    KbKey::ArrowUp => {
                        new_selection =
                            self.selection
                                .move_focus(&TableAxis::Rows, VisOffset(-1), self)
                    }
                    KbKey::ArrowRight => {
                        new_selection =
                            self.selection
                                .move_focus(&TableAxis::Columns, VisOffset(1), self)
                    }
                    KbKey::ArrowLeft => {
                        new_selection =
                            self.selection
                                .move_focus(&TableAxis::Columns, VisOffset(-1), self)
                    }
                    KbKey::Character(s) if s == " " => {
                        // This is to match Excel
                        if ke.mods.ctrl() {
                            new_selection = self.selection.extend_in_axis(TableAxis::Columns, self)
                        } else if ke.mods.shift() {
                            new_selection = self.selection.extend_in_axis(TableAxis::Rows, self)
                        }

                        // TODO - when Ctrl + Shift, select full grid
                    }
                    _ => {}
                }
            }
            _ => (),
        }

        if let Some(sel) = new_selection {
            self.set_selection(ctx, sel);
            ctx.request_focus();
        }
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &TableData,
        _env: &Env,
    ) {
        if let LifeCycle::WidgetAdded = event {
            let rtc = self.config.resolve(_env);
            // Todo: column moves / hiding etc
            self.column_measure.set_axis_properties(
                rtc.cell_border_thickness,
                self.cell_delegate.number_of_columns_in_data(data),
                &Remap::Pristine,
            );

            self.remap_spec_rows = self.cell_delegate.initial_spec();
            self.remap_rows = self.cell_delegate.remap(data, &self.remap_spec_rows);
            self.row_measure.set_axis_properties(
                rtc.cell_border_thickness,
                data.idx_len(),
                &self.remap_rows,
            );
            self.resolved_config = Some(rtc);
        }
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: &TableData, data: &TableData, _env: &Env) {
        if let Some(rtc) = &self.resolved_config {
            if !old_data.same(data) {
                // Reapply sorting / filtering. May need to rate limit
                // and/or have some async way to notify back that sort is complete
                if !self.remap_spec_rows.is_empty() {
                    self.remap_rows = self.cell_delegate.remap(data, &self.remap_spec_rows);
                }

                if old_data.idx_len() != data.idx_len() {
                    // need to deal with reordering and key columns etc

                    self.row_measure.set_axis_properties(
                        rtc.cell_border_thickness,
                        data.idx_len(),
                        &self.remap_rows,
                    );
                    ctx.request_layout(); // TODO: Work out if needed - if we were filling our area before
                }
                // Columns update from data
            }
        }
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        _data: &TableData,
        _env: &Env,
    ) -> Size {
        bc.debug_check("TableCells");
        bc.constrain(self.measured_size())
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &TableData, env: &Env) {
        self.cell_delegate.init(ctx, env); // TODO reduce calls? Invalidate on some changes

        let rtc = self.config.resolve(env);
        let rect = ctx.region().to_rect();

        let draw_rect = rect.intersect(Rect::from_origin_size(Point::ZERO, self.measured_size()));

        ctx.fill(draw_rect, &rtc.cells_background);

        let cell_rect = CellRect::new(
            self.row_measure
                .vis_range_from_pixels(draw_rect.y0, draw_rect.y1),
            self.column_measure
                .vis_range_from_pixels(draw_rect.x0, draw_rect.x1),
        );

        self.paint_cells(ctx, data, env, &rtc, &cell_rect)
    }
}

impl<RowData, TableData, ColDel, RowMeasure, ColumnMeasure> CellDemap
    for Cells<RowData, TableData, ColDel, RowMeasure, ColumnMeasure>
where
    RowData: Data,
    TableData: IndexedData<Item = RowData, Idx = LogIdx>,
    ColDel: CellsDelegate<TableData>,
    ColumnMeasure: AxisMeasure,
    RowMeasure: AxisMeasure,
{
    fn get_log_idx(&self, axis: TableAxis, vis: &VisIdx) -> Option<LogIdx> {
        let remap = self.remap_for_axis(axis);
        return remap.get_log_idx(*vis);
    }
}
