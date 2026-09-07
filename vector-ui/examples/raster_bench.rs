//! Headless timings exclude window upload/presentation and include returned RGBA allocation.
use std::{hint::black_box, time::Instant};
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let source = args.get(1).map(|p| std::fs::read_to_string(p).unwrap()).unwrap_or(forma::EXAMPLE.into());
    let template = args.get(2).map(|p| std::fs::read_to_string(p).unwrap()).unwrap_or(forma::BUTTON_COMPONENT.into());
    for (w,h,scale) in [(400,200,1.),(800,400,2.),(1600,1000,2.)] {
        let mut b=forma::Button::from_sources(&source,&template).unwrap();
        let t=Instant::now();black_box(b.pixels(w,h,scale));let cold=t.elapsed().as_secs_f64()*1000.;
        let t=Instant::now();for _ in 0..5{black_box(b.pixels(w,h,scale));}let warm=t.elapsed().as_secs_f64()*200.;
        let bounds=b.bounds();b.pointer(bounds[0]+bounds[2]/2.,bounds[1]+bounds[3]/2.,0);
        let t=Instant::now();for _ in 0..5{b.tick(16.);black_box(b.pixels(w,h,scale));}let animated=t.elapsed().as_secs_f64()*200.;
        println!("{w}x{h}: cold={cold:.3}ms warm={warm:.3}ms animation={animated:.3}ms");
    }
}
