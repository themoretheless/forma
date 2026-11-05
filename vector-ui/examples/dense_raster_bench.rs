//! Public single-control CPU raster: cold build and animated repaint + RGBA copy.
use forma::Button;
use std::{hint::black_box,time::Instant};
fn stats(values:&mut [f64])->serde_json::Value {
    values.sort_by(f64::total_cmp);
    serde_json::json!({"p50_ms":values[values.len()/2],"p95_ms":values[(values.len()*95/100).min(values.len()-1)]})
}
fn main(){
    let background=std::env::args().any(|arg|arg=="--background");
    let mut rows=Vec::new();
    for (width,height,scale) in [(800,600,1.),(1600,1200,2.)] {
        for (name,content) in [("full",""),("geometry",""),("mixed","Text { text:'你好 мир🙂'; fontSize:14; color:#ffffff; }"),("nested","ContentClip { x:3; y:4; width:100; height:60; radius:3; } ContentClip { x:8; y:9; width:40; height:30; radius:2; } ContentShape { points:'10 10 25 10 25 25 10 25'; color:#ff000080; } ContentClipEnd {} ContentClipEnd {}")] {
            let source=if name=="full" {
                "component App { Frame { width:800; height:600; Button { x:0; y:0; width:800; height:600; } } }"
            } else {
                "component App { Frame { width:800; height:600; Button { x:30; y:40; width:160; height:80; } } }"
            };
            let component=format!("component Button {{ Rectangle {{ radius:4; background:#abcdef80; Reveal {{ color:#ffaa44; }} {content} }} }}");
            let mut cold=Vec::new();
            for _ in 0..25 {
                let model=Button::from_sources(source,&component).unwrap();
                let start=Instant::now();black_box(if background {model.pixels(width,height,scale)} else {model.content_pixels(width,height,scale)});
                cold.push(start.elapsed().as_secs_f64()*1000.);
            }
            let mut model=Button::from_sources(source,&component).unwrap();
            let mut warm=Vec::new();
            for n in 0..50 {
                let before=model.raster_stats();
                let start=Instant::now();
                model.pointer(40.+n as f32,60.,0);model.tick(1000./60.);
                black_box(if background {model.pixels(width,height,scale)} else {model.content_pixels(width,height,scale)});
                let ms=start.elapsed().as_secs_f64()*1000.;
                let after=model.raster_stats();
                if n>0 {assert_eq!(after[0],before[0]);assert_eq!(after[1],before[1]+1,"fixture must repaint");}
                if n>=10 {warm.push(ms);}
            }
            rows.push(serde_json::json!({"name":name,"width":width,"height":height,"scale":scale,"cold":stats(&mut cold),"repaint":stats(&mut warm),"cold_samples":cold,"repaint_samples":warm}));
        }
    }
    println!("{}",serde_json::json!({"scope":"single-control dense pixels; cold build and tick/repaint/RGBA copy; no allocator instrumentation or OS presentation","background":background,"rows":rows}));
}
