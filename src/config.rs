use druid::{KeyOrValue, Color, theme, Env};

pub(crate) const DEFAULT_HEADER_HEIGHT: f64 = 25.0;

#[derive(Clone)]
pub struct TableConfig {
    pub header_height: KeyOrValue<f64>,
    pub header_background: KeyOrValue<Color>,
    pub cells_background: KeyOrValue<Color>,
    pub cells_border: KeyOrValue<Color>,
    pub cell_border_thickness: KeyOrValue<f64>,
    pub cell_padding: KeyOrValue<f64>,
}

pub struct ResolvedTableConfig {
    pub(crate) header_height: f64,
    pub(crate) header_background: Color,
    pub(crate) cells_background: Color,
    pub(crate) cells_border: Color,
    pub(crate) cell_border_thickness: f64,
    pub(crate) cell_padding: f64,
}

impl Default for TableConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl TableConfig {
    pub fn new() -> TableConfig {
        TableConfig {
            header_height: DEFAULT_HEADER_HEIGHT.into(),
            header_background: theme::BACKGROUND_LIGHT.into(),
            cells_background: theme::LABEL_COLOR.into(),
            cells_border: theme::BORDER_LIGHT.into(),
            cell_border_thickness: 1.0.into(),
            cell_padding: 2.0.into(),
        }
    }

    pub(crate) fn resolve(&self, env: &Env) -> ResolvedTableConfig {
        ResolvedTableConfig {
            header_height: self.header_height.resolve(env),
            header_background: self.header_background.resolve(env),
            cells_background: self.cells_background.resolve(env),
            cells_border: self.cells_border.resolve(env),
            cell_border_thickness: self.cell_border_thickness.resolve(env),
            cell_padding: self.cell_padding.resolve(env),
        }
    }
}