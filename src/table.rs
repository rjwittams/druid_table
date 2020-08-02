use crate::axis_measure::TableAxis;
use crate::cells::CellsDelegate;
use crate::headings::HeadersFromData;
use crate::{
    AxisMeasure, CellRender, Cells, Headings, IndexedData, IndexedItems, LogIdx, TableConfig,
    ADJUST_AXIS_MEASURE, SELECT_INDICES,
};
use druid::widget::{CrossAxisAlignment, Flex, Scroll, ScrollTo, SCROLL_TO};
use druid::{BoxConstraints, Data, Env, Event, EventCtx, LayoutCtx, LifeCycle, LifeCycleCtx, PaintCtx, Point, Rect, Size, UpdateCtx, Widget, WidgetExt, WidgetId, WidgetPod, InternalLifeCycle};

pub struct HeaderBuild<
    HeadersSource: HeadersFromData + 'static,
    HeaderRender: CellRender<HeadersSource::Header> + 'static,
> {
    source: HeadersSource,
    render: HeaderRender,
}

impl<
        HeadersSource: HeadersFromData + 'static,
        HeaderRender: CellRender<HeadersSource::Header> + 'static,
    > HeaderBuild<HeadersSource, HeaderRender>
{
    pub fn new(source: HeadersSource, render: HeaderRender) -> Self {
        HeaderBuild { source, render }
    }
}

// This trait exists to move type parameters to associated types
pub trait HeaderBuildT {
    type TableData: Data;
    type Header: Data;
    type Headers: IndexedItems<Item = Self::Header, Idx = LogIdx> + 'static;
    type HeadersSource: HeadersFromData<Headers = Self::Headers, Header = Self::Header, TableData = Self::TableData>
        + 'static;
    type HeaderRender: CellRender<Self::Header> + 'static;

    fn content(self) -> (Self::HeadersSource, Self::HeaderRender);
}

impl<
        HeadersSource: HeadersFromData + 'static,
        HeaderRender: CellRender<HeadersSource::Header> + 'static,
    > HeaderBuildT for HeaderBuild<HeadersSource, HeaderRender>
{
    type TableData = HeadersSource::TableData;
    type Header = HeadersSource::Header;
    type Headers = HeadersSource::Headers;
    type HeadersSource = HeadersSource;
    type HeaderRender = HeaderRender;

    fn content(self) -> (Self::HeadersSource, Self::HeaderRender) {
        (self.source, self.render)
    }
}

pub struct TableArgs<
    TableData: IndexedData<Idx = LogIdx>,
    RowM: AxisMeasure + 'static,
    ColM: AxisMeasure + 'static,
    RowH: HeaderBuildT<TableData = TableData>,
    ColH: HeaderBuildT<TableData = TableData>,
    CellsDel: CellsDelegate<TableData> + 'static,
> where TableData::Item : Data
{
    cells_delegate: CellsDel,
    row_m: RowM,
    col_m: ColM,
    row_h: Option<RowH>,
    col_h: Option<ColH>,
    table_config: TableConfig,
}

impl<
        RowData: Data,
        TableData: IndexedData<Item = RowData, Idx = LogIdx>,
        RowM: AxisMeasure + 'static,
        ColM: AxisMeasure + 'static,
        RowH: HeaderBuildT<TableData = TableData>,
        ColH: HeaderBuildT<TableData = TableData>,
        CellsDel: CellsDelegate<TableData> + 'static,
    > TableArgs<TableData, RowM, ColM, RowH, ColH, CellsDel>
{
    pub fn new(
        cells_delegate: CellsDel,
        row_m: RowM,
        col_m: ColM,
        row_h: Option<RowH>,
        col_h: Option<ColH>,
        table_config: TableConfig,
    ) -> Self {
        TableArgs {
            cells_delegate,
            row_m,
            col_m,
            row_h,
            col_h,
            table_config,
        }
    }
}

// This trait exists to move type parameters to associated types
pub trait TableArgsT {
    type RowData: Data; // Required because associated type bounds are unstable
    type TableData: IndexedData<Item=Self::RowData, Idx = LogIdx>;
    type RowM: AxisMeasure + 'static;
    type ColM: AxisMeasure + 'static;
    type RowH: HeaderBuildT<TableData = Self::TableData>;
    type ColH: HeaderBuildT<TableData = Self::TableData>;

