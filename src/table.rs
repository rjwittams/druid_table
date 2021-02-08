use crate::axis_measure::{AxisMeasure, AxisPair, TableAxis, VisOffset};
use crate::cells::CellsDelegate;
use crate::config::ResolvedTableConfig;
use crate::headings::HeadersFromData;
use crate::selection::CellDemap;
use crate::{
    CellRender, Cells, Headings, IndexedData, LogIdx, Remap, RemapSpec, TableConfig,
    TableSelection, VisIdx,
};
use druid::widget::{
    Axis, CrossAxisAlignment, Flex, Scope, ScopePolicy, ScopeTransfer, Scroll,
};
use druid::{
    BoxConstraints, Data, Env, Event, EventCtx, LayoutCtx, Lens, LifeCycle, LifeCycleCtx, PaintCtx,
    Point, Rect, Size, UpdateCtx, Widget, WidgetExt, WidgetPod,
};
use druid_bindings::*;
use std::marker::PhantomData;
use std::sync::Arc;
use std::fmt::Debug;

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
    type Headers: IndexedData<Item = Self::Header> + 'static;
    type HeadersSource: HeadersFromData<
        Headers = Self::Headers,
        Header = Self::Header,
        TableData = Self::TableData,
    > + 'static;
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
    TableData: IndexedData,
    RowH: HeaderBuildT<TableData = TableData>,
    ColH: HeaderBuildT<TableData = TableData>,
    CellsDel: CellsDelegate<TableData> + 'static,
> where
    TableData::Item: Data,
{
    cells_delegate: CellsDel,
    row_h: Option<RowH>,
    col_h: Option<ColH>,
    table_config: TableConfig,
}

impl<
        TableData: IndexedData,
        RowH: HeaderBuildT<TableData = TableData>,
        ColH: HeaderBuildT<TableData = TableData>,
        CellsDel: CellsDelegate<TableData> + 'static,
    > TableArgs<TableData, RowH, ColH, CellsDel>
{
    pub fn new(
        cells_delegate: CellsDel,
        row_h: Option<RowH>,
        col_h: Option<ColH>,
        table_config: TableConfig,
    ) -> Self {
        TableArgs {
            cells_delegate,
            row_h,
            col_h,
            table_config,
        }
    }
}

// This trait exists to move type parameters to associated types
pub trait TableArgsT {
    type RowData: Data; // Required because associated type bounds are unstable
    type TableData: IndexedData<Item = Self::RowData>;
    type RowH: HeaderBuildT<TableData = Self::TableData>;
    type ColH: HeaderBuildT<TableData = Self::TableData>;

    type CellsDel: CellsDelegate<Self::TableData> + 'static;
    fn content(self) -> TableArgs<Self::TableData, Self::RowH, Self::ColH, Self::CellsDel>;
}

impl<
        TableData: IndexedData,
        RowH: HeaderBuildT<TableData = TableData>,
        ColH: HeaderBuildT<TableData = TableData>,
        CellsDel: CellsDelegate<TableData> + 'static,
    > TableArgsT for TableArgs<TableData, RowH, ColH, CellsDel>
where
    TableData::Item: Data,
{
    type RowData = TableData::Item;
    type TableData = TableData;
    type RowH = RowH;
    type ColH = ColH;
    type CellsDel = CellsDel;

    fn content(self) -> TableArgs<TableData, RowH, ColH, CellsDel> {
        self
    }
}

#[derive(Data, Clone, Debug, Lens)]
pub(crate) struct TableState<TableData: Data> {
    scroll_x: f64,
    scroll_y: f64,
    pub(crate) config: TableConfig,
    pub(crate) resolved_config: ResolvedTableConfig,
    pub(crate) table_data: TableData,
    pub(crate) remap_specs: AxisPair<RemapSpec>,
    pub(crate) remaps: AxisPair<Remap>,
    pub(crate) selection: TableSelection,
    #[data(ignore)]
    pub(crate) measures: AxisPair<AxisMeasure>, // TODO
    pub(crate) cells_del: Arc<dyn CellsDelegate<TableData>>
}

impl<TableData: IndexedData> TableState<TableData> where TableData::Item : Data{
    pub fn new(
        config: TableConfig,
        resolved_config: ResolvedTableConfig,
        data: TableData,
        measures: AxisPair<AxisMeasure>,
        cells_del: Arc<dyn CellsDelegate<TableData>>
    ) -> Self {
            let mut state = TableState {
                scroll_x: 0.0,
                scroll_y: 0.0,
                config,
                resolved_config,
                table_data: data,
                remap_specs: AxisPair::new(RemapSpec::default(), RemapSpec::default()),
                remaps: AxisPair::new(Remap::Pristine, Remap::Pristine),
                selection: TableSelection::default(),
                measures,
                cells_del
            };
            state.remap_rows();
            state.remap_cols();
            state
    }


