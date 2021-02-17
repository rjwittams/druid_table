use std::marker::PhantomData;
use std::ops::Deref;

use crate::axis_measure::{AxisPair, LogIdx};
use crate::data::{RemapDetails, SortDirection, SortSpec};
use crate::selection::SingleCell;
use crate::{CellsDelegate, IndexedData, Remap, RemapSpec, Remapper, TableAxis};
use druid::text::{EditableText, TextStorage};
use druid::widget::prelude::*;
use druid::widget::{RawLabel, TextBox};
use druid::{Color, Data, KeyOrValue, Lens, WidgetExt};
use std::cmp::Ordering;
use std::fmt;
use std::fmt::{Debug, Formatter};

pub trait DisplayFactory<RowData> {
    fn make_display(&self, cell: &CellCtx) -> Option<Box<dyn Widget<RowData>>>;
    fn make_editor(&self, ctx: &CellCtx) -> Option<Box<dyn Widget<RowData>>>;
}

pub trait DataCompare<Item> {
    fn compare(&self, a: &Item, b: &Item) -> Ordering;
}

pub trait CellDelegate<RowData>: DisplayFactory<RowData> + DataCompare<RowData> {}

impl<RowData> DisplayFactory<RowData> for Box<dyn CellDelegate<RowData>> {
    fn make_display(&self, cell: &CellCtx) -> Option<Box<dyn Widget<RowData>>> {
        self.deref().make_display(cell)
    }

    fn make_editor(&self, ctx: &CellCtx) -> Option<Box<dyn Widget<RowData>>> {
        self.deref().make_editor(ctx)
    }
}

impl<T> DataCompare<T> for Box<dyn CellDelegate<T>> {
    fn compare(&self, a: &T, b: &T) -> Ordering {
        self.deref().compare(a, b)
    }
}

impl<RowData, T> CellDelegate<RowData> for T where T: DisplayFactory<RowData> + DataCompare<RowData> {}

impl<RowData> DisplayFactory<RowData> for Box<dyn DisplayFactory<RowData>> {
    fn make_display(&self, cell: &CellCtx) -> Option<Box<dyn Widget<RowData>>> {
        self.deref().make_display(cell)
    }

    fn make_editor(&self, cell: &CellCtx) -> Option<Box<dyn Widget<RowData>>> {
        self.deref().make_editor(cell)
    }
}

#[derive(Debug)]
pub struct HeaderInfo {
    pub axis: TableAxis,
    pub level: LogIdx,
    pub idx: LogIdx,
}

impl HeaderInfo {
    pub fn new(axis: TableAxis, level: LogIdx, idx: LogIdx) -> Self {
        HeaderInfo { axis, level, idx }
    }
}

#[derive(Debug)]
pub enum CellCtx<'a> {
    Absent,
    Cell(&'a SingleCell),
    Header(HeaderInfo),
}

impl<T, CR: DisplayFactory<T>> DisplayFactory<T> for Vec<CR> {
    fn make_display(&self, cell: &CellCtx) -> Option<Box<dyn Widget<T>>> {
        if let CellCtx::Cell(SingleCell {
            log: AxisPair { col, .. },
            ..
        }) = cell
        {
            if let Some(found) = self.get(col.0) {
                return found.make_display(cell);
            }
        }
        None
    }

    fn make_editor(&self, cell: &CellCtx) -> Option<Box<dyn Widget<T>>> {
        if let CellCtx::Cell(SingleCell {
            log: AxisPair { col, .. },
            ..
        }) = cell
        {
            if let Some(ef) = self.get(col.0) {
                return ef.make_editor(cell);
            }
        }
        None
    }
}

pub struct WidgetCell<MakeLens, Row, Cell> {
    make_lens: MakeLens,
    make_widget: Box<dyn Fn(&CellCtx, &MakeLens) -> Option<Box<dyn Widget<Row>>>>,
    make_editor: Option<Box<dyn Fn(&CellCtx, &MakeLens) -> Option<Box<dyn Widget<Row>>>>>,
    compare: Option<Box<dyn Fn(&Row, &Row) -> Ordering>>,
    phantom_t: PhantomData<Row>,
    phantom_c: PhantomData<Cell>,
}

