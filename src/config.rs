use crate::axis_measure::TableAxis;
use druid::{theme, Color, Env, KeyOrValue};

pub(crate) const DEFAULT_COL_HEADER_HEIGHT: f64 = 25.0;
pub(crate) const DEFAULT_ROW_HEADER_WIDTH: f64 = 100.0;

#[derive(Clone, Data, Debug)]
pub struct TableConfig {
    pub col_header_height: KeyOrValue<f64>,
    pub row_header_width: KeyOrValue<f64>,
    pub header_background: KeyOrValue<Color>,
    pub header_selected_background: KeyOrValue<Color>,
    pub cells_background: KeyOrValue<Color>,
    pub cells_border: KeyOrValue<Color>,
    pub cell_border_thickness: KeyOrValue<f64>,
    pub cell_padding: KeyOrValue<f64>,
    pub selection_color: KeyOrValue<Color>,
    pub focus_color: KeyOrValue<Color>,
}

#[derive(Clone, Data, Debug)]
pub struct ResolvedTableConfig {
    pub(crate) col_header_height: f64,
    pub(crate) row_header_width: f64,
    pub(crate) header_background: Color,
    pub(crate) header_selected_background: Color,
    pub(crate) cells_background: Color,
    pub(crate) cells_border: Color,
    pub(crate) cell_border_thickness: f64,
    pub(crate) cell_padding: f64,
    pub(crate) selection_color: Color,
    pub(crate) focus_color: Color,
}

impl ResolvedTableConfig {
    pub(crate) fn cross_axis_length(&self, axis: &TableAxis) -> f64 {
        match axis {
            TableAxis::Columns => self.col_header_height,
            TableAxis::Rows => self.row_header_width,
        }
    }
}

impl Default for TableConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl TableConfig {
    pub fn new() -> TableConfig {
        TableConfig {
            col_header_height: DEFAULT_COL_HEADER_HEIGHT.into(),
            row_header_width: DEFAULT_ROW_HEADER_WIDTH.into(),
            header_background: theme::BACKGROUND_DARK.into(),
            header_selected_background: theme::PLACEHOLDER_COLOR.into(),
            cells_background: theme::LABEL_COLOR.into(),
            cells_border: theme::BORDER_LIGHT.into(),
            cell_border_thickness: 0.5.into(),
            cell_padding: 2.0.into(),
            selection_color: Color::rgb8(0xB0, 0xEE, 0xFF).into(),
            focus_color: Color::rgb8(0x4D, 0x58, 0xD8).into(),
        }
    }

    pub(crate) fn resolve(&self, env: &Env) -> ResolvedTableConfig {
        ResolvedTableConfig {
            row_header_width: self.row_header_width.resolve(env),
            col_header_height: self.col_header_height.resolve(env),
            header_background: self.header_background.resolve(env),
            header_selected_background: self.header_selected_background.resolve(env),
            cells_background: self.cells_background.resolve(env),
            cells_border: self.cells_border.resolve(env),
            cell_border_thickness: self.cell_border_thickness.resolve(env),
            cell_padding: self.cell_padding.resolve(env),
            selection_color: self.selection_color.resolve(env),
            focus_color: self.focus_color.resolve(env),
        }
    }
}
