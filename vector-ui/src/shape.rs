//! Shared native/WASM polygon coverage rasterizer for composed vector content.
//! Uses the same exact-area rasterizer as text, so CPU shapes match the GPU
//! path; simple polygons give the same result under nonzero and even-odd.
use std::cell::RefCell;
use crate::text::{rasterize, Point};

thread_local! {
    static SHAPE_EDGES: RefCell<Vec<(Point, Point)>> = const { RefCell::new(Vec::new()) };
}

pub fn draw(out:&mut [u8],width:u32,height:u32,scale:f32,points:&[[f32;2]],origin:[f32;2],color:[u8;4]) {
    if points.len()<3||color[3]==0{return;}
    let mut edges = SHAPE_EDGES.with(|cell| std::mem::take(&mut *cell.borrow_mut()));
    edges.clear();
    edges.reserve(points.len());
    let to_pt = |p: &[f32; 2]| Point { x: (p[0] + origin[0]) * scale, y: (p[1] + origin[1]) * scale };
    let first = to_pt(&points[0]);
    let mut prev = first;
    for p in &points[1..] {
        let next = to_pt(p);
        edges.push((prev, next));
        prev = next;
    }
    edges.push((prev, first));
    rasterize(out,width,&edges,[0.,0.,width as f32,height as f32],color);
    SHAPE_EDGES.with(|cell| *cell.borrow_mut() = edges);
}