impl<MakeLens: Fn() -> L, L: Lens<Row, Cell> + 'static, Row: Data, Cell: Data>
    WidgetCell<MakeLens, Row, Cell>
{
    pub fn new_unsorted<CellWidget: Widget<Cell> + 'static>(
        make_widget: impl Fn(&CellCtx) -> CellWidget + 'static,
        make_lens: MakeLens,
    ) -> Self {
        WidgetCell {
            make_widget: Box::new(move |cell: &CellCtx, make_lens: &MakeLens| {
                make_widget(cell).lens(make_lens()).boxed().into()
            }),
            make_lens,
            compare: None,
            make_editor: None,
            phantom_t: Default::default(),
            phantom_c: Default::default(),
        }
    }

    pub fn compare_with<Compare: Fn(&Cell, &Cell) -> Ordering + 'static>(
        mut self,
        cmp: Compare,
    ) -> Self {
        let lens = (self.make_lens)();
        self.compare = Some(Box::new(move |a: &Row, b: &Row| {
            lens.with(a, |a| lens.with(b, |b| cmp(a, b)))
        }));
        self
    }

    pub fn edit_with<EditWidget: Widget<Cell> + 'static>(
        mut self,
        make_editor: impl Fn(&CellCtx) -> EditWidget + 'static,
    ) -> Self {
        self.make_editor = Some(Box::new(move |cell: &CellCtx, make_lens: &MakeLens| {
            make_editor(cell).lens(make_lens()).boxed().into()
        }));
        self
    }
}

impl<MakeLens: Fn() -> L, L: Lens<Row, Cell> + 'static, Row: Data, Cell: Data + Ord>
    WidgetCell<MakeLens, Row, Cell>
{
    // By default if the cell is ord we compare with that natural order
    pub fn new<CellWidget: Widget<Cell> + 'static>(
        make_widget: impl Fn(&CellCtx) -> CellWidget + 'static,
        make_lens: MakeLens,
    ) -> Self {
        let lens = (make_lens)();

        WidgetCell {
            make_lens,
            make_widget: Box::new(move |cell: &CellCtx, make_lens: &MakeLens| {
                make_widget(cell).lens(make_lens()).boxed().into()
            }),
            make_editor: None,
            compare: Some(Box::new(move |a: &Row, b: &Row| {
                lens.with(a, |a| lens.with(b, |b| a.cmp(b)))
            })),
            phantom_t: Default::default(),
            phantom_c: Default::default(),
        }
    }
}

impl<
        MakeLens: Fn() -> L,
        L: Lens<Row, Cell> + 'static,
        Row: Data,
        Cell: Data + Ord + TextStorage + EditableText,
    > WidgetCell<MakeLens, Row, Cell>
{
    pub fn text(make_lens: MakeLens) -> Self {
        Self::new(|_| RawLabel::new().with_text_color(Color::BLACK), make_lens)
            .edit_with(|_| TextBox::new().expand())
    }

    pub fn text_configured(
        cfg: impl Fn(RawLabel<Cell>) -> RawLabel<Cell> + 'static,
        make_lens: MakeLens,
    ) -> Self {
        Self::new(
            move |_| cfg(RawLabel::new().with_text_color(Color::BLACK)),
            make_lens,
        )
        .edit_with(|_| TextBox::new().expand())
    }
}

impl<MakeLens: Fn() -> L, L: Lens<Row, Cell> + 'static, Row: Data, Cell: Data> DisplayFactory<Row>
    for WidgetCell<MakeLens, Row, Cell>
{
    fn make_display(&self, cell: &CellCtx) -> Option<Box<dyn Widget<Row>>> {
        (self.make_widget)(cell, &self.make_lens)
    }

    fn make_editor(&self, cell: &CellCtx) -> Option<Box<dyn Widget<Row>>> {
        self.make_editor
            .as_ref()
            .and_then(|m| m(cell, &self.make_lens))
    }
}

impl<MakeLens: Fn() -> L, L: Lens<Row, Cell>, Row: Data, Cell: Data> DataCompare<Row>
    for WidgetCell<MakeLens, Row, Cell>
{
    fn compare(&self, a: &Row, b: &Row) -> Ordering {
        if let Some(cmp) = &self.compare {
            (cmp)(a, b)
        } else {
            Ordering::Equal
        }
    }
}

pub struct TableColumn<H, T, CD> {
    pub(crate) header: H,
    cell_delegate: CD,
    pub(crate) width: TableColumnWidth,
    pub(crate) sort_order: Option<usize>,
    pub(crate) sort_fixed: bool,
    pub(crate) sort_dir: Option<SortDirection>,
    phantom_: PhantomData<T>,
}

impl<H: Debug, T: Data, CD: CellDelegate<T>> Debug for TableColumn<H, T, CD> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("TableColumn")
            .field("header", &self.header)
            .field("sort_order", &self.sort_order)
            .field("sort_fixed", &self.sort_fixed)
            .field("sort_dir", &self.sort_dir)
            .finish()
    }
}

pub struct TableColumnWidth {
    pub(crate) initial: Option<KeyOrValue<f64>>,
    pub(crate) min: Option<KeyOrValue<f64>>,
    pub(crate) max: Option<KeyOrValue<f64>>,
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
) -> TableColumn<String, T, Box<dyn CellDelegate<T>>> {
    TableColumn::new(header, Box::new(cell_delegate))
}

