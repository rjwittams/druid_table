use crate::axis_measure::{AxisMeasure, AxisPair, TableAxis, VisOffset};
use crate::cells::CellsDelegate;
use crate::config::ResolvedTableConfig;
use crate::data::{RemapDetails, IndexedDataDiffer, RefreshDiffer, IndexedDataDiff, IndexedDataOp};
use crate::headings::HeadersFromData;
use crate::selection::{CellDemap, SingleCell};
use crate::{
    Cells, DisplayFactory, Headings, IndexedData, LogIdx, Remap, RemapSpec, TableConfig,
    TableSelection, VisIdx,
};
use druid::widget::{Axis, CrossAxisAlignment, Flex, Scope, ScopePolicy, ScopeTransfer, Scroll};
use druid::{
    BoxConstraints, Data, Env, Event, EventCtx, LayoutCtx, Lens, LifeCycle, LifeCycleCtx, PaintCtx,
    Point, Rect, Size, UpdateCtx, Widget, WidgetExt, WidgetPod,
};
use druid_bindings::*;
use std::cmp::Ordering;
use std::fmt::Debug;
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, Instant, Duration};
use druid_widget_nursery::animation::{Animator, AnimationId, SimpleCurve};
use druid::platform_menus::win::file::new;
use std::ops::{DerefMut, Deref};
use druid::im::Vector;

pub struct HeaderBuild<
    HeadersSource: HeadersFromData + 'static,
    HeaderRender: DisplayFactory<HeadersSource::Header> + 'static,
> {
    source: HeadersSource,
    render: HeaderRender,
}

impl<
        HeadersSource: HeadersFromData + 'static,
        HeaderRender: DisplayFactory<HeadersSource::Header> + 'static,
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
    type HeadersSource: HeadersFromData<Headers = Self::Headers, Header = Self::Header, TableData = Self::TableData>
        + 'static;
    type HeaderRender: DisplayFactory<Self::Header> + 'static;

    fn content(self) -> (Self::HeadersSource, Self::HeaderRender);
}

impl<
        HeadersSource: HeadersFromData + 'static,
        HeaderRender: DisplayFactory<HeadersSource::Header> + 'static,
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

#[derive(Copy, Clone, Debug)]
pub(crate) struct RowAnim{
    pub(crate) id: AnimationId,
    pub(crate) op: IndexedDataOp
}

impl RowAnim {
    pub fn new(id: AnimationId, op: IndexedDataOp) -> Self {
        RowAnim { id, op }
    }
}


