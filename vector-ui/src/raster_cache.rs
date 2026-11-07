//! Single-entry, per-instance raster cache. A new model owns a new cache.
//! Geometry/content depend on viewport, DPI and scroll, not animated colors.
use crate::{round_coverage, shape, template::Content, text, Button};

#[derive(Clone, Copy, PartialEq)]
struct GeometryKey {
    width: u32,
    height: u32,
    scale: f32,
    scroll: [f32; 2],
    sparse: bool,
}
#[derive(Clone, Copy, PartialEq)]
struct PaintKey {
    fill: [u8; 4],
    border: [u8; 4],
    reveal: crate::reveal::Paint,
    background: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FrameRun {
    // Exclusive global pixel index; raster size is bounded below u32::MAX.
    end: u32,
    value: u8,
}

pub(crate) struct Cache {
    key: GeometryKey,
    // Four original coverage samples: fill_count * 5 + border_count.
    coverage: Vec<u8>,
    // Precise outer/border boundary coverage for square rectangles. Canvas
    // indices survive sparse packing; solid interiors use the usual palette.
    rect_edges: Vec<(usize, f32, f32)>,
    // Sparse, stable border coverage; pointer motion only changes its paint.
    reveal_pixels: Vec<(usize, f32)>,
    // Low five bits: 0..16 frame coverage. Upper bits: 0 clipped,
    // 1 content, 2 scrollbar. Equal consecutive samples share a run.
    frame: Vec<FrameRun>,
    content: Vec<u8>,
    // One span per nonempty row; content is compacted to these spans after build.
    content_spans: Vec<std::ops::Range<usize>>,
    paint: Option<PaintKey>,
    pixels: Vec<u8>,
    // Packed pixels retain their original canvas indices, so fractional DPI
    // and overflowing content use exactly the same coverage samples.
    spans: Vec<std::ops::Range<usize>>,
    native_pixels: Vec<u32>,
}

/// Straight-alpha source-over, same rounding as the outline renderer.
pub(crate) fn over(dst: &mut [u8], src: &[u8]) {
    if src[3] == 0 {
        return;
    }
    if src[3] == 255 || dst[3] == 0 {
        dst.copy_from_slice(src);
        return;
    }
    let sa = src[3] as f32 / 255.;
    let retained = dst[3] as f32 / 255. * (1. - sa);
    let a = sa + retained;
    for k in 0..3 {
        dst[k] = ((src[k] as f32 * sa + dst[k] as f32 * retained) / a).round() as u8;
    }
    dst[3] = (a * 255.).round() as u8;
}

// Conservative support of geometry plus content, or content alone. Clip
// markers only remove support; overflowing text/shapes must remain visible.
fn raster_region(model: &Button, key: GeometryKey, include_geometry: bool) -> [u32;4] {
    let full=[0,0,key.width,key.height];
    let b=&model.scene.button;let x=b.x-model.scroll_x;let y=b.y-model.scroll_y;
    let mut support:Option<[u32;4]>=None;let mut invalid=false;
    let mut add=|bounds:[f32;4]| {
        if !bounds.iter().all(|n|n.is_finite()) || bounds[2]<0. || bounds[3]<0. {invalid=true;return;}
        let physical=[bounds[0]*key.scale,bounds[1]*key.scale,(bounds[0]+bounds[2])*key.scale,(bounds[1]+bounds[3])*key.scale];
        if !physical.iter().all(|n|n.is_finite()) {invalid=true;return;}
        let rect=[(physical[0]-1.).floor().clamp(0.,key.width as f32) as u32,
            (physical[1]-1.).floor().clamp(0.,key.height as f32) as u32,
            (physical[2]+1.).ceil().clamp(0.,key.width as f32) as u32,
            (physical[3]+1.).ceil().clamp(0.,key.height as f32) as u32];
        if rect[0]>=rect[2]||rect[1]>=rect[3] {return;}
        support=Some(match support {None=>rect,Some(old)=>[old[0].min(rect[0]),old[1].min(rect[1]),old[2].max(rect[2]),old[3].max(rect[3])]});
    };
    if include_geometry { add([x,y,b.width,b.height]); }
    if model.template.text.is_some() {
        let r=b.radius.min(b.width/2.).min(b.height/2.);
        add([x+r*0.3,y+r*0.3,b.width-r*0.6,b.height-r*0.6]);
    }
    for content in &model.template.content {
        match content {
            Content::Text {bounds,..}=>add([x+bounds[0],y+bounds[1],bounds[2],bounds[3]]),
            Content::Shape {points,..}=>{
                if points.is_empty() {continue;}
                let mut bounds=[f32::INFINITY,f32::INFINITY,f32::NEG_INFINITY,f32::NEG_INFINITY];
                for point in points {
                    if !point.iter().all(|n|n.is_finite()) {return full;}
                    bounds[0]=bounds[0].min(point[0]);bounds[1]=bounds[1].min(point[1]);
                    bounds[2]=bounds[2].max(point[0]);bounds[3]=bounds[3].max(point[1]);
                }
                add([x+bounds[0],y+bounds[1],bounds[2]-bounds[0],bounds[3]-bounds[1]]);
            }
            Content::Clip {..}|Content::ClipEnd=>{}
        }
    }
    if invalid {return full;}
    support.map_or([0;4],|[x0,y0,x1,y1]|[x0,y0,x1-x0,y1-y0])
}

#[cfg(test)]
thread_local! { static FRAME_SAMPLES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

fn frame_value(model: &Button, key: GeometryKey, v: [f32; 4], x: u32, y: u32) -> u8 {
    #[cfg(test)]
    FRAME_SAMPLES.with(|count| count.set(count.get() + 1));
    let [vx, vy, vw, vh] = v;
    let b = &model.scene.button;
    let mut frame = (round_coverage(
        x,
        y,
        key.scale,
        [0., 0., model.width(), model.height()],
        model.scene.radius,
    ) * 16.) as u8;
    let mut viewport = 1;
    if model.scene.scroll {
        let px = (x as f32 + 0.5) / key.scale;
        let py = (y as f32 + 0.5) / key.scale;
        if px < vx || py < vy || px >= vx + vw || py >= vy + vh {
            viewport = 0;
        }
        if vw > 0. && vh > 0. {
            let ch = b.height;
            let cw = b.width;
            let vertical = ch > vh
                && px >= (vx + vw - 5.).max(vx)
                && px < vx + vw
                && py >= vy + model.scroll_y / ch * vh
                && py < vy + (model.scroll_y + vh) / ch * vh;
            let horizontal = cw > vw
                && py >= (vy + vh - 5.).max(vy)
                && py < vy + vh
                && px >= vx + model.scroll_x / cw * vw
                && px < vx + (model.scroll_x + vw) / cw * vw;
            if vertical || horizontal {
                viewport = 2;
            }
        }
    }
    frame |= viewport << 5;
    frame
}

fn append_frame_run(runs: &mut Vec<FrameRun>, end: u32, value: u8) {
    match runs.last_mut() {
        Some(run) if run.value == value => run.end = end,
        _ => runs.push(FrameRun { end, value }),
    }
}

// All x comparisons use the original pixel-center arithmetic. This avoids
// floor/ceil conversions changing inclusive/exclusive viewport boundaries.
fn frame_x_boundary(width: u32, scale: f32, threshold: f32) -> u32 {
    let (mut lo, mut hi) = (0, width);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if (mid as f32 + 0.5) / scale >= threshold { hi = mid; }
        else { lo = mid + 1; }
    }
    lo
}

fn frame_runs(model: &Button, key: GeometryKey) -> Vec<FrameRun> {
    let mut runs = Vec::new();
    if key.sparse { return runs; }
    let v = model.viewport_rect();
    let [vx, _, vw, vh] = v;
    let b = &model.scene.button;
    let mut thresholds = vec![model.width() / 2.];
    if model.scene.scroll {
        thresholds.extend([vx, vx + vw]);
        if vw > 0. && vh > 0. {
            thresholds.push((vx + vw - 5.).max(vx));
            if b.width > vw {
                thresholds.extend([
                    vx + model.scroll_x / b.width * vw,
                    vx + (model.scroll_x + vw) / b.width * vw,
                ]);
            }
        }
    }
    let monotone = key.width >= 64 && model.width().is_finite()
        && model.height().is_finite() && model.width() > 0. && model.height() > 0.
        && model.scene.radius.is_finite() && model.scene.radius >= 0.
        && v.iter().chain(thresholds.iter()).all(|n| n.is_finite());
    let mut cuts = vec![0, key.width];
    if monotone {
        cuts.extend(thresholds.iter().map(|&x| frame_x_boundary(key.width, key.scale, x)));
        cuts.sort_unstable(); cuts.dedup();
    }
    for y in 0..key.height {
        if !monotone {
            for x in 0..key.width {
                append_frame_run(&mut runs, y * key.width + x + 1, frame_value(model, key, v, x, y));
            }
            continue;
        }
        // Each interval has constant viewport/scrollbar classification and
        // lies on one side of the rounded frame center. Its coverage is
        // monotone, so an equal-value prefix cannot recur after it ends.
        for interval in cuts.windows(2) {
            let mut x = interval[0];
            while x < interval[1] {
                let value = frame_value(model, key, v, x, y);
                let (mut lo, mut hi) = (x + 1, interval[1]);
                while lo < hi {
                    let mid = lo + (hi - lo) / 2;
                    if frame_value(model, key, v, mid, y) == value { lo = mid + 1; }
                    else { hi = mid; }
                }
                append_frame_run(&mut runs, y * key.width + lo, value);
                x = lo;
            }
        }
    }
    runs
}

impl Cache {
    fn build(model: &Button, key: GeometryKey) -> Self {
        Self::build_with_content_region(model, key, raster_region(model, key, false))
    }