    fn remap_rows(&mut self) {
        let state = self;
        state.remaps[TableAxis::Rows] = state.cells_del.remap_items(&state.table_data, &state.remap_specs[TableAxis::Rows]);
        state.measures[TableAxis::Rows].set_axis_properties(
            state.resolved_config.cell_border_thickness,
            state.table_data.data_len(),
            &state.remaps[TableAxis::Rows],
        );
    }

    fn remap_cols(&mut self) {
        let state = self;
        state.remaps[TableAxis::Columns] = state.remap_specs[TableAxis::Columns]
            .remap_placements(LogIdx(state.cells_del.number_of_columns_in_data(&state.table_data) - 1));

        state.measures[TableAxis::Columns].set_axis_properties(
            state.resolved_config.cell_border_thickness,
            state.cells_del.number_of_columns_in_data(&state.table_data),
            &state.remaps[TableAxis::Columns],
        );
    }
}

impl<TableData: Data> TableState<TableData>{

    pub fn explicit_header_move(&mut self, axis: TableAxis, moved_to_idx: VisIdx) {
        log::info!(
            "Move selection {:?} on {:?} to {:?}",
            self.selection,
            axis,
            moved_to_idx
        );
        let mut offset = 0;
        if let Some(headers_moved) = self.selection.fully_selected_on_axis(axis) {
            for vis_idx in headers_moved {
                if let Some(log_idx) = self.remaps[axis].get_log_idx(vis_idx) {
                    self.remap_specs[axis].place(log_idx, moved_to_idx + VisOffset(offset));
                    offset += 1;
                }
            }
        }
    }
}

impl CellDemap for AxisPair<Remap> {
    fn get_log_idx(&self, axis: TableAxis, vis: &VisIdx) -> Option<LogIdx> {
        self[axis].get_log_idx(*vis)
    }
}

type TableChild<TableData> = WidgetPod<
    TableData,
    Scope<TableScopePolicy<TableData>, Box<dyn Widget<TableState<TableData>>>>,
>;

pub struct Table<TableData: IndexedData> where TableData::Item : Data{
    child: TableChild<TableData>,
}

struct TableScopePolicy<TableData> {
    config: TableConfig,
    measures: AxisPair<AxisMeasure>,
    cells_delegate: Arc<dyn CellsDelegate<TableData>>,
    phantom_td: PhantomData<TableData>,
}

impl<TableData> TableScopePolicy<TableData> {
    pub fn new(config: TableConfig, measures: AxisPair<AxisMeasure>, cells_delegate: Arc<dyn CellsDelegate<TableData>>) -> Self {
        TableScopePolicy {
            config,
            measures,
            cells_delegate,
            phantom_td: Default::default(),
        }
    }
}

impl<TableData: IndexedData> ScopePolicy for TableScopePolicy<TableData> where TableData::Item : Data {
    type In = TableData;
    type State = TableState<TableData>;
    type Transfer = TableScopeTransfer<TableData>;

    fn create(self, inner: &Self::In, env: &Env) -> (Self::State, Self::Transfer) {
        let rc = self.config.resolve(env);
        (
            TableState::new(self.config, rc, inner.clone(), self.measures, self.cells_delegate),
            TableScopeTransfer::new(),
        )
    }
}

struct TableScopeTransfer<TableData> {
    phantom_td: PhantomData<TableData>,
}

impl<TableData> TableScopeTransfer<TableData> {
    pub fn new() -> Self {
        TableScopeTransfer {
            phantom_td: Default::default(),
        }
    }
}

impl<TableData: IndexedData> ScopeTransfer for TableScopeTransfer<TableData>
where TableData::Item : Data{
    type In = TableData;
    type State = TableState<TableData>;

    fn read_input(&self, state: &mut Self::State, inner: &Self::In, env: &Env) {
        state.table_data = inner.clone();
    }

    fn write_back_input(&self, state: &Self::State, inner: &mut Self::In) {
        if !inner.same(&state.table_data) {
            *inner = state.table_data.clone();
        }
    }

    fn update_computed(&self, old_state: &Self::State, state: &mut Self::State) -> bool {
        let remaps_same = old_state.remap_specs.zip_with(&state.remap_specs, |old, new| old.same(new));

        if !remaps_same[TableAxis::Rows] {
            state.remap_rows();
        }

        if !remaps_same[TableAxis::Columns] {
            state.remap_cols();
        }

        true
    }
}