    type CellsDel: CellsDelegate<Self::TableData> + 'static;
    fn content(
        self,
    ) -> TableArgs<
        Self::TableData,
        Self::RowM,
        Self::ColM,
        Self::RowH,
        Self::ColH,
        Self::CellsDel,
    >;
}


impl<
        TableData: IndexedData<Idx = LogIdx>,
        RowM: AxisMeasure + 'static,
        ColM: AxisMeasure + 'static,
        RowH: HeaderBuildT<TableData = TableData>,
        ColH: HeaderBuildT<TableData = TableData>,
        CellsDel: CellsDelegate<TableData> + 'static,
    > TableArgsT for TableArgs<TableData, RowM, ColM, RowH, ColH, CellsDel>
where TableData::Item : Data
{
    type RowData = TableData::Item;
    type TableData = TableData;
    type RowM = RowM;
    type ColM = ColM;
    type RowH = RowH;
    type ColH = ColH;
    type CellsDel = CellsDel;

    fn content(self) -> TableArgs<TableData, RowM, ColM, RowH, ColH, CellsDel> {
        self
    }
}

struct TableChild<TableData> {
    ids: Ids,
    pod: WidgetPod<TableData, Box<dyn Widget<TableData>>>,
}

impl<TableData> TableChild<TableData> {
    pub fn new(ids: Ids, pod: WidgetPod<TableData, Box<dyn Widget<TableData>>>) -> Self {
        TableChild { pod, ids }
    }
}

pub struct Table<Args: TableArgsT> {
    args: Option<Args>,
    child: Option<TableChild<Args::TableData>>,
}

#[derive(Copy, Clone, Debug)]
struct AxisIds {
    headers: WidgetId,
    scroll: WidgetId,
}

impl AxisIds {
    pub fn new() -> Self {
        AxisIds {
            headers: WidgetId::next(),
            scroll: WidgetId::next(),
        }
    }
}

struct Ids {
    cells: WidgetId,
    rows: Option<AxisIds>,
    columns: Option<AxisIds>,
}

impl Ids {
    pub fn new(cells: WidgetId, rows: Option<AxisIds>, columns: Option<AxisIds>) -> Self {
        Ids {
            cells,
            rows,
            columns,
        }
    }
}

impl<Args: TableArgsT> Table<Args> {
    pub fn new(args: Args) -> Self {
        Table {
            args: Some(args),
            child: None,
        }
    }

    fn build_child(&self, args_t: Args) -> TableChild<Args::TableData> {
        let args = args_t.content();
        let table_config = args.table_config;

        let col_headings = true;
        let row_headings = true;

        let ids = Ids::new(
            WidgetId::next(),
            if_opt!(row_headings, AxisIds::new()),
            if_opt!(col_headings, AxisIds::new()),
        );

        let cells_delegate = args.cells_delegate;
        let mut cells = Cells::new(
            table_config.clone(),
            args.col_m.clone(),
            args.row_m.clone(),
            cells_delegate,
        );

        // These have to be added before we move Cells into scroll
        if let Some(AxisIds { headers, .. }) = ids.columns {
            cells.add_selection_handler(move |ctx, table_sel| {
                ctx.submit_command(
                    SELECT_INDICES.with(table_sel.to_axis_selection(TableAxis::Columns)),
                    headers,
                );
            });
        };

        if let Some(AxisIds { headers, .. }) = ids.rows {
            cells.add_selection_handler(move |ctx, table_sel| {
                ctx.submit_command(
                    SELECT_INDICES.with(table_sel.to_axis_selection(TableAxis::Rows)),
                    headers,
                );
            });
        }

        let mut cells_scroll = Scroll::new(cells.with_id(ids.cells));

        if let Some(AxisIds { scroll, .. }) = ids.columns {
            cells_scroll.add_scroll_handler(move |ctx, pos| {
                ctx.submit_command(SCROLL_TO.with(ScrollTo::x(pos.x)), scroll);
            });
        };

        if let Some(AxisIds { scroll, .. }) = ids.rows {
            cells_scroll.add_scroll_handler(move |ctx, pos| {
                ctx.submit_command(SCROLL_TO.with(ScrollTo::y(pos.y)), scroll);
            });
        }

        Self::add_headings(
            args.col_m,
            args.row_m,
            args.col_h,
            args.row_h,
            table_config,
            ids,
            cells_scroll,
        )
    }