    fn build_with_content_region(model: &Button, key: GeometryKey, content_region: [u32; 4]) -> Self {
        let GeometryKey {
            width,
            height,
            scale,
            ..
        } = key;
        let region=raster_region(model,key,true);
        let [rx,ry,rw,rh]=region;
        let offscreen=rw==0||rh==0;
        let len=(rw*rh) as usize;
        let storage_index=|x:u32,y:u32|((y-ry)*rw+x-rx) as usize;
        // Clip markers cannot produce pixels by themselves. A geometry-only
        // control needs no temporary RGBA content layer, even on the dense path.
        let [cx, cy, cw, ch] = content_region;
        let content_len = (cw * ch) as usize;
        let has_content = content_len > 0;
        let content_index = |x: u32, y: u32| ((y - cy) * cw + x - cx) as usize;
        let mut cache = Self {
            key,
            coverage: vec![0; len],
            rect_edges: Vec::new(),
            reveal_pixels: Vec::new(),
            frame: Vec::new(),
            content: vec![0; content_len * 4],
            content_spans: Vec::new(),
            paint: None,
            pixels: Vec::new(),
            spans: Vec::new(),
            native_pixels: Vec::new(),
        };
        if offscreen && key.sparse { return cache; }
        let b = &model.scene.button;
        let bx = b.x - model.scroll_x;
        let by = b.y - model.scroll_y;
        let r = b.radius.min(b.width / 2.).min(b.height / 2.);
        let bw = model.template.border.as_ref().map_or(0., |b| b.width);
        // No geometry evaluation in the empty part of the canvas.
        let x0 = (bx * scale).floor().clamp(0., width as f32) as u32;
        let y0 = (by * scale).floor().clamp(0., height as f32) as u32;
        let x1 = ((bx + b.width) * scale).ceil().clamp(0., width as f32) as u32;
        let y1 = ((by + b.height) * scale).ceil().clamp(0., height as f32) as u32;
        let half_w = b.width * 0.5;
        let half_h = b.height * 0.5;
        let center_x = bx + half_w;
        let center_y = by + half_h;
        let corner_w = half_w - r;
        let corner_h = half_h - r;
        let reveal_bounds = model.template.reveal.as_ref().map(|_| model.reveal_bounds());
        let reveal_r = model
            .template
            .reveal
            .as_ref()
            .map_or(r, |rev| rev.target_radius.unwrap_or(r));
        let reveal_w = model.template.reveal.as_ref().map_or(0., |rev| rev.width);
        let default_reveal_bounds = model.template.reveal.as_ref().map_or(true, |rev| {
            rev.target_x.is_none()
                && rev.target_y.is_none()
                && rev.target_width.is_none()
                && rev.target_height.is_none()
                && rev.target_radius.is_none()
        });
        let safe_inset = r.max(bw).max(if default_reveal_bounds {
            reveal_w + 0.5 / scale
        } else {
            f32::MAX
        }) + 1.0 / scale;
        let ix0 = ((bx + safe_inset) * scale).ceil().clamp(x0 as f32, x1 as f32) as u32;
        let iy0 = ((by + safe_inset) * scale).ceil().clamp(y0 as f32, y1 as f32) as u32;
        let ix1 = ((bx + b.width - safe_inset) * scale)
            .floor()
            .clamp(x0 as f32, x1 as f32) as u32;
        let iy1 = ((by + b.height - safe_inset) * scale)
            .floor()
            .clamp(y0 as f32, y1 as f32) as u32;
        for y in y0..y1 {
            let row_inside = y >= iy0 && y < iy1 && ix1 > ix0;
            for x in x0..x1 {
                if row_inside && x >= ix0 && x < ix1 {
                    cache.coverage[storage_index(x,y)] = 20;
                    continue;
                }
                if r == 0. {
                    let area = crate::rect_area_coverage(x, y, scale, [bx, by, b.width, b.height]);
                    let inner = crate::rect_area_coverage(x, y, scale, [bx + bw, by + bw, b.width - 2. * bw, b.height - 2. * bw]);
                    let band = (area - inner).max(0.);
                    let index = (y * width + x) as usize;
                    cache.coverage[storage_index(x,y)] = if area == 0. { 0 } else if area == 1. && band == 1. { 24 } else { 20 };
                    if area > 0. && (area < 1. || (band > 0. && band < 1.)) {
                        cache.rect_edges.push((index, area, band));
                    }
                } else {
                    let mut fill = 0;
                    let mut border = 0;
                    for (sx, sy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                        let dx = ((x as f32 + sx) / scale - center_x).abs() - corner_w;
                        let dy = ((y as f32 + sy) / scale - center_y).abs() - corner_h;
                        let distance = if dx <= 0. && dy <= 0. {
                            dx.max(dy) - r
                        } else {
                            dx.max(0.).hypot(dy.max(0.)) + dx.max(dy).min(0.) - r
                        };
                        if distance <= 0. {
                            fill += 1;
                            if bw > 0. && distance >= -bw {
                                border += 1;
                            }
                        }
                    }
                    cache.coverage[storage_index(x,y)] = fill * 5 + border;
                }
                if let Some(r_bounds) = reveal_bounds {
                    let band = crate::reveal::band_coverage(
                        [(x as f32 + 0.5) / scale, (y as f32 + 0.5) / scale],
                        r_bounds,
                        reveal_r,
                        reveal_w,
                        scale,
                    );
                    if band > 0. {
                        cache.reveal_pixels.push(((y * width + x) as usize, band));
                    }
                }
            }
        }
        cache.frame = frame_runs(model, key);
        if let Some(t) = &model.template.text {
            text::draw_text_region(
                &mut cache.content,
                width,
                height,
                scale,
                &t.text,
                t.font_size,
                [
                    bx + r * 0.3,
                    by + r * 0.3,
                    b.width - r * 0.6,
                    b.height - r * 0.6,
                ],
                t.color,content_region,
            );
        }
        let mut groups = Vec::new();
        // Sibling clip groups reuse the same layer. Only nesting depth, rather
        // than total group count, determines region scratch allocations.
        let mut layers: Vec<Vec<u8>> = Vec::new();
        for c in model.template.content.iter().filter(|_| has_content) {
            match c {
                Content::Text { bounds, text: t } => text::draw_text_region(
                    &mut cache.content,
                    width,
                    height,
                    scale,
                    &t.text,
                    t.font_size,
                    [bx + bounds[0], by + bounds[1], bounds[2], bounds[3]],
                    t.color,content_region,
                ),
                Content::Shape { points, color } => shape::draw_region(
                    &mut cache.content,
                    width,
                    height,
                    scale,
                    points,
                    [bx, by],
                    *color,content_region,
                ),
                Content::Clip { bounds, radius } => {
                    let layer = match layers.pop() {
                        Some(mut layer) => { layer.fill(0); layer }
                        None => vec![0; content_len * 4],
                    };
                    groups.push((
                        std::mem::replace(&mut cache.content, layer),
                        [bounds[0] + bx, bounds[1] + by, bounds[2], bounds[3]],
                        *radius,
                    ));
                }
                Content::ClipEnd => {
                    if let Some((mut parent, bounds, radius)) = groups.pop() {
                        // round_coverage has no support outside these bounds;
                        // one physical pixel conservatively includes every AA sample.
                        let [x0, y0, x1, y1] = clip_pixels(bounds, scale, width, height);
                        let (x0,y0,x1,y1)=(x0.max(cx),y0.max(cy),x1.min(cx+cw),y1.min(cy+ch));
                        for y in y0..y1 {
                            for x in x0..x1 {
                                let i = content_index(x,y)*4;
                                if cache.content[i + 3] == 0 {
                                    continue;
                                }
                                let mut pixel: [u8; 4] =
                                    cache.content[i..i + 4].try_into().unwrap();
                                pixel[3] = (pixel[3] as f32
                                    * round_coverage(x, y, scale, bounds, radius))
                                .round() as u8;
                                over(&mut parent[i..i + 4], &pixel);
                            }
                        }
                        layers.push(std::mem::replace(&mut cache.content, parent));
                    }
                }
            }
        }
        drop(layers);
        drop(groups);
        // Most canvases contain only a few rows of text/shapes. Keep their exact
        // RGBA samples, without retaining four bytes for every transparent pixel.
        let mut compacted = 0;
        for y in 0..ch as usize {
            let row_start = y * cw as usize;
            let row = &cache.content[row_start * 4..(row_start + cw as usize) * 4];
            if let Some(first_alpha_byte) = row.iter().skip(3).step_by(4).position(|&a| a != 0) {
                let first = first_alpha_byte;
                let last = row
                    .iter()
                    .skip(3)
                    .step_by(4)
                    .rposition(|&a| a != 0)
                    .unwrap_or(first);
                let span = row_start + first..row_start + last + 1;
                let bytes = span.len() * 4;
                cache.content.copy_within(span.start * 4..span.end * 4, compacted);
                compacted += bytes;
                let global_row=(y+cy as usize)*width as usize+cx as usize;
                cache.content_spans.push(global_row+first..global_row+last+1);
            }
        }
        cache.content.truncate(compacted);
        cache.content.shrink_to_fit();
        {
            // Each row is the union of the button's possible animated paint
            // (including reveal) and actual static content. Content may extend
            // outside the button, including on rows above/below its rectangle.
            let mut content_spans = cache.content_spans.iter().peekable();
            let mut packed = 0;
            for y in ry..ry+rh {
                let row = (y * width) as usize;
                let mut start = width as usize;
                let mut end = 0;
                if y >= y0 && y < y1 && x0 < x1 {
                    start = x0 as usize;
                    end = x1 as usize;
                }
                if let Some(span) = content_spans.peek() {
                    if span.start / width as usize == y as usize {
                        start = start.min(span.start - row);
                        end = end.max(span.end - row);
                        content_spans.next();
                    }
                }
                if start < end {
                    let span = row + start..row + end;
                    let local_start=storage_index(start as u32,y);
                    cache.coverage.copy_within(local_start..local_start+span.len(), packed);
                    packed += span.len();
                    cache.spans.push(span);
                }
            }
            cache.coverage.truncate(packed);
            cache.coverage.shrink_to_fit();
        }
        let pixel_len = if key.sparse { cache.coverage.len() } else { width as usize * height as usize };
        cache.pixels.resize(pixel_len * 4, 0);
        cache
    }