impl<Header, T: Data, CD: CellDelegate<T>> TableColumn<Header, T, CD> {
    pub fn new(header: impl Into<Header>, cell_delegate: CD) -> Self {
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

impl<Header, RowData: Data, CR: CellDelegate<RowData>> DisplayFactory<RowData>
    for TableColumn<Header, RowData, CR>
{
    fn make_display(&self, cell: &CellCtx) -> Option<Box<dyn Widget<RowData>>> {
        self.cell_delegate.make_display(cell)
    }

    fn make_editor(&self, ctx: &CellCtx) -> Option<Box<dyn Widget<RowData>>> {
        self.cell_delegate.make_editor(ctx)
    }
}

impl<Header, T: Data, CR: CellDelegate<T>> DataCompare<T> for TableColumn<Header, T, CR> {
    fn compare(&self, a: &T, b: &T) -> Ordering {
        self.cell_delegate.compare(a, b)
    }
}

pub struct ProvidedColumns<Header, TableData: IndexedData, ColumnType: CellDelegate<TableData::Item>> {
    cols: Vec<TableColumn<Header, TableData::Item, ColumnType>>,
    phantom_td: PhantomData<TableData>,
}

impl<Header: Debug, TableData: IndexedData, ColumnType: CellDelegate<TableData::Item>> Debug
    for ProvidedColumns<Header, TableData, ColumnType>
{
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("ProvidedColumns")
            .field("cols", &self.cols)
            .finish()
    }
}

impl<Header, TableData: IndexedData, ColumnType: CellDelegate<TableData::Item>>
    ProvidedColumns<Header, TableData, ColumnType>
{
    pub fn new(cols: Vec<TableColumn<Header, TableData::Item, ColumnType>>) -> Self {
        ProvidedColumns {
            cols,
            phantom_td: PhantomData,
        }
    }
}

impl<Header, TableData: IndexedData, ColumnType: CellDelegate<TableData::Item>> Remapper<TableData>
    for ProvidedColumns<Header, TableData, ColumnType>
{
    fn sort_fixed(&self, idx: usize) -> bool {
        self.cols.get(idx).map(|c| c.sort_fixed).unwrap_or(false)
    }

    fn initial_spec(&self) -> RemapSpec {
        let mut spec = RemapSpec::default();

        // Put the columns in sort order
        let mut in_order: Vec<(usize, &TableColumn<Header, TableData::Item, ColumnType>)> =
            self.cols.iter().enumerate().collect();
        in_order.sort_by(|(_, a), (_, b)| match (a.sort_order, b.sort_order) {
            (Some(a), Some(b)) => a.cmp(&b),
            (Some(_), _) => Ordering::Greater,
            (_, Some(_)) => Ordering::Less,
            _ => Ordering::Equal,
        });

        // Then add the ones which have a
        for (idx, dir) in in_order
            .into_iter()
            .map(|(idx, c)| c.sort_dir.as_ref().map(|c| (idx, c)))
            .flatten()
        {
            spec.add_sort(SortSpec::new(idx, dir.clone()))
        }
        spec
    }

    fn remap_from_records(&self, table_data: &TableData, remap_spec: &RemapSpec) -> Remap {
        if remap_spec.is_empty() {
            Remap::Pristine(table_data.data_len()) // Todo: preserve moves
        } else {
            log::info!("Remapping rows");

            //Todo: Filter
            let mut idxs: Vec<LogIdx> = (0usize..table_data.data_len()).map(LogIdx).collect(); //TODO Give up if too big?
            idxs.sort_by(|a_idx, b_idx| {
                table_data
                    .with(*a_idx, |a_row| {
                        table_data
                            .with(*b_idx, |b_row| {
                                for SortSpec { idx, direction } in &remap_spec.sort_by {
                                    if let Some(col) = self.cols.get(*idx) {
                                        let ord = col.compare(a_row, b_row);
                                        if ord != Ordering::Equal {
                                            return direction.apply(ord);
                                        }
                                    } else {
                                        return Ordering::Less;
                                    }
                                }
                                a_idx.0.cmp(&b_idx.0)
                            })
                            .unwrap_or(Ordering::Less)
                    })
                    .unwrap_or(Ordering::Less)
            });
            Remap::Selected(RemapDetails::make_full(idxs))
        }
    }
}

impl<Header, TableData: IndexedData, ColumnType: CellDelegate<TableData::Item>>
    DisplayFactory<TableData::Item> for ProvidedColumns<Header, TableData, ColumnType>
{
    fn make_display(
        &self,
        cell: &CellCtx,
    ) -> Option<Box<dyn Widget<<TableData as IndexedData>::Item>>> {
        self.cols.make_display(cell)
    }

    fn make_editor(&self, ctx: &CellCtx) -> Option<Box<dyn Widget<TableData::Item>>> {
        self.cols.make_editor(ctx)
    }
}

impl<Header, TableData: IndexedData, ColumnType: CellDelegate<TableData::Item>> CellsDelegate<TableData>
    for ProvidedColumns<Header, TableData, ColumnType>
{
    fn data_fields(&self, _data: &TableData) -> usize {
        self.cols.len()
    }
}