    fn add_headings(
        col_m: Args::ColM,
        row_m: Args::RowM,
        col_h: Option<Args::ColH>,
        row_h: Option<Args::RowH>,
        table_config: TableConfig,
        ids: Ids,
        widget: impl Widget<Args::TableData> + 'static,
    ) -> TableChild<Args::TableData> {
        if let (Some(AxisIds { headers, scroll }), Some(col_h)) = (ids.columns, col_h) {
            let (source, render) = col_h.content();
            let mut col_headings = Headings::new(
                TableAxis::Columns,
                table_config.clone(),
                col_m,
                source,
                render,
            );
            let ci = ids.cells;
            col_headings.add_axis_measure_adjustment_handler(move |ctx, adj| {
                ctx.submit_command(ADJUST_AXIS_MEASURE.with(adj.clone()), ci);
            });

            let ch_scroll = Scroll::new(col_headings.with_id(headers))
                .disable_scrollbars()
                .with_id(scroll);

            let cells_column = Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Start)
                .with_child(ch_scroll)
                .with_flex_child(widget, 1.);
            Self::add_row_headings(table_config, true,row_m, row_h, ids, cells_column)
        } else {
            Self::add_row_headings(table_config, false, row_m, row_h, ids, widget)
        }
    }

    fn add_row_headings(
        table_config: TableConfig,
        corner_needed: bool,
        row_m: Args::RowM,
        row_h: Option<Args::RowH>,
        ids: Ids,
        widget: impl Widget<Args::TableData> + 'static,
    ) -> TableChild<Args::TableData> {
        if let (Some(AxisIds { headers, scroll }), Some(row_h)) = (ids.rows, row_h) {
            let (source, render) = row_h.content();
            let mut row_headings =
                Headings::new(TableAxis::Rows, table_config.clone(), row_m, source, render);
            let ci = ids.cells;
            row_headings.add_axis_measure_adjustment_handler(move |ctx, adj| {
                ctx.submit_command(ADJUST_AXIS_MEASURE.with(adj.clone()), ci);
            });

            let row_scroll = Scroll::new(row_headings.with_id(headers))
                .disable_scrollbars()
                .with_id(scroll);

            let mut rh_col = Flex::column().cross_axis_alignment(CrossAxisAlignment::Start);
            if corner_needed{
                rh_col.add_spacer(table_config.col_header_height)
            }
            rh_col.add_flex_child(row_scroll, 1.);

            TableChild::new(
                ids,
                WidgetPod::new(Box::new(
                    Flex::row()
                        .cross_axis_alignment(CrossAxisAlignment::Start)
                        .with_child(rh_col)
                        .with_flex_child(widget, 1.)
                        .center(),
                )),
            )
        } else {
            TableChild::new(ids, WidgetPod::new(Box::new(widget.center())))
        }
    }
}

impl<Args: TableArgsT> Widget<Args::TableData> for Table<Args> {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut Args::TableData, env: &Env) {
       // log::info!("Table event {:?}", event);
        if let Some(child) = self.child.as_mut() {
            child.pod.event(ctx, event, data, env);
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &Args::TableData,
        env: &Env,
    ) {
        match event {
            LifeCycle::WidgetAdded | LifeCycle::Internal(InternalLifeCycle::RouteWidgetAdded) => {
                if self.args.is_some() {
                    let mut args = None;
                    std::mem::swap(&mut self.args, &mut args);
                    self.child = args.map(|args| self.build_child(args));
                    log::info!("Made child table")
                } else{
                    log::warn!("Tried to create child but args consumed!")
                }
            }
            _ => {}
        }
        if let Some(child) = self.child.as_mut() {
            child.pod.lifecycle(ctx, event, data, env);
        }
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &Args::TableData,
        data: &Args::TableData,
        env: &Env,
    ) {
        if let Some(child) = self.child.as_mut() {
            child.pod.update(ctx, data, env);
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &Args::TableData,
        env: &Env,
    ) -> Size {
        if let Some(child) = self.child.as_mut() {
            let size = child.pod.layout(ctx, bc, data, env);
            child
                .pod
                .set_layout_rect(ctx, data, env, Rect::from_origin_size(Point::ORIGIN, size));
            size
        } else {
            bc.max()
        }
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &Args::TableData, env: &Env) {
        if let Some(child) = self.child.as_mut() {
            child.pod.paint_raw(ctx, data, env);
        }
    }
}