    fn paint(&mut self, model: &Button, key: PaintKey) {
        // Only 15 combinations are possible; evaluate color mixing once for each,
        // instead of four samples and square roots for every pixel on every frame.
        let mut palette = [[0u8; 4]; 25];
        let fa = key.fill[3] as f32 / 255.;
        let ba = key.border[3] as f32 / 255.;
        for f in 0..=4 {
            for b in 0..=f {
                let a = (fa * (f - b) as f32 + (fa * (1. - ba) + ba) * b as f32) / 4.;
                if a > 0. {
                    for k in 0..3 {
                        palette[f * 5 + b][k] = ((key.fill[k] as f32 * fa * (f - b) as f32
                            + (key.fill[k] as f32 * fa * (1. - ba) + key.border[k] as f32 * ba)
                                * b as f32)
                            / 4.
                            / a)
                            .round() as u8;
                    }
                }
                palette[f * 5 + b][3] = (a * 255.).round() as u8;
            }
        }
        let mut reveal_pixels = self.reveal_pixels.iter().peekable();
        let mut rect_edges = self.rect_edges.iter().peekable();
        let mut content_spans = self.content_spans.iter();
        let mut content_span = content_spans.next();
        let mut content_pixels = self.content.chunks_exact(4);
        if self.key.sparse {
            let indices = self.spans.iter().flat_map(|span| span.clone());
            for ((i, dst), packed) in indices.zip(self.pixels.chunks_exact_mut(4)).zip(0..) {
                dst.copy_from_slice(&palette[self.coverage[packed] as usize]);
                if let Some(&&(index, area, band)) = rect_edges.peek() {
                    if index == i {
                        dst.copy_from_slice(&crate::rect_area_paint(key.fill, key.border, area, band));
                        rect_edges.next();
                    }
                }
                if let Some(span) = content_span {
                    if i >= span.start {
                        over(dst, content_pixels.next().unwrap());
                        if i + 1 == span.end {
                            content_span = content_spans.next();
                        }
                    }
                }
                if let Some(&&(index, band)) = reveal_pixels.peek() {
                    if index == i {
                        let x = (i % self.key.width as usize) as f32;
                        let y = (i / self.key.width as usize) as f32;
                        let mut color = key.reveal.color_at([(x + 0.5) / self.key.scale, (y + 0.5) / self.key.scale]);
                        color[3] = (color[3] as f32 * band).round() as u8;
                        over(dst, &color);
                        reveal_pixels.next();
                    }
                }
            }
        } else {
            // Base geometry and static content occupy packed global spans;
            // only the final frame/background composition needs a dense pass.
            self.pixels.fill(0);
            let mut packed = 0;
            for span in &self.spans {
                for (dst, &coverage) in self.pixels[span.start * 4..span.end * 4]
                    .chunks_exact_mut(4)
                    .zip(&self.coverage[packed..packed + span.len()])
                {
                    dst.copy_from_slice(&palette[coverage as usize]);
                }
                packed += span.len();
            }
            for &(i, area, band) in &self.rect_edges {
                self.pixels[i * 4..i * 4 + 4].copy_from_slice(
                    &crate::rect_area_paint(key.fill, key.border, area, band));
            }
            let mut packed = 0;
            for span in &self.content_spans {
                let bytes = span.len() * 4;
                for (dst, src) in self.pixels[span.start * 4..span.end * 4]
                    .chunks_exact_mut(4).zip(self.content[packed..packed + bytes].chunks_exact(4))
                {
                    over(dst, src);
                }
                packed += bytes;
            }
            for &(i, band) in &self.reveal_pixels {
                let x = (i % self.key.width as usize) as f32;
                let y = (i / self.key.width as usize) as f32;
                let mut color = key.reveal.color_at([(x + 0.5) / self.key.scale, (y + 0.5) / self.key.scale]);
                color[3] = (color[3] as f32 * band).round() as u8;
                over(&mut self.pixels[i * 4..i * 4 + 4], &color);
            }
            let mut start = 0;
            for &FrameRun { end, value: frame } in &self.frame {
                let pixels = &mut self.pixels[start * 4..end as usize * 4];
                start = end as usize;
                match frame >> 5 {
                    0 => pixels.fill(0),
                    2 => {
                        for dst in pixels.chunks_exact_mut(4) {
                            dst.copy_from_slice(&[110, 130, 170, 255]);
                        }
                    }
                    _ => {}
                }
                let frame_coverage = frame & 31;
                let coverage = frame_coverage as f32 / 16.;
                if key.background {
                    let mut background = model.scene.background;
                    if !model.scene.clip {
                        background[3] = (background[3] as f32 * coverage).round() as u8;
                    }
                    for dst in pixels.chunks_exact_mut(4) {
                        if dst[3] == 0 {
                            dst.copy_from_slice(&background);
                        } else if dst[3] != 255 {
                            let mut bg = background;
                            over(&mut bg, dst);
                            dst.copy_from_slice(&bg);
                        }
                    }
                }
                if model.scene.clip && frame_coverage != 16 {
                    for dst in pixels.chunks_exact_mut(4) {
                        dst[3] = (dst[3] as f32 * coverage).round() as u8;
                        if dst[3] == 0 {
                            dst.fill(0);
                        }
                    }
                }
            }
        }
        self.paint = Some(key);
        self.native_pixels.clear();
    }
}

impl Button {
    fn cached(
        &self,
        width: u32,
        height: u32,
        scale: f32,
        background: bool,
        sparse: bool,
    ) -> Option<std::cell::RefMut<'_, Cache>> {
        if !valid_raster_size(width, height) || !scale.is_finite() || scale <= 0. {
            return None;
        }
        let geometry = GeometryKey {
            width,
            height,
            scale,
            scroll: [self.scroll_x, self.scroll_y],
            sparse,
        };
        let paint = PaintKey {
            fill: self.fill.color(),
            border: self.border.color(),
            reveal: self.reveal_paint(),
            background,
        };
        let mut stored = self.raster_cache.borrow_mut();
        if stored.as_ref().is_none_or(|cache| cache.key != geometry) {
            *stored = Some(Cache::build(self, geometry));
            self.raster_builds
                .set(self.raster_builds.get().wrapping_add(1));
        }
        let cache = stored.as_mut().unwrap();
        if cache.paint != Some(paint) {
            cache.paint(self, paint);
            self.raster_paints
                .set(self.raster_paints.get().wrapping_add(1));
        }
        Some(std::cell::RefMut::map(stored, |cache| {
            cache.as_mut().unwrap()
        }))
    }

