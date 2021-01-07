use std::marker::PhantomData;

use druid::widget::prelude::*;
use druid::{
    Affine, BoxConstraints, Cursor, Data, Env, Event, EventCtx, LayoutCtx, LifeCycle, LifeCycleCtx,
    PaintCtx, Rect, Size, UpdateCtx, Widget,
};

use crate::axis_measure::{AxisMeasure, LogIdx, TableAxis, VisIdx, VisOffset};
use crate::columns::{CellCtx, CellRender};
use crate::config::{ResolvedTableConfig, TableConfig};
use crate::data::{IndexedItems, SortSpec};
use crate::headings::HeaderMovement::{Disallowed, Permitted};
use crate::numbers_table::LogIdxTable;
use crate::render_ext::RenderContextExt;
use crate::table::TableState;
use crate::IndicesSelection;
use std::collections::HashMap;
use druid_bindings::{BindableAccess, bindable_self_body};

pub trait HeadersFromData {
    type TableData: Data;
    type Header: Data;
    type Headers: IndexedItems<Item = Self::Header, Idx = LogIdx>;
    fn get_headers(&self, table_data: &Self::TableData) -> Self::Headers;
}

pub struct SuppliedHeaders<Headers, TableData> {
    headers: Headers,
    phantom_td: PhantomData<TableData>,
}

impl<Headers, TableData> SuppliedHeaders<Headers, TableData> {
    pub fn new(headers: Headers) -> Self {
        SuppliedHeaders {
            headers,
            phantom_td: Default::default(),
        }
    }
}

impl<Headers: IndexedItems<Idx = LogIdx> + Clone, TableData: Data> HeadersFromData
    for SuppliedHeaders<Headers, TableData>
where
    Headers::Item: Data,
{
    type TableData = TableData;
    type Header = Headers::Item;
    type Headers = Headers;
    fn get_headers(&self, _table_data: &Self::TableData) -> Headers {
        self.headers.clone()
    }
}

#[derive(Clone)]
pub struct HeadersFromIndices<TableData> {
    phantom_td: PhantomData<TableData>,
}

impl<TableData: Data> Data for HeadersFromIndices<TableData> {
    fn same(&self, _other: &Self) -> bool {
        true
    }
}

impl<TableData> Default for HeadersFromIndices<TableData> {
    fn default() -> Self {
        HeadersFromIndices::new()
    }
}

impl<TableData> HeadersFromIndices<TableData> {
    pub fn new() -> Self {
        HeadersFromIndices {
            phantom_td: Default::default(),
        }
    }
}

impl<TableData: IndexedItems + Data> HeadersFromData for HeadersFromIndices<TableData> {
    type TableData = TableData;
    type Header = LogIdx;
    type Headers = LogIdxTable;

    fn get_headers(&self, table_data: &TableData) -> LogIdxTable {
        LogIdxTable::new(table_data.idx_len())
    }
}

enum HeaderMovement {
    Disallowed,
    Permitted,
    Moving(VisIdx),
}

pub struct Headings<HeadersSource, Render>
where
    HeadersSource: HeadersFromData,
    Render: CellRender<HeadersSource::Header>,
{
    axis: TableAxis,
    config: TableConfig,
    resolved_config: Option<ResolvedTableConfig>,
    headers_source: HeadersSource,
    headers: Option<HeadersSource::Headers>,
    header_render: Render,
    header_movement: HeaderMovement,
    resize_dragging: Option<VisIdx>,
    selection_dragging: bool,
}

