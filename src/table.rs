use crate::axis_measure::{AxisMeasure, AxisPair, TableAxis, VisOffset};
use crate::cells::CellsDelegate;
use crate::config::ResolvedTableConfig;
use crate::data::{IndexedDataDiff, IndexedDataDiffer, IndexedDataOp, RemapDetails};
use crate::headings::HeadersFromData;
use crate::interp::{
    EnterExit, HasInterp, Interp, InterpCoverage, InterpNode, InterpResult
};
use crate::selection::{CellDemap, SingleCell};
use crate::{
    Cells, DisplayFactory, Headings, IndexedData, LogIdx, Remap, RemapSpec, Remapper, TableConfig,
    TableSelection, VisIdx,
};
use druid::widget::{Axis, CrossAxisAlignment, Flex, Scope, ScopePolicy, ScopeTransfer, Scroll};
use druid::{
    BoxConstraints, Data, Env, Event, EventCtx, LayoutCtx, Lens, LensExt, LifeCycle, LifeCycleCtx,
    PaintCtx, Point, Rect, Size, UpdateCtx, Widget, WidgetExt, WidgetPod,
};
use druid_bindings::*;
use druid_widget_nursery::animation::{AnimationCtx, AnimationId, Animator, SimpleCurve};
use std::collections::HashMap;
use std::fmt::Debug;
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};


pub struct HeaderBuild<
    HeadersSource: HeadersFromData + 'static
> {
    source: HeadersSource,
    render: Box<dyn DisplayFactory<HeadersSource::Header>>,
}

impl<
        HeadersSource: HeadersFromData + 'static
    > HeaderBuild<HeadersSource>
{
    pub fn new(source: HeadersSource, render: Box<dyn DisplayFactory<HeadersSource::Header>>) -> Self {
        HeaderBuild { source, render }
    }
}

// This trait exists to move type parameters to associated types
pub trait HeaderBuildT {
    type TableData: Data;
    type Header: Data;
    type Headers: IndexedData<Item = Self::Header> + 'static;
    type HeadersSource: HeadersFromData<Headers = Self::Headers, Header = Self::Header, TableData = Self::TableData>
        + 'static;

    fn content(self) -> (Self::HeadersSource, Box<dyn DisplayFactory<Self::Header>>);
}

impl<
        HeadersSource: HeadersFromData + 'static
    > HeaderBuildT for HeaderBuild<HeadersSource>
{
    type TableData = HeadersSource::TableData;
    type Header = HeadersSource::Header;
    type Headers = HeadersSource::Headers;
    type HeadersSource = HeadersSource;

    fn content(self) -> (Self::HeadersSource, Box<dyn DisplayFactory<HeadersSource::Header>>) {
        (self.source, self.render)
    }
}

pub struct TableArgs<
    TableData: IndexedData,
    RowH: HeaderBuildT<TableData = TableData>,
    ColH: HeaderBuildT<TableData = TableData>,
    CellsDel: CellsDelegate<TableData> + 'static,