    pub(crate) fn raster_cached(
        &self,
        width: u32,
        height: u32,
        scale: f32,
        background: bool,
    ) -> Vec<u8> {
        self.cached(width, height, scale, background, false)
            .map_or_else(Vec::new, |cache| cache.pixels.clone())
    }

    /// Copy RGBA output into caller-owned storage, retaining its allocation.
    pub fn paint_rgba(&self, dst: &mut [u8], width: u32, height: u32, scale: f32,
        background: bool) -> Result<(), &'static str> {
        if (width as usize).checked_mul(height as usize).and_then(|n| n.checked_mul(4)) != Some(dst.len()) {
            return Err("Invalid RGBA buffer size");
        }
        let cache = self.cached(width, height, scale, background, false)
            .ok_or("Visible scene exceeds CPU raster budget")?;
        dst.copy_from_slice(&cache.pixels);
        Ok(())
    }

    /// Composite a leaf without allocating/copying a transparent full canvas.
    /// Root frame clipping and scrollbars are applied once by Runtime.
    pub(crate) fn composite_content(&self, dst: &mut [u8], width: u32, height: u32, scale: f32) {
        debug_assert!(!self.scene.clip && !self.scene.scroll);
        let Some(cache) = self.cached(width, height, scale, false, true) else { return };
        let mut pixels = cache.pixels.as_slice();
        for span in &cache.spans {
            let bytes = span.len() * 4;
            for (dst, src) in dst[span.start * 4..span.end * 4]
                .chunks_exact_mut(4).zip(pixels[..bytes].chunks_exact(4)) {
                over(dst, src);
            }
            pixels = &pixels[bytes..];
        }
    }

    /// Paint into the host-owned XRGB buffer without a full-window RGBA copy.
    /// The current runtime has fixed logical layout: resizing the host does not
    /// change it. Keep that scene cached at native DPI and crop/copy on resize.
    /// This intentionally does not stretch content or lower quality while dragging.
    pub fn paint_native(
        &self,
        dst: &mut [u32],
        width: u32,
        height: u32,
        scale: f32,
    ) -> Result<(), &'static str> {
        let len = (width as usize)
            .checked_mul(height as usize)
            .ok_or("Native buffer size overflow")?;
        if width == 0 || height == 0 || len != dst.len() || !scale.is_finite() || scale <= 0. {
            return Err("Invalid native buffer or DPI");
        }
        let [right, bottom] = self.paint_extent();
        let mut rw = (right * scale).ceil().max(1.) as u32;
        let mut rh = (bottom * scale).ceil().max(1.) as u32;
        // Do not allocate an unbounded offscreen scene. For oversized content,
        // retain only the visible portion (the existing raster safety limit).
        if !valid_raster_size(rw, rh) {
            rw = rw.min(width);
            rh = rh.min(height);
        }
        let mut cache = self
            .cached(rw, rh, scale, true, false)
            .ok_or("Visible scene exceeds CPU raster budget")?;
        if cache.native_pixels.is_empty() {
            let Cache { native_pixels, pixels, .. } = &mut *cache;
            native_pixels.reserve(pixels.len() / 4);
            native_pixels.extend(pixels.chunks_exact(4).map(|p| {
                let a = p[3] as u32;
                if a == 255 {
                    ((p[0] as u32) << 16) | ((p[1] as u32) << 8) | (p[2] as u32)
                } else if a == 0 {
                    0x111319
                } else {
                    let c = |k: usize, bg: u32| (p[k] as u32 * a + bg * (255 - a) + 127) / 255;
                    (c(0, 17) << 16) | (c(1, 19) << 8) | c(2, 25)
                }
            }));
        }
        let pixels = &cache.native_pixels;
        let copy_width = width.min(rw) as usize;
        let copy_height = height.min(rh) as usize;
        for y in 0..copy_height {
            let row = &mut dst[y * width as usize..(y + 1) * width as usize];
            row[..copy_width]
                .copy_from_slice(&pixels[y * rw as usize..y * rw as usize + copy_width]);
            row[copy_width..].fill(0x111319);
        }
        dst[copy_height * width as usize..].fill(0x111319);
        Ok(())
    }

    // Conservative bounds include overflowing template primitives, not just the
    // Button box. Clip/Scroll restrict them to the root viewport. Text already
    // clips itself to its own bounds; an extra pixel covers fractional edges.
    fn paint_extent(&self) -> [f32; 2] {
        let mut extent = [self.width(), self.height()];
        if self.scene.clip || self.scene.scroll {
            return extent;
        }
        let b = &self.scene.button;
        let x = b.x - self.scroll_x;
        let y = b.y - self.scroll_y;
        extent[0] = extent[0].max(x + b.width);
        extent[1] = extent[1].max(y + b.height);
        for content in &self.template.content {
            match content {
                Content::Text { bounds, .. } => {
                    extent[0] = extent[0].max(x + bounds[0] + bounds[2] + 1.);
                    extent[1] = extent[1].max(y + bounds[1] + bounds[3] + 1.);
                }
                Content::Shape { points, .. } => {
                    for p in points {
                        extent[0] = extent[0].max(x + p[0]);
                        extent[1] = extent[1].max(y + p[1]);
                    }
                }
                _ => {}
            }
        }
        extent
    }
}