impl<HeadersSource, Render> Headings<HeadersSource, Render>
where
    HeadersSource: HeadersFromData,
    Render: CellRender<HeadersSource::Header>,
{
    pub fn new(
        axis: TableAxis,
        config: TableConfig,
        headers_source: HeadersSource,
        header_render: Render,
        allow_moves: bool,
    ) -> Headings<HeadersSource, Render> {
        Headings {
            axis,
            config,
            resolved_config: None,
            headers_source,
            headers: None,
            header_render,
            header_movement: if allow_moves { Permitted } else { Disallowed },
            resize_dragging: None,
            selection_dragging: false,
        }
    }

    fn set_pix_length_for_axis(
        &mut self,
        measure: &mut AxisMeasure,
        ctx: &mut EventCtx,
        vis_idx: VisIdx,
        pixel: f64,
    ) {
        measure.set_far_pixel_for_vis(vis_idx, pixel);
        // TODO : this might be overkill if we knew that we are bigger that the viewport - repaint would work
        ctx.request_layout();
    }

    fn paint_header(
        &mut self,
        ctx: &mut PaintCtx,
        data: &TableState<<HeadersSource as HeadersFromData>::TableData>,
        env: &Env,
        measure: &AxisMeasure,
        indices_selection: &IndicesSelection,
        sort_dirs: &HashMap<LogIdx, SortSpec>,
        vis_main_idx: VisIdx,
    ) -> Option<()> {
        let rtc = self.resolved_config.as_ref()?;
        let headers = self.headers.as_ref()?;
        let axis = self.axis;
        let header_render = &mut self.header_render;

        let cell_rect = Rect::from_origin_size(
            axis.cell_origin(measure.first_pixel_from_vis(vis_main_idx)?, 0.),
            axis.size(
                measure.pixels_length_for_vis(vis_main_idx)?,
                rtc.cross_axis_length(&axis),
            ),
        );

        if indices_selection.vis_index_selected(vis_main_idx) {
            ctx.fill(cell_rect, &rtc.header_selected_background);
        }

        let padded_rect = cell_rect.inset(-rtc.cell_padding);
        if let Some(log_main_idx) = data.remaps[self.axis].get_log_idx(vis_main_idx) {
            let cell = CellCtx::Header(&axis, log_main_idx, sort_dirs.get(&log_main_idx));

            headers.with(log_main_idx, |col_name| {
                ctx.with_save(|ctx| {
                    let layout_origin = padded_rect.origin().to_vec2();
                    ctx.clip(padded_rect);
                    ctx.transform(Affine::translate(layout_origin));
                    ctx.with_child_ctx(padded_rect, |ctxt| {
                        header_render.paint(ctxt, &cell, col_name, env);
                    });
                });
            });

            ctx.stroke_bottom_left_border(&cell_rect, &rtc.cells_border, rtc.cell_border_thickness);
        }
        Some(())
    }
}

impl<HeadersSource, Render> Widget<TableState<HeadersSource::TableData>>
    for Headings<HeadersSource, Render>
