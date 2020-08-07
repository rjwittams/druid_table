#[macro_use]
mod macros;

mod axis_measure;
mod builder;
mod cells;
mod columns;
mod config;
mod data;
mod headings;
pub mod numbers_table;
mod render_ext;
mod selection;
mod table;

pub use axis_measure::{
    AxisMeasure, FixedAxisMeasure, LogIdx, StoredAxisMeasure, TableAxis, VisIdx,
    ADJUST_AXIS_MEASURE,
};
pub use builder::{AxisMeasurementType, DefaultTableArgs, ShowHeadings, TableBuilder};
pub use cells::{Cells, CellsDelegate};
pub use columns::{column, CellRender, CellRenderExt, DataCompare, TextCell, CellCtx, EditorFactory};
pub use config::TableConfig;
pub use data::{IndexedData, IndexedItems, Remap, RemapSpec, Remapper, SortDirection};
pub use headings::{HeadersFromIndices, Headings, SuppliedHeaders, SELECT_INDICES};
pub use selection::{IndicesSelection, TableSelection};
pub use table::{HeaderBuild, Table, TableArgs};
