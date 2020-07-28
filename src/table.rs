use std::marker::PhantomData;
use std::ops::DerefMut;

use druid::widget::prelude::*;
use druid::{Data, Env, Lens, Color, Point, Rect, Widget, EventCtx, LifeCycle, PaintCtx, BoxConstraints, LifeCycleCtx, Size, LayoutCtx, Event, UpdateCtx, Affine, WidgetExt, theme, KeyOrValue};
use druid::kurbo::Line;
use druid::piet::{PietFont, FontBuilder, Text, TextLayout, TextLayoutBuilder};
use im::Vector;
use std::rc::Rc;
use std::cell::RefCell;
use druid::widget::{Scroll, SCROLL_TO, ScrollTo, Flex, CrossAxisAlignment, Align};

pub trait CellRender<T>{
    fn paint(&mut self, ctx: &mut PaintCtx, row_idx: usize, col_idx: usize , data: &T, env: &Env);
}

impl <T> CellRender<T> for Box<dyn CellRender<T>> {
    fn paint(&mut self, ctx: &mut PaintCtx, row_idx: usize, col_idx: usize , data: &T, env: &Env) {
        self.deref_mut().paint(ctx, row_idx, col_idx, data, env);
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


    fn paint(&mut self, ctx: &mut PaintCtx, row_idx: usize, col_idx:usize, data: &T, env: &Env) {
        let inner = &mut self.inner;
        self.lens.with(data, |inner_data|{
            inner.paint(ctx, row_idx, col_idx, inner_data, env);
        })
    }
}

pub struct TextCell{
    text_color: KeyOrValue<Color>,
    font_name: KeyOrValue<&'static str>,
    font_size: KeyOrValue<f64>,
    cached_font: Option<PietFont>
}

impl TextCell{
    pub fn new() ->TextCell{
        TextCell{
            text_color: Color::BLACK.into(),
            font_name: theme::FONT_NAME.into(),
            font_size: theme::TEXT_SIZE_NORMAL.into(),
            cached_font: None
        }
    }

    pub fn text_color(mut self, text_color: impl Into<KeyOrValue<Color>>) -> TextCell {
        self.text_color = text_color.into();
        self
    }

    pub fn font_name(mut self, font_name: impl Into<KeyOrValue<&'static str>>) -> TextCell {
        self.font_name = font_name.into();
        self
    }

    pub fn font_size(mut self, font_size: impl Into<KeyOrValue<f64>>) -> TextCell {
        self.font_size = font_size.into();
        self
    }

}

impl CellRender<String> for TextCell {
    fn paint(&mut self, ctx: &mut PaintCtx, row_idx: usize, col_idx:usize, data: &String, env: &Env) {
        if self.cached_font.is_none(){
            let font: PietFont = ctx
                .text()
                .new_font_by_name(self.font_name.resolve(env), self.font_size.resolve(env) )
                .build()
                .unwrap();
            self.cached_font = Some(font);
        }

        // Here's where we actually use the UI state
        let layout = ctx
            .text()
            .new_text_layout(self.cached_font.as_ref().unwrap(), &data, std::f64::INFINITY)
            .build()
            .unwrap();

        let fill_color = self.text_color.resolve(env);
        ctx.draw_text(&layout, (0.0, layout.line_metric(0).unwrap().height), &fill_color);
    }
}

struct TableColumn<T: Data, CR: CellRender<T>>{
    header: String,
    cell_render: CR,
    phantom_: PhantomData<T>
}

pub struct TableConfig<T : Data>{
    table_columns: Vec<TableColumn<T, Box<dyn CellRender<T>>>>,
    column_header_render: Box<dyn CellRender<String>>,
    header_background: KeyOrValue<Color>,
    cells_background: KeyOrValue<Color>,
    cells_border: KeyOrValue<Color>,
    cell_border_thickness: KeyOrValue<f64>,
    cell_padding: KeyOrValue<f64>
}

impl <T:Data> TableConfig<T>{
    pub fn new() -> TableConfig<T>{
        TableConfig {
            table_columns: Vec::<TableColumn<T, Box<dyn CellRender<T>>>>::new(),
            column_header_render: Box::new(TextCell::new().text_color(theme::PRIMARY_LIGHT)),
            header_background: theme::BACKGROUND_LIGHT.into(),
            cells_background: theme::LABEL_COLOR.into(),
            cells_border: theme::BORDER_LIGHT.into(),
            cell_border_thickness: 1.0.into(),
            cell_padding: 2.0.into()
        }
    }

    pub fn with_column<CR: CellRender<T> + 'static>(mut self, header: impl Into<String>, cell_render: CR) ->Self{
        self.add_column(header, cell_render);
        self
    }

    pub fn add_column<CR: CellRender<T> + 'static>(&mut self, header: impl Into<String>, cell_render: CR) {
        self.table_columns.push(TableColumn {
            header: header.into(),
            cell_render: Box::new(cell_render),
            phantom_: PhantomData::default()
        });
    }

    pub fn build_widget(self) -> Align<im::Vector<T>> {
        let shared_config = Rc::new(RefCell::new(self));

        let ch_id = WidgetId::next();

        let headings = ColumnHeadings(Rc::clone(&shared_config)); //.lens(TableState::items);

        let ch_scroll = Scroll::new(headings).with_id(ch_id);
        let mut cells_scroll = Scroll::new(Cells(Rc::clone(&shared_config) )); // .lens(TableState::items));
        cells_scroll.add_scroll_handler(move|ctxt, pos|{
            ctxt.submit_command(SCROLL_TO.with(ScrollTo::x(pos.x)),
                                ch_id
            );
        });
        let col = Flex::column()
            .cross_axis_alignment(CrossAxisAlignment::Start )
            .with_child(ch_scroll)
            .with_flex_child(cells_scroll, 1.)
            .center();
        col
    }

    fn columns(&self)->usize{
        self.table_columns.len()
    }

    //TODO: Measure content or fixed sizes per axis
    fn cell_size(&self, _data: &Vector<T>, env: &Env)->Size{
        let border_thickness = self.cell_border_thickness.resolve(env);

        let col_width = 100.0;
        let width = border_thickness + col_width;

        let row_height = 40.0;
        let height = border_thickness + row_height;

        Size::new(width, height)
    }

}