> {
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
    type TableData: IndexedData;
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
{
    type TableData = TableData;
    type RowH = RowH;
    type ColH = ColH;
    type CellsDel = CellsDel;

    fn content(self) -> TableArgs<TableData, RowH, ColH, CellsDel> {
        self
    }
}

#[derive(Clone, Debug, Default)]
pub struct PixelRange {
    pub(crate) p_0: f64,
    pub(crate) p_1: f64,
}

impl PixelRange {
    pub fn new(p_0: f64, p_1: f64) -> Self {
        PixelRange { p_0: p_0.min(p_1), p_1: p_0.max(p_1) }
    }

    pub fn move_to(&mut self, p_0: f64) {
        let diff = self.p_1 - self.p_0;
        self.p_0 = p_0;
        self.p_1 = p_0 + diff;
        log::info!("Move px range {:?}", (diff, self.p_0, self.p_1))
    }

    pub fn extent(&self)->f64{
        self.p_1 - self.p_0
    }
}

impl EnterExit for PixelRange {
    fn enter(&self) -> Self {
        Self {
            p_0: self.p_0,
            p_1: self.p_0,
        }
    }

    fn exit(&self) -> Self {
        self.enter()
    }
}

impl HasInterp for PixelRange {
    type Interp = PixelRangeInterp;
}

#[derive(Debug, Default)]
pub struct PixelRangeInterp {
    pub p_0: InterpNode<f64>,
    pub p_1: InterpNode<f64>,
}

impl Interp for PixelRangeInterp {
    type Value = PixelRange;

    fn interp(&mut self, ctx: &AnimationCtx, val: &mut Self::Value) -> InterpResult {
        self.p_0.interp(ctx, &mut val.p_0)?;
        self.p_1.interp(ctx, &mut val.p_1)
    }

    fn coverage(&self) -> InterpCoverage {
        self.p_0.coverage() + self.p_1.coverage()
    }

    fn select_animation_segment(self, idx: AnimationId) -> Result<Self, Self> {
        Ok(Self {
            p_0: self.p_0.select_anim(idx),
            p_1: self.p_1.select_anim(idx),
        })
    }

    fn merge(self, other: Self) -> Self {
        Self {
            p_0: self.p_0.merge(other.p_0),
            p_1: self.p_1.merge(other.p_1),
        }
    }

    fn build(start: Self::Value, end: Self::Value) -> Self {
        Self {
            p_0: start.p_0.tween(end.p_0),
            p_1: start.p_1.tween(end.p_1),
        }
    }
}

struct ExitingRow {
    pub(crate) y_0: f64,
    pub(crate) y_1: f64,
    //pub(crate) row: ImageBuffer
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TableOverrides {
    // rows that will be visible at the end of animation
    pub(crate) measure: AxisPair<HashMap<LogIdx, PixelRange>>,
    // exiting rows
    //pub(crate) exiting_rows: Vec<ExitingRow>
}

impl HasInterp for TableOverrides {
    type Interp = TableOverridesInterp;
}

#[derive(Debug, Default)]
pub(crate) struct TableOverridesInterp {
    pub(crate) measure: InterpNode<AxisPair<HashMap<LogIdx, PixelRange>>>,
}

impl Interp for TableOverridesInterp {
    type Value = TableOverrides;

    fn interp(&mut self, ctx: &AnimationCtx, val: &mut Self::Value) -> InterpResult {
        self.measure.interp(ctx, &mut val.measure)
    }

    fn coverage(&self) -> InterpCoverage {
        self.measure.coverage()
    }

    fn select_animation_segment(self, idx: AnimationId) -> Result<Self, Self> {
        Ok(Self {
            measure: self.measure.select_anim(idx),
        })
    }

    fn merge(self, other: Self) -> Self {
        Self {
            measure: self.measure.merge(other.measure),
        }
    }

    fn build(start: Self::Value, end: Self::Value) -> Self {
        Self {
            measure: start.measure.tween(end.measure),
        }
    }
}

#[derive(Data, Clone, Lens)]
pub(crate) struct TableState<TableData> {
    pub(crate) table_data: TableData,
    pub(crate) input_instant: Instant,
    pub(crate) scroll_rect: Rect,
    pub(crate) config: TableConfig,
    pub(crate) resolved_config: ResolvedTableConfig,
    pub(crate) remap_specs: AxisPair<RemapSpec>,
    pub(crate) remaps: AxisPair<Remap>,
    pub(crate) selection: TableSelection,
    #[data(ignore)]
    pub(crate) measures: AxisPair<AxisMeasure>, // TODO
    pub(crate) cells_del: Arc<dyn CellsDelegate<TableData>>,
    pub(crate) last_diff: Option<IndexedDataDiff>,
    #[data(ignore)]
    pub(crate) animator: Arc<Mutex<Animator>>,
    #[data(ignore)]
    pub(crate) overrides: TableOverrides,
    pub(crate) interp: Arc<Mutex<InterpNode<TableOverrides>>>,
}

impl<TableData: IndexedData> TableState<TableData> {
    pub fn new(
        config: TableConfig,
        resolved_config: ResolvedTableConfig,
        data: TableData,
        measures: AxisPair<AxisMeasure>,
        cells_del: Arc<dyn CellsDelegate<TableData>>,
    ) -> Self {
        let mut state = TableState {
            scroll_rect: Rect::ZERO,
            config,
            resolved_config,
            table_data: data,
            input_instant: Instant::now(),
            remap_specs: AxisPair::new(cells_del.initial_spec(), RemapSpec::default()),
            remaps: AxisPair::new(Remap::default(), Remap::default()),
            selection: TableSelection::default(),
            measures,
            cells_del,
            last_diff: None,
            animator: Arc::new(Mutex::new(Animator::default())),
            overrides: TableOverrides::default(),
            interp: Arc::new(Mutex::new(Default::default())),
        };
        state.remap_rows();
        state.refresh_measure(TableAxis::Rows);
        state.refresh_measure(TableAxis::Columns);
        state
    }

    fn axis_log_len(&self, axis: TableAxis) -> usize {
        match axis {
            TableAxis::Rows => self.table_data.data_len(),
            TableAxis::Columns => self.cells_del.data_fields(&self.table_data),
        }
    }

    fn refresh_measure(&mut self, axis: TableAxis) {
        let log_len = self.axis_log_len(axis);
        self.measures[axis].set_axis_properties(
            self.resolved_config.cell_border_thickness,
            log_len,
            &self.remaps[axis],
        );
        // TODO: Maintain logical selection
        self.selection = TableSelection::NoSelection;
    }

    fn remap_rows(&mut self) {
        self.remaps[TableAxis::Rows] = self
            .cells_del
            .remap_from_records(&self.table_data, &self.remap_specs[TableAxis::Rows]);
    }

    pub(crate) fn visible_rect(&self) -> Rect {
        self.scroll_rect.intersect(Rect::from_origin_size(
            Point::ZERO,
            self.measures.measured_size(),
        ))
    }

    pub(crate) fn find_cell(&self, pos: Point) -> Option<SingleCell> {
        let vis = self
            .measures
            .zip_with(&AxisPair::new(pos.y, pos.x), |m, p| {
                m.vis_idx_from_pixel(*p)
            })
            .opt()?;
        let log = self.remaps.get_log_cell(&vis)?;
        Some(SingleCell::new(vis, log))
    }

    pub(crate) fn vis_idx_visible_for_axis(&self, axis: TableAxis) -> impl Iterator<Item = VisIdx> {
        let vis_rect = self.visible_rect();
        let cells = self.measures.cell_rect_from_pixels(vis_rect);
        let (from, to) = cells.range(axis);
        VisIdx::range_inc_iter(from, to)
    }

    pub(crate) fn log_idx_visible_for_axis(
        &self,
        axis: TableAxis,
    ) -> impl Iterator<Item = LogIdx> + '_ {
        let remap = &self.remaps[axis];
        self.vis_idx_visible_for_axis(axis)
            .flat_map(move |vis| remap.get_log_idx(vis))
    }

    pub fn explicit_header_move(
        &mut self,
        axis: TableAxis,
        moved_from_idx: VisIdx,
        moved_to_idx: VisIdx,
    ) {
        log::info!(
            "Move selection {:?}\n\t on {:?} from {:?} to {:?}",
            self.selection,
            axis,
            moved_from_idx,
            moved_to_idx
        );

        let size = match axis {
            TableAxis::Columns => self.cells_del.data_fields(&self.table_data),
            TableAxis::Rows => self.table_data.data_len(),
        };

        if size > 0 {
            let last_vis = VisIdx(size - 1);

            let move_by = moved_to_idx - moved_from_idx;

            if move_by != VisOffset(0) {
                if let Some(mut headers_moved) = self.selection.fully_selected_on_axis(axis) {
                    let mut past_end: Vec<LogIdx> = Default::default();

                    if move_by.0 > 0 {
                        headers_moved.reverse()
                    }

                    let mut current: Vec<_> =
                        self.remaps[axis].log_idx_in_vis_order(last_vis).collect();
                    for vis_idx in headers_moved {
                        let new_vis = vis_idx + move_by;
                        if vis_idx.0 >= current.len() {
                            log::warn!(
                                "Trying to move {:?}->{:?} to {:?} but len is {}",
                                vis_idx,
                                current.get(vis_idx.0),
                                new_vis,
                                current.len()
                            )
                        } else {
                            let log_idx = current.remove(vis_idx.0);

                            if new_vis.0 >= current.len() {
                                past_end.push(log_idx)
                            } else {
                                current.insert(new_vis.0, log_idx)
                            }
                        }
                    }

                    if move_by.0 > 0 {
                        past_end.reverse()
                    }
                    current.append(&mut past_end);

                    //self.selection.move_by(move_by, axis);
                    self.remaps[axis] = Remap::Selected(RemapDetails::make_full(current));
                    self.selection = TableSelection::NoSelection;
                }
            }
        }
    }
}

impl CellDemap for AxisPair<Remap> {
    fn get_log_idx(&self, axis: TableAxis, vis: VisIdx) -> Option<LogIdx> {
        self[axis].get_log_idx(vis)
    }

    fn get_vis_idx(&self, axis: TableAxis, log: LogIdx) -> Option<VisIdx> {
        self[axis].get_vis_idx(log)
    }
}

type TableChild<TableData> = WidgetPod<
    TableData,
    Scope<TableScopePolicy<TableData>, Box<dyn Widget<TableState<TableData>>>>,
>;

pub struct Table<TableData: IndexedData> {
    child: TableChild<TableData>,
}

struct TableScopePolicy<TableData> {
    config: TableConfig,
    measures: AxisPair<AxisMeasure>,
    cells_delegate: Arc<dyn CellsDelegate<TableData>>,
    differ: Box<dyn IndexedDataDiffer<TableData>>,
    phantom_td: PhantomData<TableData>,
}

impl<TableData> TableScopePolicy<TableData> {
    pub fn new(
        config: TableConfig,
        measures: AxisPair<AxisMeasure>,
        cells_delegate: Arc<dyn CellsDelegate<TableData>>,
        differ: Box<dyn IndexedDataDiffer<TableData>>,
    ) -> Self {
        TableScopePolicy {
            config,
            measures,
            cells_delegate,
            differ,
            phantom_td: Default::default(),
        }
    }
}

impl<TableData: IndexedData> ScopePolicy for TableScopePolicy<TableData> {
    type In = TableData;
    type State = TableState<TableData>;
    type Transfer = TableScopeTransfer<TableData>;

    fn create(self, inner: &Self::In, env: &Env) -> (Self::State, Self::Transfer) {
        let rc = self.config.resolve(env);
        (
            TableState::new(
                self.config,
                rc,
                inner.clone(),
                self.measures,
                self.cells_delegate,
            ),
            TableScopeTransfer::new(self.differ),
        )
    }
}

struct TableScopeTransfer<TableData> {
    phantom_td: PhantomData<TableData>,
    differ: Box<dyn IndexedDataDiffer<TableData>>,
}

impl<TableData: IndexedData> TableScopeTransfer<TableData> {
    pub fn new(differ: Box<dyn IndexedDataDiffer<TableData>>) -> Self {
        TableScopeTransfer {
            phantom_td: Default::default(),
            differ,
        }
    }
}

impl<TableData: IndexedData> ScopeTransfer for TableScopeTransfer<TableData> {
    type In = TableData;
    type State = TableState<TableData>;

    fn read_input(&self, state: &mut Self::State, input: &Self::In, _env: &Env) {
        log::info!("Read input table data to TableState");
        if !input.same(&state.table_data) {
            log::info!("Actually wrote table data to TableState");
            state.table_data = input.clone();
            state.input_instant = Instant::now();
        }
    }

    fn write_back_input(&self, state: &Self::State, input: &mut Self::In) {
        if !input.same(&state.table_data) {
            *input = state.table_data.clone();
        }
    }

    fn update_computed(&self, old_state: &Self::State, state: &mut Self::State, _env: &Env) -> bool {
        log::info!(
            "Update computed TableScope data changed:{}",
            !old_state.same(state)
        );

        let new_diff = self.differ.diff(&old_state.table_data, &state.table_data);

        let remap_specs_same = old_state
            .remap_specs
            .zip_with(&state.remap_specs, |old, new| old.same(new));

        if (!remap_specs_same[TableAxis::Rows]) || new_diff.is_some() {
            state.remap_rows();
        }

        let remaps_same = old_state
            .remaps
            .zip_with(&state.remaps, |old, new| old.same(new));

        // Record old positions needed for animations like move
        let mut start_table_overrides = TableOverrides::default();

        if let Some(new_diff) = &new_diff {
            for op in new_diff.ops() {
                match op {
                    IndexedDataOp::Move(old_log_idx, log_idx) => {
                        if let Some(old_vis_idx) =
                            state.remaps.get_vis_idx(TableAxis::Rows, old_log_idx)
                        {
                            if let Some(px) =
                                state.measures[TableAxis::Rows].pix_range_from_vis(old_vis_idx)
                            {
                                start_table_overrides.measure[TableAxis::Rows]
                                    .insert(log_idx, px);
                            }
                        }
                    }
                    IndexedDataOp::Delete(_old_log_idx) => {

                        // if let Some(old_vis_idx) = state.remaps.get_vis_idx(TableAxis::Rows, old_log_idx) {
                        //     if let Some((y_0, y_1)) = state.measures[TableAxis::Rows].pix_range_from_vis(old_vis_idx) {
                        //         if let Ok(mut device) = Device::new(){
                        //             if let Ok(mut target) = device.bitmap_target(1000, (y_1 - y_0) as usize, 1.0){
                        //                 let ctx = target.render_context();
                        //
                        //             }
                        //         }
                        //         //start_table_overrides.current_rows.insert(log_idx, RowOverrides { y_0, y_1 });
                        //     }
                        // }
                    }
                    _ => (),
                }
            }
        }

        remaps_same.for_each(|axis, same| {
            if !same {
                state.refresh_measure(axis);
            }
        });

        let mut end_table_overrides = TableOverrides::default();
        if let Some(new_diff) = &new_diff {
            let actually_new = match &state.last_diff {
                None => true,
                Some(last_diff) => !last_diff.same(new_diff),
            };

            if actually_new {
                if new_diff.is_refresh() {
                } else {
                    for op in new_diff.ops() {
                        match op {
                            IndexedDataOp::Move(_old_log_idx, log_idx) => {
                                if let Some(vis_idx) =
                                    state.remaps.get_vis_idx(TableAxis::Rows, log_idx)
                                {
                                    if let Some(px) =
                                        state.measures[TableAxis::Rows].pix_range_from_vis(vis_idx)
                                    {
                                        end_table_overrides.measure[TableAxis::Rows]
                                            .insert(log_idx, px);
                                    }
                                }
                            }
                            IndexedDataOp::Insert(log_idx) => {
                                if let Some(vis_idx) =
                                    state.remaps.get_vis_idx(TableAxis::Rows, log_idx)
                                {
                                    if let Some(px) =
                                        state.measures[TableAxis::Rows].pix_range_from_vis(vis_idx)
                                    {
                                        end_table_overrides.measure[TableAxis::Rows]
                                            .insert(log_idx, px);
                                    }
                                }
                            }
                            _ => (),
                        }
                    }
                }
                let new_interp = if let Ok(mut animator) = state.animator.lock() {
                    let anim_id = animator
                        .new_animation()
                        .curve(SimpleCurve::EaseIn)
                        .duration(Duration::from_millis(500))
                        .id();

                    //let remove_ov_ai = animator.new_animation().after(anim_id).id();

                    state.overrides.measure[TableAxis::Rows]
                        .extend(start_table_overrides.measure[TableAxis::Rows].clone());
                    start_table_overrides
                        .tween(end_table_overrides.clone())
                        .select_anim(anim_id)
                    //  .merge(end_table_overrides.tween(Default::default()).select_anim(remove_ov_ai))
                } else {
                    Default::default()
                };
                if !new_interp.is_noop() {
                    if let Ok(mut interp) = state.interp.lock() {
                        interp.merge_ref(new_interp);
                    }
                }
            }
        }

        state.last_diff = new_diff.or(state.last_diff.take());

        true
    }
}

impl<TableData: IndexedData> Table<TableData> {
    pub fn new<Args: TableArgsT<TableData = TableData> + 'static>(
        args: Args,
        measures: AxisPair<AxisMeasure>,
        differ: Box<dyn IndexedDataDiffer<TableData>>,
    ) -> Self {
        Table {
            child: Table::build_child(args, measures, differ),
        }
    }

    fn build_child<Args: TableArgsT<TableData = TableData> + 'static>(
        args_t: Args,
        measures: AxisPair<AxisMeasure>,
        differ: Box<dyn IndexedDataDiffer<TableData>>,
    ) -> TableChild<TableData> {
        let args = args_t.content();
        let table_config = args.table_config;

        let cells_delegate = args.cells_delegate;
        let cells = Cells::new();

        let cells_scroll =
            Scroll::new(cells).binding(TableState::scroll_rect.bind(ScrollRectProperty::default()));

        let policy = TableScopePolicy::new(
            table_config.clone(),
            measures,
            Arc::new(cells_delegate),
            differ,
        );
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

            let col_headings = Headings::new(TableAxis::Columns, source, Box::new(render), true);
            let ch_scroll = Scroll::new(col_headings).disable_scrollbars().binding(
                TableState::<TableData>::scroll_rect
                    .then(lens!(Rect, x0))
                    .bind(ScrollToProperty::new(Axis::Horizontal)),
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
            let row_headings = Headings::new(TableAxis::Rows, source, Box::new(render), false);

            let row_scroll = Scroll::new(row_headings).disable_scrollbars().binding(
                TableState::<TableData>::scroll_rect
                    .then(lens!(Rect, y0))
                    .bind(ScrollToProperty::new(Axis::Vertical)),
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
        WidgetPod::new(Scope::new(policy, Box::new(widget)))
    }

    fn state(&self) -> Option<&TableState<TableData>> {
        self.child.widget().state()
    }

    fn state_mut(&mut self) -> Option<&mut TableState<TableData>> {
        self.child.widget_mut().state_mut()
    }
}

impl<TableData: IndexedData> Widget<TableData> for Table<TableData> {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut TableData, env: &Env) {
        self.child.event(ctx, event, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &TableData,
        env: &Env,
    ) {
        self.child.lifecycle(ctx, event, data, env);
    }

    fn update(&mut self, ctx: &mut UpdateCtx, _old_data: &TableData, data: &TableData, env: &Env) {
        log::info!(
            "Table update {:?} data:{}, env:{}, req_up:{}",
            SystemTime::now(),
            !_old_data.same(data),
            ctx.env_changed(),
            ctx.has_requested_update()
        );
        if ctx.env_changed() {
            if let Some(state) = self.child.widget_mut().state_mut() {
                state.resolved_config = state.config.resolve(env);
            }
        }
        self.child.update(ctx, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &TableData,
        env: &Env,
    ) -> Size {
        let size = self.child.layout(ctx, bc, data, env);
        self.child
            .set_layout_rect(ctx, data, env, Rect::from_origin_size(Point::ORIGIN, size));
        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &TableData, env: &Env) {
        self.child.paint_raw(ctx, data, env);
    }
}

impl<TableData: IndexedData> BindableAccess for Table<TableData> {
    bindable_self_body!();
}

pub struct TableSelectionProp<TableData> {
    phantom_td: PhantomData<TableData>,
}

impl<TableData> Default for TableSelectionProp<TableData> {
    fn default() -> Self {
        Self {
            phantom_td: Default::default(),
        }
    }
}

impl<TableData: IndexedData> BindableProperty for TableSelectionProp<TableData> {
    type Controlled = Table<TableData>;
    type Value = TableSelection;
    type Change = ();

    fn write_prop(
        &self,
        controlled: &mut Self::Controlled,
        _ctx: &mut UpdateCtx,
        field_val: &Self::Value,
        _env: &Env,
    ) {
        if let Some(s) = controlled.state_mut() {
            s.selection = field_val.clone()
        }
    }

    fn append_changes(
        &self,
        controlled: &Self::Controlled,
        field_val: &Self::Value,
        change: &mut Option<Self::Change>,
        _env: &Env,
    ) {
        if let Some(s) = controlled.state() {
            if !s.selection.same(field_val) {
                *change = Some(())
            }
        }
    }

    fn update_data_from_change(
        &self,
        controlled: &Self::Controlled,
        _ctx: &EventCtx,
        field: &mut Self::Value,
        _change: Self::Change,
        _env: &Env,
    ) {
        if let Some(s) = controlled.state() {
            *field = s.selection.clone()
        }
    }
}

#[cfg(test)]
mod test {
    use crate::VisIdx;

    #[test]
    fn test_range() {
        let v: Vec<_> = VisIdx::range_inc_iter(VisIdx(1), VisIdx(0)).collect();
        assert!(v.is_empty())
    }
}
