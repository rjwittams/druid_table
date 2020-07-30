use druid::{RenderContext, Rect, Point};
use druid::kurbo::Line;
use druid::piet::IntoBrush;

pub(crate) trait RenderContextExt: RenderContext {
    fn stroke_bottom_left_border(
        &mut self,
        cell_rect: &Rect,
        border: &impl IntoBrush<Self>,
        border_thickness: f64,
    ) {
        self.stroke(
            Line::new(
                Point::new(cell_rect.x1, cell_rect.y0),
                Point::new(cell_rect.x1, cell_rect.y1),
            ),
            border,
            border_thickness,
        );
        self.stroke(
            Line::new(
                Point::new(cell_rect.x0, cell_rect.y1),
                Point::new(cell_rect.x1, cell_rect.y1),
            ),
            border,
            border_thickness,
        );
    }
}

impl<R: RenderContext> RenderContextExt for R {}