where
    HeadersSource: HeadersFromData,
    Render: CellRender<HeadersSource::Header>,
{
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut TableState<HeadersSource::TableData>,
        _env: &Env,
    ) {
        let measure = &mut data.measures[self.axis];
        match event {
            Event::MouseDown(me) => {
                let pix_main = self.axis.main_pixel_from_point(&me.pos);
                if me.count == 2 {
                    let extend = me.mods.ctrl() || me.mods.meta();
                    if let Some(vis_idx) = measure.vis_idx_from_pixel(pix_main) {
                        if let Some(log_idx) = data.remaps[self.axis].get_log_idx(vis_idx) {
                            data.remap_specs[self.axis.cross_axis()].toggle_sort(log_idx, extend);
                        }
                        ctx.set_handled()
                    }
                } else if me.count == 1 {
                    //TODO: Combine lookups
                    if let Some(idx) = measure.pixel_near_border(pix_main) {
                        if idx > VisIdx(0) && measure.can_resize(idx - VisOffset(1)) {
                            self.resize_dragging = Some(idx - VisOffset(1));
                            ctx.set_active(true);
                            ctx.set_cursor(self.axis.resize_cursor());
                            ctx.set_handled()
                        }
                    } else if let Some(idx) = measure.vis_idx_from_pixel(pix_main) {
                        let sel = &mut data.selection;
                        // Already selected so move headings:
                        if sel.fully_selects_heading(self.axis, idx) {
                            self.header_movement = HeaderMovement::Moving(idx);
                            ctx.set_active(true);
                        } else {
                            // Change the selection
                            if me.mods.shift() {
                                sel.extend_in_axis(self.axis, idx, &data.remaps);
                            } else {
                                sel.select_in_axis(self.axis, idx, &data.remaps);
                            }
                            self.selection_dragging = true;
                            ctx.set_active(true);
                        }
                        ctx.set_handled()
                    }
                }
            }
            Event::MouseMove(me) => {
                let pix_main = self.axis.main_pixel_from_point(&me.pos);
                if let Some(idx) = self.resize_dragging {
                    self.set_pix_length_for_axis(measure, ctx, idx, pix_main);

                    if me.buttons.is_empty() {
                        self.resize_dragging = None;
                    } else {
                        ctx.set_cursor(self.axis.resize_cursor());
                    }
                    ctx.set_handled()
                } else if let HeaderMovement::Moving(_idx) = self.header_movement {
                    // Show visual indicator
                    if let Some(idx) = measure.vis_idx_from_pixel(pix_main) {}
                    ctx.set_handled()
                } else if self.selection_dragging {
                    if let Some(idx) = measure.vis_idx_from_pixel(pix_main) {
                        data.selection.extend_in_axis(self.axis, idx, &data.remaps);
                    }
                } else if let Some(idx) = measure.pixel_near_border(pix_main) {
                    if idx > VisIdx(0) {
                        let cursor = if measure.can_resize(idx - VisOffset(1)) {
                            self.axis.resize_cursor()
                        } else {
                            &Cursor::NotAllowed
                        };
                        ctx.set_cursor(cursor);
                        ctx.set_handled();
                    }
                } // TODO grabber for when header can move (ie selected)
            }
            Event::MouseUp(me) => {
                let pix_main = self.axis.main_pixel_from_point(&me.pos);
                if let Some(idx) = self.resize_dragging {
                    self.set_pix_length_for_axis(measure, ctx, idx, pix_main);
                    self.resize_dragging = None;
                    ctx.set_active(false);
                    ctx.set_handled();
                } else if let HeaderMovement::Moving(moved_idx) = self.header_movement {
                    if let Some(moved_to_idx) = measure.vis_idx_from_pixel(pix_main) {
                        data.explicit_header_move(self.axis, moved_to_idx)
                    }
                    ctx.set_active(false);
                    ctx.set_handled()
                } else if self.selection_dragging {
                    self.selection_dragging = false;
                    ctx.set_active(false);
                    ctx.set_handled()
                }
            }
            _ => (),
        }
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &TableState<HeadersSource::TableData>,
        env: &Env,
    ) {
        if let LifeCycle::WidgetAdded = event {
            let rtc = self.config.resolve(env);
            self.headers = Some(self.headers_source.get_headers(&data.data)); // TODO Option
            self.resolved_config = Some(rtc);
        }
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &TableState<HeadersSource::TableData>,
        data: &TableState<HeadersSource::TableData>,
        _env: &Env,
    ) {
        if !old_data.same(data) {
            self.headers = Some(self.headers_source.get_headers(&data.data));
            ctx.request_layout(); // TODO Only relayout if actually changed
        }
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &TableState<HeadersSource::TableData>,
        _env: &Env,
    ) -> Size {
        bc.debug_check("ColumnHeadings");
        let cross_axis_length = if let Some(rc) = &self.resolved_config {
            match self.axis {
                TableAxis::Columns => rc.col_header_height,
                TableAxis::Rows => rc.row_header_width,
            }
        } else {
            self.axis.default_header_cross()
        };

        bc.constrain(self.axis.size(
            data.measures[self.axis].total_pixel_length(),
            cross_axis_length,
        ))
    }

    fn paint(
        &mut self,
        ctx: &mut PaintCtx,
        data: &TableState<HeadersSource::TableData>,
        env: &Env,
    ) {
        let measure = &data.measures[self.axis];
        let indices_selection = data.selection.to_axis_selection(self.axis, &data.remaps);

        // TODO build on change of spec
        let cross_rem = &data.remap_specs[self.axis.cross_axis()];
        let sort_dirs: HashMap<LogIdx, SortSpec> = cross_rem
            .sort_by
            .iter()
            .enumerate()
            .map(|(ord, x)| (LogIdx(x.idx), SortSpec::new(ord, x.direction)))
            .collect();

        if let Some(rtc) = &self.resolved_config {
            self.header_render.init(ctx, env);
            let rect = ctx.region().bounding_box();

            ctx.fill(rect, &rtc.header_background);

            let (p0, p1) = self.axis.pixels_from_rect(&rect);
            let (start_main, end_main) = measure.vis_range_from_pixels(p0, p1);

            for vis_main_idx in VisIdx::range_inc_iter(start_main, end_main) {
                // TODO: excessive unwrapping
                self.paint_header(
                    ctx,
                    data,
                    env,
                    measure,
                    &indices_selection,
                    &sort_dirs,
                    vis_main_idx,
                );
            }
        }
    }
}

impl<HeadersSource, Render> BindableAccess for Headings<HeadersSource, Render>
where
    HeadersSource: HeadersFromData,
    Render: CellRender<HeadersSource::Header>,
{
    bindable_self_body!();
}