#[derive(Data, Clone, Lens)]
pub(crate) struct TableState<TableData: Data> {
    pub(crate) table_data: TableData,
    pub(crate) input_instant: Instant,
    pub(crate) scroll_x: f64,
    pub(crate) scroll_y: f64,
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
    pub(crate) row_anims: Vector<RowAnim>
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
            scroll_x: 0.0,
            scroll_y: 0.0,
            scroll_rect: Rect::ZERO,
            config,
            resolved_config,
            table_data: data,
            input_instant: Instant::now(),
            remap_specs: AxisPair::new(RemapSpec::default(), RemapSpec::default()),
            remaps: AxisPair::new(Remap::Pristine, Remap::Pristine),
            selection: TableSelection::default(),
            measures,
            cells_del,
            last_diff: None,
            animator: Arc::new(Mutex::new(Animator::default())),
            row_anims: Vector::new()
        };
        state.remap_rows();
        state.refresh_measure(TableAxis::Rows);
        state.refresh_measure(TableAxis::Columns);
        state
    }

    fn axis_log_len(&self, axis: TableAxis)->usize{
        match axis {
            TableAxis::Rows => self.table_data.data_len(),
            TableAxis::Columns => self.cells_del.data_columns(&self.table_data)
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
            TableAxis::Columns => self.cells_del.data_columns(&self.table_data),
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

                    let mut current: Vec<_> = self.remaps[axis].iter(last_vis).collect();
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
                    self.remaps[axis] =
                        Remap::Selected(RemapDetails::Full(current.into_iter().collect()));
                    self.selection = TableSelection::NoSelection;
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
        differ: Box<dyn IndexedDataDiffer<TableData>>
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
    differ: Box<dyn IndexedDataDiffer<TableData>>
}

impl<TableData: IndexedData> TableScopeTransfer<TableData> {
    pub fn new(differ: Box<dyn IndexedDataDiffer<TableData>>) -> Self {
        TableScopeTransfer {
            phantom_td: Default::default(),
            differ
        }
    }
}

fn same_check<T: Data>(old: &T, new: &T) {
    if !old.same(new) {
        log::warn!("{} not same", std::any::type_name::<T>())
    } else {
        // log::info!("{} same", std::any::type_name::<T>())
    }
}

fn indexed_same<TableData: IndexedData>(old: &TableData, new: &TableData) {
    let (ol, nl) = (old.data_len(), new.data_len());
    if ol != nl {
        log::warn!("Lengths not the same {} {}", ol, nl);
        return;
    }

    for idx in (0..ol).map(LogIdx) {
        old.with(idx, |od| {
            new.with(idx, |nd| {
                if !od.same(nd) {
                    log::info!("Not same at idx {}", idx.0);
                }
            });
        });
    }
}

impl<TableData: IndexedData> ScopeTransfer for TableScopeTransfer<TableData> {
    type In = TableData;
    type State = TableState<TableData>;

    fn read_input(&self, state: &mut Self::State, input: &Self::In, env: &Env) {
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

    fn update_computed(&self, old_state: &Self::State, state: &mut Self::State, env: &Env) -> bool {
        log::info!(
            "Update computed TableScope data changed:{}",
            !old_state.same(state)
        );
        same_check(&old_state.remaps, &state.remaps);
        //same_check(&old_state.measures , &state.measures);
        same_check(&old_state.resolved_config, &state.resolved_config);
        same_check(&old_state.cells_del, &state.cells_del);
        same_check(&old_state.scroll_rect, &state.scroll_rect);
        same_check(&old_state.scroll_x, &state.scroll_x);
        same_check(&old_state.scroll_y, &state.scroll_y);
        same_check(&old_state.selection, &state.selection);
        same_check(&old_state.table_data, &state.table_data);
        indexed_same(&old_state.table_data, &state.table_data);

        let data_changed = !old_state.table_data.same(&state.table_data);

        let new_diff = self.differ.diff(&old_state.table_data, &state.table_data);
        if let (Some(new_diff)) = &new_diff {
            let actually_new = match &state.last_diff{
                None => true,
                Some(last_diff) => !last_diff.same(new_diff)
            };

            if actually_new {
                let anim_id = state.animator.lock().unwrap().new_animation()
                    .curve(SimpleCurve::EaseIn
                    ).duration(Duration::from_millis(500)).id();
                if new_diff.is_refresh() {} else {
                    for op in new_diff.ops() {
                        state.row_anims.push_back(RowAnim::new(anim_id, op));
                    }
                }
            }
        }
        state.last_diff = new_diff.or(state.last_diff.take());

        let remap_specs_same = old_state
            .remap_specs
            .zip_with(&state.remap_specs, |old, new| old.same(new));

        if !remap_specs_same[TableAxis::Rows] || data_changed {
            state.remap_rows();
        }

        let remaps_same =
            old_state.remaps.zip_with(&state.remaps, |old, new| old.same(new));

        remaps_same.for_each(|axis, same|{
            if !same {
                state.refresh_measure(axis);
            }
        });

        true
    }
}

impl<TableData: IndexedData> Table<TableData> {
    pub fn new<Args: TableArgsT<TableData = TableData> + 'static>(
        args: Args,
        measures: AxisPair<AxisMeasure>,
        differ: Box<dyn IndexedDataDiffer<TableData>>
    ) -> Self {
        Table {
            child: Table::build_child(args, measures, differ),
        }
    }

    fn build_child<Args: TableArgsT<TableData = TableData> + 'static>(
        args_t: Args,
        measures: AxisPair<AxisMeasure>,
        differ: Box<dyn IndexedDataDiffer<TableData>>
    ) -> TableChild<TableData> {
        let args = args_t.content();
        let table_config = args.table_config;

        let cells_delegate = args.cells_delegate;
        let cells = Cells::new();

        let cells_scroll = Scroll::new(cells).binding(
            TableState::<TableData>::scroll_x
                .bind(ScrollToProperty::new(Axis::Horizontal))
                .and(TableState::<TableData>::scroll_y.bind(ScrollToProperty::new(Axis::Vertical)))
                .and(TableState::<TableData>::scroll_rect.bind(ScrollRectProperty::default())),
        );

        let policy =
            TableScopePolicy::new(table_config.clone(), measures,
                                  Arc::new(cells_delegate), differ);
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
        if let Some(state) = self.child.widget_mut().state_mut() {
            if !state.row_anims.is_empty(){
                ctx.request_anim_frame()
            }
        }
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
        ctx: &mut UpdateCtx,
        field_val: &Self::Value,
        env: &Env,
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
        env: &Env,
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
        ctx: &EventCtx,
        field: &mut Self::Value,
        change: Self::Change,
        env: &Env,
    ) {
        if let Some(s) = controlled.state() {
            *field = s.selection.clone()
        }
    }
}