pub struct ColumnHeadings<T:Data>(pub Rc<RefCell<TableConfig<T>>>);

impl <T : Data> Widget<Vector<T>> for ColumnHeadings <T>{
    fn event(&mut self, _ctx: &mut EventCtx, _event: &Event, _data: &mut Vector<T>, _env: &Env) {

    }

    fn lifecycle(&mut self, _ctx: &mut LifeCycleCtx, _event: &LifeCycle, _data: &Vector<T>, _env: &Env) {

    }

    fn update(&mut self, _ctx: &mut UpdateCtx, _old_data: &Vector<T>, _data: &Vector<T>, _env: &Env) {

    }

    fn layout(&mut self, _ctx: &mut LayoutCtx, bc: &BoxConstraints, data: &Vector<T>, env: &Env) -> Size {
        bc.debug_check("ColumnHeadings");
        let table_config: &TableConfig<T> = &self.0.borrow();
        let cell_size = table_config.cell_size(data, env);
        bc.constrain( Size::new(cell_size.width * (table_config.columns() as f64), cell_size.height ))
    }

    fn paint(&mut self, ctx: &mut PaintCtx, _data: &Vector<T>, env: &Env) {
        let rect = ctx.region().to_rect();
        let table_config: &mut TableConfig<T> = &mut self.0.borrow_mut();

        ctx.fill(rect, &table_config.header_background.resolve(env));

        let cell_size = Size::new(100.0, 40.0); // TODO: column and row size policies
        let border_thickness = table_config.cell_border_thickness.resolve(env);
        let border = table_config.cells_border.resolve(env);
        let padding = table_config.cell_padding.resolve(env);

        let mut cell_left = 0.;
        let row_top = 0.;

        for (col_idx, col) in table_config.table_columns.iter_mut().enumerate() {
            let cell_rect = Rect::from_origin_size( Point::new(cell_left, row_top), cell_size );
            let padded_rect = cell_rect.inset(-padding);

            let header_render = &mut table_config.column_header_render;

            ctx.with_save(|ctx| {
                let layout_origin = padded_rect.origin().to_vec2();
                ctx.transform(Affine::translate(layout_origin));
                ctx.with_child_ctx(padded_rect, |ctxt| {
                    header_render.paint(ctxt, 0, col_idx,&col.header, env);
                });
            });
            ctx.stroke( Line::new(
                Point::new(cell_rect.x1, cell_rect.y0),
                Point::new (cell_rect.x1, cell_rect.y1)
            ), &border, border_thickness);
            ctx.stroke( Line::new(
                Point::new(cell_rect.x0, cell_rect.y1),
                Point::new (cell_rect.x1, cell_rect.y1)
            ), &border, border_thickness);

            cell_left = cell_rect.x1 + border_thickness;
        }

    }

}

pub struct Cells<T:Data>(pub Rc<RefCell<TableConfig<T>>>);

impl <T : Data> Widget<Vector<T>> for Cells<T>{
    fn event(&mut self, _ctx: &mut EventCtx, _event: &Event, _data: &mut Vector<T>, _env: &Env) {

    }

    fn lifecycle(&mut self, _ctx: &mut LifeCycleCtx, _event: &LifeCycle, _data: &Vector<T>, _env: &Env) {

    }

    fn update(&mut self, _ctx: &mut UpdateCtx, _old_data: &Vector<T>, _data: &Vector<T>, _env: &Env) {

    }

    fn layout(&mut self, _ctx: &mut LayoutCtx, bc: &BoxConstraints, data: &Vector<T>, env: &Env) -> Size {
        bc.debug_check("TableCells");
        let table_config: &TableConfig<T> = &self.0.borrow();
        let cell_size = table_config.cell_size(data, env);
        bc.constrain( Size::new(cell_size.width * (table_config.columns() as f64), cell_size.height *  (data.len() as f64)))
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &Vector<T>, env: &Env) {
        let mut config = self.0.borrow_mut();
        let rect = ctx.region().to_rect();

        ctx.fill(rect, &config.cells_background.resolve(env) );

        let cell_size = Size::new(100.0, 40.0);
        let border_thickness = 1.0;
        let padding = 2.0;

        let mut row_top = 0.;
        for (row_idx, row) in data.iter().enumerate() {

            let mut cell_left = 0.;

            for (col_idx, col) in config.table_columns.iter_mut().enumerate() {
                let cell_rect = Rect::from_origin_size( Point::new(cell_left, row_top), cell_size );
                let padded_rect = cell_rect.inset(-padding);

                ctx.with_save(|ctx| {
                    let layout_origin = padded_rect.origin().to_vec2();
                    ctx.transform(Affine::translate(layout_origin));
                    ctx.with_child_ctx(padded_rect, |ctxt| {
                        col.cell_render.paint(ctxt, row_idx, col_idx, row, env);
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

            row_top += cell_size.height + border_thickness;
        }



    }

}
