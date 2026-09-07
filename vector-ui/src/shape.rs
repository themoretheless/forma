//! Shared native/WASM polygon coverage rasterizer for composed vector content.
pub fn draw(out:&mut [u8],width:u32,height:u32,scale:f32,points:&[[f32;2]],origin:[f32;2],color:[u8;4]) {
    if points.len()<3||color[3]==0{return;}
    let points:Vec<[f32;2]>=points.iter().map(|p|[(p[0]+origin[0])*scale,(p[1]+origin[1])*scale]).collect();
    let min_x=points.iter().map(|p|p[0]).fold(f32::INFINITY,f32::min).floor().clamp(0.,width as f32)as u32;
    let max_x=points.iter().map(|p|p[0]).fold(f32::NEG_INFINITY,f32::max).ceil().clamp(0.,width as f32)as u32;
    let min_y=points.iter().map(|p|p[1]).fold(f32::INFINITY,f32::min).floor().clamp(0.,height as f32)as u32;
    let max_y=points.iter().map(|p|p[1]).fold(f32::NEG_INFINITY,f32::max).ceil().clamp(0.,height as f32)as u32;
    // Scanline intersections, even-odd coverage; four samples per pixel.
    let mut coverage=vec![0u8;(max_x-min_x)as usize];
    let mut hits=Vec::with_capacity(points.len());
    for y in min_y..max_y {
        coverage.fill(0);
        for sy in [0.25,0.75] {
            let scan=y as f32+sy;hits.clear();
            for i in 0..points.len(){let a=points[i];let b=points[(i+1)%points.len()];if (a[1]<=scan&&b[1]>scan)||(b[1]<=scan&&a[1]>scan){hits.push(a[0]+(scan-a[1])*(b[0]-a[0])/(b[1]-a[1]));}}
            hits.sort_by(f32::total_cmp);
            for pair in hits.chunks_exact(2){for x in (pair[0].floor().max(min_x as f32)as u32)..(pair[1].ceil().min(max_x as f32).max(0.)as u32){for sx in [0.25,0.75]{if x as f32+sx>=pair[0]&&x as f32+sx<pair[1]{coverage[(x-min_x)as usize]+=1;}}}}
        }
        for x in min_x..max_x {let alpha=coverage[(x-min_x)as usize]as f32/4.*color[3]as f32/255.;if alpha==0.{continue;}let i=((y*width+x)*4)as usize;let retained=out[i+3]as f32/255.*(1.-alpha);let total=alpha+retained;for k in 0..3{out[i+k]=((out[i+k]as f32*retained+color[k]as f32*alpha)/total).round()as u8;}out[i+3]=(total*255.).round()as u8;}
    }
}