impl<TableData: IndexedData> Table<TableData> {
    pub fn new<Args: TableArgsT<TableData = TableData> + 'static>(
        args: Args,
        measures: AxisPair<AxisMeasure>,
    ) -> Self {
        Table {
            child: Table::build_child(args, measures),
        }
    }

    fn build_child<Args: TableArgsT<TableData = TableData> + 'static>(
        args_t: Args,
        measures: AxisPair<AxisMeasure>,
    ) -> TableChild<TableData> {
        let args = args_t.content();
        let table_config = args.table_config;

        let cells_delegate = args.cells_delegate;
        let cells = Cells::new();

        let cells_scroll = Scroll::new(cells).binding(
            TableState::<TableData>::scroll_x
                .bind(ScrollToProperty::new(Axis::Horizontal))
                .and(TableState::<TableData>::scroll_y.bind(ScrollToProperty::new(Axis::Vertical))),
        );

        let policy = TableScopePolicy::new(table_config.clone(), measures.clone(), Arc::new(cells_delegate));
        Self::add_headings(args.col_h, args.row_h, policy, table_config, cells_scroll)
    }

    fn add_headings<
        ColH: HeaderBuildT<TableData = TableData>,
        RowH: HeaderBuildT<TableData = TableData>,
    >(
        col_h: Option<ColH>,
        row_h: Option<RowH>,
        policy: TableScopePolicy<TableData>,
        table_config: TableConfig,
        widget: impl Widget<TableState<TableData>> + 'static,
    ) -> TableChild<TableData> {
        if let Some(col_h) = col_h {
            let (source, render) = col_h.content();

            let col_headings = Headings::new(TableAxis::Columns, source, render, true);
            let ch_scroll = Scroll::new(col_headings).disable_scrollbars().binding(
                TableState::<TableData>::scroll_x.bind(ScrollToProperty::new(Axis::Horizontal)),
            );

            let cells_column = Flex::column()
                .cross_axis_alignment(CrossAxisAlignment::Start)
                .with_child(ch_scroll)
                .with_flex_child(widget, 1.);
            Self::add_row_headings(policy, table_config, true, row_h, cells_column)
        } else {
            Self::add_row_headings(policy, table_config, false, row_h, widget)
        }
    }

    fn add_row_headings<RowH: HeaderBuildT<TableData = TableData>>(
        policy: TableScopePolicy<TableData>,
        table_config: TableConfig,
        corner_needed: bool,
        row_h: Option<RowH>,
        widget: impl Widget<TableState<TableData>> + 'static,
    ) -> TableChild<TableData> {
        if let Some(row_h) = row_h {
            let (source, render) = row_h.content();
            let row_headings = Headings::new(TableAxis::Rows, source, render, false);

            let row_scroll = Scroll::new(row_headings).disable_scrollbars().binding(
                TableState::<TableData>::scroll_y.bind(ScrollToProperty::new(Axis::Vertical)),
            );

            let mut rh_col = Flex::column().cross_axis_alignment(CrossAxisAlignment::Start);
            if corner_needed {
                rh_col.add_spacer(table_config.col_header_height.clone())
            }
            rh_col.add_flex_child(row_scroll, 1.);

            let row = Flex::row()
                .cross_axis_alignment(CrossAxisAlignment::Start)
                .with_child(rh_col)
                .with_flex_child(widget, 1.)
                .center();

            Self::wrap_in_scope(policy, row)
        } else {
            Self::wrap_in_scope(policy, widget)
        }
    }

    fn wrap_in_scope<W: Widget<TableState<TableData>> + 'static>(
        policy: TableScopePolicy<TableData>,
        widget: W,
    ) -> TableChild<TableData> {
        WidgetPod::new(Scope::new(
            policy,
            Box::new(widget),
        ))
    }
}

impl<TableData: IndexedData> Widget<TableData> for Table<TableData> where TableData::Item : Data {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut TableData, env: &Env) {
        self.child.event(ctx, event, data, env)
    }

    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, event: &LifeCycle, data: &TableData, env: &Env) {
        self.child.lifecycle(ctx, event, data, env);
    }

    fn update(&mut self, ctx: &mut UpdateCtx, _old_data: &TableData, data: &TableData, env: &Env) {
        if ctx.env_changed(){
            if let Some(state) = self.child.widget_mut().state_mut() {
                state.resolved_config = state.config.resolve(env);
            }
        }
        self.child.update(ctx, data, env);
    }

    fn layout(&mut self, ctx: &mut LayoutCtx, bc: &BoxConstraints, data: &TableData, env: &Env) -> Size {
        let size = self.child.layout(ctx, bc, data, env);
        self.child
            .set_layout_rect(ctx, data, env, Rect::from_origin_size(Point::ORIGIN, size));
        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &TableData, env: &Env) {
        self.child.paint_raw(ctx, data, env);
    }
}
