use crate::TableAxis;
use druid::kurbo::Line;
use druid::piet::IntoBrush;
use druid::{Point, Rect, RenderContext};

pub(crate) trait RenderContextExt: RenderContext {
    fn stroke_bottom_right_border(
        &mut self,
        cell_rect: &Rect,
        border: &impl IntoBrush<Self>,
        border_thickness: f64,
    ) {
        self.stroke_bottom_border(cell_rect, border, border_thickness);
        self.stroke_right_border(cell_rect, border, border_thickness);
    }

    fn stroke_bottom_border(
        &mut self,
        cell_rect: &Rect,
        border: &impl IntoBrush<Self>,
        border_thickness: f64,
    ) {
        let y_extent = cell_rect.y1 + border_thickness / 2.;
        self.stroke(
            Line::new(
                Point::new(cell_rect.x0, y_extent),
                Point::new(cell_rect.x1 + border_thickness + 0.5, y_extent),
            ),
            border,
            border_thickness,
        );
    }

    fn stroke_top_border(
        &mut self,
        cell_rect: &Rect,
        border: &impl IntoBrush<Self>,
        border_thickness: f64,
    ) {
        let y_extent = cell_rect.y0 - border_thickness / 2.;
        self.stroke(
            Line::new(
                Point::new(cell_rect.x0, y_extent),
                Point::new(cell_rect.x1 + border_thickness + 0.5, y_extent),
            ),
            border,
            border_thickness,
        );
    }

    fn stroke_right_border(
        &mut self,
        cell_rect: &Rect,
        border: &impl IntoBrush<Self>,
        border_thickness: f64,
    ) {
        let x_extent = cell_rect.x1 + border_thickness / 2.;
        self.stroke(
            Line::new(
                Point::new(x_extent, cell_rect.y0 - 0.5),
                Point::new(x_extent, cell_rect.y1 + 0.5),
            ),
            border,
            border_thickness,
        );
    }

    fn stroke_left_border(
        &mut self,
        cell_rect: &Rect,
        border: &impl IntoBrush<Self>,
        border_thickness: f64,
    ) {
        let x = cell_rect.x0 - border_thickness / 2.;
        self.stroke(
            Line::new(
                Point::new(x, cell_rect.y0 - 0.5),
                Point::new(x, cell_rect.y1 + 0.5),
            ),
            border,
            border_thickness,
        );
    }

    fn stroke_trailing_main_border(
        &mut self,
        axis: TableAxis,
        cell_rect: &Rect,
        brush: &impl IntoBrush<Self>,
        border_thickness: f64,
    ) {
        match axis {
            TableAxis::Columns => self.stroke_left_border(cell_rect, brush, border_thickness),
            TableAxis::Rows => self.stroke_top_border(cell_rect, brush, border_thickness),
        }
    }
}

impl<R: RenderContext> RenderContextExt for R {}