fn valid_raster_size(width: u32, height: u32) -> bool {
    width > 0
        && height > 0
        && width <= 4096
        && height <= 4096
        && width as u64 * height as u64 <= 8_388_608
}

// Physical support of a rounded clip, retaining a full-pixel AA margin before
// clamping. Empty clips remain empty even when the margin would create a box.
fn clip_pixels(bounds: [f32; 4], scale: f32, width: u32, height: u32) -> [u32; 4] {
    if bounds[2] <= 0. || bounds[3] <= 0. {
        return [0; 4];
    }
    [
        (bounds[0] * scale - 1.).floor().clamp(0., width as f32) as u32,
        (bounds[1] * scale - 1.).floor().clamp(0., height as f32) as u32,
        ((bounds[0] + bounds[2]) * scale + 1.).ceil().clamp(0., width as f32) as u32,
        ((bounds[1] + bounds[3]) * scale + 1.).ceil().clamp(0., height as f32) as u32,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_run_search_matches_every_scalar_sample_at_viewport_and_aa_boundaries() {
        let mut model=Button::from_sources(
            "component App { Frame { width:100; height:80; Button { width:160; height:120; } } }",
            "component Button { Rectangle { background:#abcdef; } }"
        ).unwrap();
        let mut seed=7u32;
        let mut random=|| {seed=seed.wrapping_mul(1664525).wrapping_add(1013904223);seed as f32/u32::MAX as f32};
        for case in 0..240 {
            model.scene.width=0.125+random()*260.;model.scene.height=0.125+random()*120.;
            model.scene.radius=if case%3==0 {0.} else {random()*200.};
            model.scene.scroll=case%2==0;
            model.scene.padding=[random()*20.,random()*20.,random()*20.,random()*20.];
            model.scene.button.width=random()*400.;model.scene.button.height=random()*300.;
            model.scroll_x=random()*200.;model.scroll_y=random()*200.;
            let width=[1,16,63,64,79,128,257][case%7];
            let height=[1,17,47,96][case%4];
            let scale=[f32::MIN_POSITIVE,0.000001,0.125,0.75,1.,1.25,2.,3.5,1_000_000.,f32::MAX][case%10];
            let key=GeometryKey {width,height,scale,scroll:[model.scroll_x,model.scroll_y],sparse:false};
            let runs=frame_runs(&model,key);let v=model.viewport_rect();
            let mut start=0;let mut previous=None;
            for run in runs {
                assert!(run.end>start&&run.end<=width*height);
                assert_ne!(previous,Some(run.value));
                for i in start..run.end {
                    assert_eq!(run.value,frame_value(&model,key,v,i%width,i/width),"case={case},pixel={i},scale={scale}");
                }
                start=run.end;previous=Some(run.value);
            }
            assert_eq!(start,width*height);
        }
        // Nonfinite/degenerate geometry uses the original scalar path.
        for radius in [-1.,f32::NAN,f32::INFINITY] {
            model.scene.radius=radius;
            let key=GeometryKey {width:80,height:20,scale:1.25,scroll:[0.;2],sparse:false};
            let runs=frame_runs(&model,key);let v=model.viewport_rect();let mut expected=Vec::new();
            for y in 0..20 {for x in 0..80 {append_frame_run(&mut expected,y*80+x+1,frame_value(&model,key,v,x,y));}}
            assert_eq!(runs,expected);
        }
    }

    #[test]
    fn uniform_frame_storage_is_constant_size_even_with_offscreen_geometry() {
        let component="component Button { Rectangle { background:#abcdef; } }";
        for x in [30,3000] {
            let source=format!("component App {{ Frame {{ width:800; height:600; Button {{ x:{x}; y:40; width:160; height:80; }} }} }}");
            let model=Button::from_sources(&source,component).unwrap();
            let key=GeometryKey {width:1600,height:1200,scale:2.,scroll:[0.;2],sparse:false};
            FRAME_SAMPLES.with(|samples|samples.set(0));
            let cache=Cache::build(&model,key);
            let samples=FRAME_SAMPLES.with(|samples|samples.get());
            eprintln!("uniform1600x1200 frame: {samples} scalar samples instead of1,920,000");
            assert!(samples<100_000,"uniform frame reverted to per-pixel evaluation: {samples}");
            assert_eq!(cache.frame,vec![FrameRun {end:1_920_000,value:48}]);
            if x==30 {
                FRAME_SAMPLES.with(|samples|samples.set(0));
                let mut scalar=Vec::new();let v=model.viewport_rect();
                for py in 0..key.height {for px in 0..key.width {
                    append_frame_run(&mut scalar,py*key.width+px+1,frame_value(&model,key,v,px,py));
                }}
                let scalar_samples=FRAME_SAMPLES.with(|samples|samples.get());
                eprintln!("scalar reference frame: {scalar_samples} samples");
                assert_eq!(scalar_samples,1_920_000);
                assert_eq!(scalar,cache.frame);
            }
            assert!(cache.frame.capacity()*std::mem::size_of::<FrameRun>()<=64);
            assert_eq!(cache.pixels.len(),1_920_000*4);
        }
    }

    #[test]
    fn cropped_content_matches_full_canvas_storage_exactly() {
        let source = "component App { Frame { width:180; height:140; radius:7; background:#12345680; Button { x:31.3; y:23.7; width:68; height:49; } } }";
        let components = [
            "component Button { Rectangle { radius:4; background:#abcdef80; Text { text:'你好 мир🙂'; fontSize:12; color:#ffffff; } } }",
            "component Button { Rectangle { background:#abcdef80; ContentText { x:-20; y:-12; width:110; height:21; text:'Яg🙂'; fontSize:16; color:#ffccaa80; } ContentClip { x:-6; y:-8; width:85; height:74; radius:6; } ContentShape { points:'-18 -14 80 -14 80 66 -18 66'; color:#ff000080; } ContentClip { x:12.3; y:8.7; width:40; height:30; radius:3; } ContentShape { points:'-2 -2 60 0 45 44'; color:#00ff0080; } ContentClipEnd {} ContentClipEnd {} } }",
            "component Button { Rectangle { background:#abcdef80; ContentShape { points:'-130 -100 -110 -100 -110 -80'; color:#ffffff; } } }",
            "component Button { Rectangle { ContentClip { x:0; y:0; width:20; height:30; } ContentClipEnd {} } }",
        ];
        for component in components {
            let mut model = Button::from_sources(source, component).unwrap();
            for scroll in [[0.,0.],[27.4,19.8],[300.,300.]] {
                model.scroll_x=scroll[0]; model.scroll_y=scroll[1];
                for scale in [0.75,1.,1.25,1.5,2.] {
                    for sparse in [false,true] {
                        for background in [false,true] {
                            let key=GeometryKey {width:240,height:190,scale,scroll,sparse};
                            let mut actual=Cache::build(&model,key);
                            let mut full=Cache::build_with_content_region(&model,key,[0,0,240,190]);
                            let paint=PaintKey {fill:model.fill.color(),border:model.border.color(),
                                reveal:model.reveal_paint(),background};
                            actual.paint(&model,paint); full.paint(&model,paint);
                            assert!(actual.pixels == full.pixels,"scale={scale}, sparse={sparse}, scroll={scroll:?}");
                            assert_eq!(actual.content_spans,full.content_spans);
                            assert_eq!(actual.spans,full.spans);
                            // An offscreen sparse cache returns before compaction;
                            // its unused scratch storage is intentionally different.
                            if !actual.spans.is_empty() {
                                assert!(actual.content == full.content);
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn geometry_only_content_matches_transparent_layer_reference() {
        let source = "component App { Frame { width:160; height:120; Button { x:11; y:7; width:64; height:48; } } }";
        let plain = "component Button { Rectangle { background:#abcdef; } }";
        for scale in [0.75, 1., 1.25, 2.] {
            for sparse in [false, true] {
                let model = Button::from_sources(source, plain).unwrap();
                let key = GeometryKey { width:200, height:150, scale, scroll:[0.;2], sparse };
                let mut empty = Cache::build(&model, key);
                assert_eq!(empty.content.capacity(), 0);
                // Force the original RGBA-layer path with a zero-alpha shape.
                let reference = Button::from_sources(source,
                    "component Button { Rectangle { background:#abcdef; ContentShape { points:'0 0 10 0 10 10'; color:#00000000; } } }"
                ).unwrap();
                let mut full = Cache::build(&reference, key);
                let paint = PaintKey { fill:[0xab,0xcd,0xef,255], border:[0;4],
                    reveal:model.reveal_paint(), background:false };
                empty.paint(&model, paint);
                full.paint(&reference, paint);
                assert_eq!(empty.pixels, full.pixels);
                assert_eq!(empty.spans, full.spans);
                assert_eq!(empty.frame, full.frame);
            }
        }
    }

    #[test]
    fn offscreen_sparse_cache_has_no_canvas_storage_and_preserves_overflow() {
        let source = "component App { Frame { width:200; height:100; Button { x:500; y:300; width:40; height:30; } } }";
        let plain = "component Button { Rectangle { background:#abcdef; } }";
        for scale in [0.75,1.,1.25,2.] {
            let mut model = Button::from_sources(source,plain).unwrap();
            let mut out = vec![0;200*100*4];
            model.composite_content(&mut out,200,100,scale);
            assert!(out.iter().all(|&x|x==0));
            {
                let cache = model.raster_cache.borrow();let cache=cache.as_ref().unwrap();
                assert_eq!(cache.coverage.capacity()+cache.content.capacity()+cache.pixels.capacity()+cache.frame.capacity(),0);
            }
            model.scroll_x=500.;model.scroll_y=300.;
            out.fill(0);model.composite_content(&mut out,200,100,scale);
            assert!(out.chunks_exact(4).any(|p|p[3]>0),"scroll must rebuild the previously empty cache");
            // Shape reaches the canvas despite the offscreen parent bounds.
            let overflowing = "component Button { Rectangle { background:#abcdef; ContentShape { points:'-490 -290 -470 -290 -470 -270 -490 -270'; color:#ff0000; } } }";
            let model = Button::from_sources(source,overflowing).unwrap();
            out.fill(0);model.composite_content(&mut out,200,100,scale);
            assert_eq!(out,model.content_pixels(200,100,scale));
            assert!(out.chunks_exact(4).any(|p|p[3]>0),"overflow must remain visible");
        }
    }


    #[test]
    fn packed_leaf_retains_only_local_rows_and_empty_offscreen_leaf_is_valid() {
        let source = "component Demo { Frame { width:1000; height:600; padding:0; Button { x:700.3; y:400.7; width:40; height:24; } } }";
        let component = "component Button { Rectangle { radius:4; background:#abcdef80; Reveal { color:#ffaa44; } ContentText { x:-6; y:-14; width:60; height:12; text:'Я'; fontSize:11; color:#ffffff; } ContentShape { points:'-8 3 6 3 6 8 -8 8'; color:#334455; } } }";
        let model = Button::from_sources(source, component).unwrap();
        let expected = model.content_pixels(1000, 600, 1.25);
        let mut actual = vec![0; expected.len()];
        model.composite_content(&mut actual, 1000, 600, 1.25);
        assert_eq!(actual, expected);
        {
            let stored = model.raster_cache.borrow();
            let cache = stored.as_ref().unwrap();
            assert!(cache.key.sparse && cache.frame.is_empty());
            assert_eq!(cache.pixels.len(), cache.coverage.len() * 4);
            assert_eq!(cache.coverage.len(), cache.spans.iter().map(|span| span.len()).sum::<usize>());
            assert!(cache.coverage.capacity() < 3000, "packed geometry must not retain a full canvas");
            assert!(cache.pixels.capacity() < 12_000, "packed RGBA must not retain a full canvas");
            assert!(cache.content.capacity() < 2000);
            assert!(cache.spans.windows(2).all(|pair| pair[0].end <= pair[1].start));
            assert!(cache.spans.iter().all(|span| span.start < span.end && span.start / 1000 == (span.end - 1) / 1000));
        }
        let mut outside = vec![0; 20 * 20 * 4];
        model.composite_content(&mut outside, 20, 20, 1.25);
        assert!(outside.iter().all(|&b| b == 0));
        let stored = model.raster_cache.borrow();
        let cache = stored.as_ref().unwrap();
        assert!(cache.spans.is_empty() && cache.pixels.is_empty() && cache.coverage.is_empty());
    }

    #[test]
    fn sparse_content_retains_only_occupied_rows_without_changing_pixels() {
        let source = "component Demo { Frame { width:160; height:120; padding:0; Button { width:64; height:48; } } }";
        let component = "component Button { Rectangle { background:#00000000; ContentShape { points:'20 10 25 10 25 15 20 15'; color:#abcdef; } } }";
        let model = Button::from_sources(source, component).unwrap();
        let pixels = model.content_pixels(160, 120, 1.);
        for y in 0..120 {
            for x in 0..160 {
                let expected = if (20..25).contains(&x) && (10..15).contains(&y) {
                    [0xab, 0xcd, 0xef, 255]
                } else {
                    [0; 4]
                };
                assert_eq!(&pixels[(y * 160 + x) * 4..(y * 160 + x + 1) * 4], &expected);
            }
        }
        let cache = model.raster_cache.borrow();
        let cache = cache.as_ref().unwrap();
        let retained = cache.content.capacity()
            + cache.content_spans.capacity() * std::mem::size_of::<std::ops::Range<usize>>();
        assert!(retained < 1024, "a 25-pixel shape must not retain a full-canvas RGBA layer: {retained}");
    }

    #[test]
    fn clip_support_contains_all_fractional_coverage_samples() {
        for scale in [0.75, 1., 1.25, 2.] {
            for bounds in [
                [-4.3, -2.1, 13.7, 9.2],
                [7.1, 5.6, 0.2, 0.4],
                [3.3, 4.8, 18.1, 12.5],
                [100., 100., 10., 10.],
                [5., 5., 0., 8.],
            ] {
                let [x0, y0, x1, y1] = clip_pixels(bounds, scale, 48, 40);
                for radius in [0., 2.5, 50.] {
                    for y in 0..40 {
                        for x in 0..48 {
                            if x < x0 || x >= x1 || y < y0 || y >= y1 {
                                assert_eq!(round_coverage(x, y, scale, bounds, radius), 0.);
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn pooled_sibling_and_nested_clips_preserve_reference_coverage() {
        let source = "component Demo { Frame { width:48; height:40; padding:0; radius:4; background:#10203080; Button { width:48; height:40; } } }";
        let component = "component Button { Rectangle { background:#12345670; ContentClip { x:1.3; y:2.8; width:16.5; height:22.7; radius:3.2; } ContentShape { points:'-3 -2 45 -2 45 38 -3 38'; color:#ff000080; } ContentClip { x:5.4; y:8.2; width:18; height:11.8; radius:2; } ContentShape { points:'0 0 48 0 48 40 0 40'; color:#00ff0080; } ContentClipEnd {} ContentClipEnd {} ContentClip { x:26.2; y:1.4; width:12.6; height:16.3; radius:4; } ContentShape { points:'0 0 48 0 48 40 0 40'; color:#0000ff80; } ContentClipEnd {} ContentClip { x:0; y:0; width:0; height:30; } ContentShape { points:'0 0 48 0 48 40 0 40'; color:#ffffff; } ContentClipEnd {} } }";
        let model = Button::from_sources(source, component).unwrap();
        for scale in [1., 1.25, 2.] {
            for background in [false, true] {
                let reference = model.raster_reference(96, 80, scale, background);
                let actual = model.raster_cached(96, 80, scale, background);
                for (a, b) in reference.chunks_exact(4).zip(actual.chunks_exact(4)) {
                    for k in 0..4 {
                        let premul = |p: &[u8]| if k == 3 { p[3] as f32 } else { p[k] as f32 * p[3] as f32 / 255. };
                        assert!((premul(a) - premul(b)).abs() <= 2., "{a:?} != {b:?}");
                    }
                }
            }
        }
    }
}
