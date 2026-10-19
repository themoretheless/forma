//! Shared native/WASM polygon coverage rasterizer for composed vector content.
//! Uses the same exact-area rasterizer as text, so CPU shapes match the GPU
//! path; simple polygons give the same result under nonzero and even-odd.
use crate::text::{rasterize, Point};

pub fn draw(out:&mut [u8],width:u32,height:u32,scale:f32,points:&[[f32;2]],origin:[f32;2],color:[u8;4]) {
    if points.len()<3||color[3]==0{return;}
    let physical:Vec<Point>=points.iter().map(|p|Point{x:(p[0]+origin[0])*scale,y:(p[1]+origin[1])*scale}).collect();
    let edges:Vec<(Point,Point)>=(0..physical.len()).map(|i|(physical[i],physical[(i+1)%physical.len()])).collect();
    rasterize(out,width,&edges,[0.,0.,width as f32,height as f32],color);
}
