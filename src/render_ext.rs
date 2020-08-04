use druid::kurbo::Line;
use druid::piet::IntoBrush;
use druid::{Point, Rect, RenderContext};

pub(crate) trait RenderContextExt: RenderContext {
    fn stroke_bottom_left_border(
        &mut self,
        cell_rect: &Rect,
        border: &impl IntoBrush<Self>,
        border_thickness: f64,
    ) {
        let half_border = border_thickness / 2.;
        let x_extent = cell_rect.x1 + half_border;
        let y_extent = cell_rect.y1 + half_border;
        self.stroke(
            Line::new(
                Point::new(x_extent, cell_rect.y0 - 0.5),
                Point::new(x_extent, cell_rect.y1 + 0.5),
            ),
            border,
            border_thickness,
        );
        self.stroke(
            Line::new(
                Point::new(cell_rect.x0, y_extent),
                Point::new(cell_rect.x1 + border_thickness + 0.5, y_extent),
            ),
            border,
            border_thickness,
        );
    }
}

impl<R: RenderContext> RenderContextExt for R {}
