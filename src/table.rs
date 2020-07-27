use std::marker::PhantomData;
use std::ops::DerefMut;

use druid::widget::prelude::*;
use druid::{Data, Env, Lens, Color, Point, Rect,
            Widget, EventCtx, LifeCycle, PaintCtx, BoxConstraints,
            LifeCycleCtx, Size, LayoutCtx, Event, UpdateCtx, Affine};
use druid::kurbo::Line;
use druid::piet::{FontBuilder, Text, TextLayout, TextLayoutBuilder};
use im::Vector;

pub trait CellRender<T>{
    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env);
}

impl <T> CellRender<T> for Box<dyn CellRender<T>> {
    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
        self.deref_mut().paint(ctx, data, env);
    }
}

pub struct LensWrapCR<U, L, W> {
    inner: W,
    lens: L,
    // The following is a workaround for otherwise getting E0207.
    phantom: PhantomData<U>,
}

impl <U, L, W> LensWrapCR<U, L, W>{
    fn new(inner: W,
           lens: L) -> LensWrapCR<U, L, W> {
        LensWrapCR{
            inner, lens, phantom: PhantomData::default()
        }
    }
}

pub trait CellRenderExt<T: Data>: CellRender<T> + Sized + 'static{
    fn lens<S: Data, L: Lens<S, T>>(self, lens: L) -> LensWrapCR<T, L, Self> {
        LensWrapCR::new(self, lens)
    }
}

impl<T: Data, CR: CellRender<T> + 'static> CellRenderExt<T> for CR {}

impl<T, U, L, CR> CellRender<T> for LensWrapCR<U, L, CR>
    where
        T: Data,
        U: Data,
        L: Lens<T, U>,
        CR: CellRender<U>,
{
    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
        let inner = &mut self.inner;
        self.lens.with(data, |inner_data|{
            inner.paint(ctx, inner_data, env);
        })
    }
}

pub struct TextCell{

}

impl TextCell{
    pub fn new() ->TextCell{
        TextCell{}
    }
}

impl CellRender<String> for TextCell {
    fn paint(&mut self, ctx: &mut PaintCtx, data: &String, _env: &Env) {
        let font = ctx
            .text()
            .new_font_by_name("Segoe UI", 14.0)
            .build()
            .unwrap();
        // Here's where we actually use the UI state
        let layout = ctx
            .text()
            .new_text_layout(&font, &data, std::f64::INFINITY)
            .build()
            .unwrap();

        let fill_color = Color::BLACK;
        ctx.draw_text(&layout, (0.0, layout.line_metric(0).unwrap().height), &fill_color);
    }
}

struct TableColumn<T: Data, CR: CellRender<T>>{
    header: String,
    cell_render: CR,
    phantom_: PhantomData<T>
}

pub struct Table<T : Data>{
    table_columns: Vec<TableColumn<T, Box<dyn CellRender<T>>>>,
    phantom_: PhantomData<T>
}

impl <T:Data> Table<T>{
    pub fn new() -> Table<T>{
        Table{
            table_columns: Vec::<TableColumn<T, Box<dyn CellRender<T>>>>::new(),
            phantom_: PhantomData::default()
        }
    }

    pub fn add_column<CR: CellRender<T> + 'static>(mut self, header: impl Into<String>, cell_render: CR) ->Self{
        self.table_columns.push(TableColumn{
            header: header.into(),
            cell_render: Box::new(cell_render),
            phantom_: PhantomData::default()
        });
        self
    }
}

impl <T : Data> Widget<Vector<T>> for Table<T>{
    fn event(&mut self, _ctx: &mut EventCtx, _event: &Event, _data: &mut Vector<T>, _env: &Env) {

    }

    fn lifecycle(&mut self, _ctx: &mut LifeCycleCtx, _event: &LifeCycle, _data: &Vector<T>, _env: &Env) {

    }

    fn update(&mut self, _ctx: &mut UpdateCtx, _old_data: &Vector<T>, _data: &Vector<T>, _env: &Env) {

    }

    fn layout(&mut self, _ctx: &mut LayoutCtx, bc: &BoxConstraints, _data: &Vector<T>, _env: &Env) -> Size {
        bc.max()
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &Vector<T>, env: &Env) {
        let rect = ctx.region().to_rect();

        ctx.fill(rect, &Color::WHITE);

        let cell_size = Size::new(100.0, 40.0);
        let border_thickness = 1.0;
        let padding = 2.0;

        let reg_rect = ctx.region().to_rect();
        let mut row_top = reg_rect.y0;
        for row in data {

            let mut cell_left = reg_rect.x0;
            for col in &mut self.table_columns{
                let cell_rect = Rect::from_origin_size( Point::new(cell_left, row_top), cell_size );
                let padded_rect = cell_rect.inset(-padding);

                ctx.with_save(|ctx| {
                    let layout_origin = padded_rect.origin().to_vec2();
                    ctx.transform(Affine::translate(layout_origin));
                    ctx.with_child_ctx(padded_rect, |ctxt| {
                        col.cell_render.paint(ctxt, row, env);
                    });
                });
                ctx.stroke( Line::new(
                    Point::new(cell_rect.x1, cell_rect.y0),
                    Point::new (cell_rect.x1, cell_rect.y1)
                ), &Color::BLACK, border_thickness);
                ctx.stroke( Line::new(
                    Point::new(cell_rect.x0, cell_rect.y1),
                    Point::new (cell_rect.x1, cell_rect.y1)
                ), &Color::BLACK, border_thickness);

                cell_left = cell_rect.x1 + border_thickness;
            }

            //let fill_color = Color::rgba8(0x00, 0x00, 0x00, 0x7F);
            //ctx.draw_text(&layout, (10.0, row_top + layout.line_metric(0).unwrap().height), &fill_color);

            row_top += cell_size.height + border_thickness;
        }



    }

}


