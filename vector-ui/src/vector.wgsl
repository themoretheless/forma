// Same vector rasterizer in native wgpu and browser WebGPU. No input textures.
// scroll.x: 0 fixed, 1 content (bounds - params.scroll.xy),
// 2 vertical thumb (bounds.y + scroll.y * scroll.y factor), 3 horizontal thumb.
struct Command { info:vec4f, bounds:vec4f, color:vec4f, extra:vec4f, scroll:vec4f }
struct Params { viewport:vec4f, fill:vec4f, border:vec4f, scroll:vec4f }
@group(0) @binding(0) var<uniform> params:Params;
@group(0) @binding(1) var<storage,read> commands:array<Command>;
@group(0) @binding(2) var<storage,read> edges:array<vec4f>;
@group(0) @binding(3) var<storage,read> tiles:array<u32>;

@group(0) @binding(4) var<storage,read> paints:array<vec4f>;

// Geometry stays in content coordinates; only the command bounds move. Glyph
// edges are shifted the same way inside path_coverage. Returns a vec4 rather
// than copying the whole Command per command per pixel.
fn scrolled_bounds(c:Command)->vec4f {
    if c.scroll.x==1.{return vec4f(c.bounds.xy-params.scroll.xy,c.bounds.zw);}
    if c.scroll.x==2.{return vec4f(c.bounds.x,c.bounds.y+params.scroll.y*c.scroll.y,c.bounds.zw);}
    if c.scroll.x==3.{return vec4f(c.bounds.x+params.scroll.x*c.scroll.y,c.bounds.yzw);}
    return c.bounds;
}
@vertex fn vs(@builtin(vertex_index) index:u32)->@builtin(position) vec4f {
    let p=array<vec2f,3>(vec2f(-1.,-1.),vec2f(3.,-1.),vec2f(-1.,3.));
    return vec4f(p[index],0.,1.);
}
fn over(fg:vec4f,bg:vec4f)->vec4f {return fg+bg*(1.-fg.a);}
fn premul(c:vec4f)->vec4f {return vec4f(c.rgb*c.a,c.a);}
fn distance(p:vec2f,b:vec4f,radius:f32)->f32 {
    let r=min(radius,min(b.z,b.w)*0.5);
    let d=abs(p-b.xy-b.zw*0.5)-(b.zw*0.5-vec2f(r));
    return length(max(d,vec2f(0.)))+min(max(d.x,d.y),0.)-r;
}
// Signed distance is in logical units; coverage spans one physical pixel.
// Unlike binary sub-samples this has no 25% opacity steps at curved edges.
fn edge_coverage(d:f32)->f32 {
    return clamp(0.5-d*params.viewport.z,0.,1.);
}
fn clip_coverage(pixel:vec2f,c:Command,b:vec4f)->f32 {
    if any(b.zw<=vec2f(0.)){return 0.;}
    let scale=params.viewport.z;
    if c.extra.y==1.{
        let p=(pixel+0.5)/scale;
        return select(0.,1.,all(p>=b.xy)&&all(p<b.xy+b.zw));
    }
    return edge_coverage(distance((pixel+0.5)/scale,b,c.extra.x));
}
// Exact signed area that one edge (physical pixels) contributes to the pixel
// whose top-left corner is `origin`: the integral over the pixel row of
// clamp(x_edge(y) - origin.x, 0, 1), signed by the edge direction. Summing over
// all edges of a contour gives the winding-weighted area of the pixel, so a
// path needs one pass over its edges instead of four point samples.
fn edge_area(a:vec2f,b:vec2f,origin:vec2f)->f32 {
    if a.y==b.y {return 0.;}
    let ya=max(min(a.y,b.y),origin.y);
    let yb=min(max(a.y,b.y),origin.y+1.);
    if ya>=yb {return 0.;}
    let slope=(b.x-a.x)/(b.y-a.y);
    let fa=a.x+(ya-a.y)*slope-origin.x;
    let fb=a.x+(yb-a.y)*slope-origin.x;
    let d=fb-fa;
    var area=clamp(fa,0.,1.);
    if d!=0.{
        // f is linear in t over [0,1]; it saturates at 0 and 1 outside [lo,hi].
        let t0=clamp(-fa/d,0.,1.);
        let t1=clamp((1.-fa)/d,0.,1.);
        let lo=min(t0,t1);
        let hi=max(t0,t1);
        let flo=clamp(fa+d*lo,0.,1.);
        let fhi=clamp(fa+d*hi,0.,1.);
        area=select(lo,1.-hi,d>0.)+(flo+fhi)*0.5*(hi-lo);
    }
    return select(-area,area,b.y>a.y)*(yb-ya);
}
fn path_coverage(pixel:vec2f,c:Command,shift:vec2f)->f32 {
    let scale=params.viewport.z;
    var area=0.;
    for(var i=u32(c.info.z);i<u32(c.info.z+c.info.w);i++){
        let e=edges[i];
        area+=edge_area((e.xy-shift)*scale,(e.zw-shift)*scale,pixel);
    }
    // Nonzero and even-odd agree for the simple contours Forma emits; overlapping
    // same-direction contours saturate instead of double-counting.
    return min(abs(area),1.);
}
fn coverage(pixel:vec2f,c:Command,b:vec4f)->vec4f {
    if c.info.x==5.{
        let p=(pixel+0.5)/params.viewport.z;
        return premul(c.color)*select(0.,1.,all(p>=b.xy)&&all(p<b.xy+b.zw));
    }
    if c.info.x!=1.{
        let p=(pixel+0.5)/params.viewport.z;
        let d=distance(p,b,c.extra.x);
        let outer=edge_coverage(d);
        if c.info.x==6.{
            let data=paints[u32(c.extra.z)];
            let color=paints[u32(c.extra.z)+1u];
            let band=max(outer-edge_coverage(d+c.extra.y),0.);
            let radial=clamp(1.-length(p-data.xy)/max(data.z,0.0001),0.,1.);
            return premul(color)*band*select(radial,1.,data.w==1.);
        }
        if c.info.x==4.{
            var result=premul(paints[u32(c.extra.z)])*outer;
            if c.extra.y>0.&&outer>0.{
                let inner=edge_coverage(d+c.extra.y);
                let band=max(outer-inner,0.);
                // Composite the border over the fill within the same shape;
                // do not multiply outer-edge coverage twice.
                let border=premul(paints[u32(c.extra.w)]);
                result=border*band+result*(1.-border.a*band/outer);
            }
            return result;
        }
        return premul(c.color)*outer;
    }
    // Edges are stored unscrolled; scrolled content shifts them into screen space.
    let shift=select(vec2f(0.),params.scroll.xy,c.scroll.x==1.);
    return premul(c.color)*path_coverage(pixel,c,shift);
}
fn linear(c:vec3f)->vec3f {
    return select(pow((c+0.055)/1.055,vec3f(2.4)),c/12.92,c<=vec3f(0.04045));
}
@fragment fn fs(@builtin(position) position:vec4f)->@location(0) vec4f {
    let pixel=floor(position.xy);
    let cols=u32(ceil(params.viewport.x/32.));
    let tile=(u32(pixel.y)/32u*cols+u32(pixel.x)/32u)*2u;
    let start=tiles[tile];let count=tiles[tile+1u];
    var parents:array<vec4f,32>;var masks:array<f32,32>;
    var depth=0u;var result=vec4f(0.);var visible=1.;
    for(var i=0u;i<count;i++){
        let c=commands[tiles[start+i]];
        if c.info.x==2.{
            parents[depth]=result;masks[depth]=clip_coverage(pixel,c,scrolled_bounds(c));depth+=1u;
            result=vec4f(0.);visible*=masks[depth-1u];
        }else if c.info.x==3.{
            depth-=1u;result=over(result*masks[depth],parents[depth]);
            visible=1.;for(var j=0u;j<depth;j++){visible*=masks[j];}
        }else if visible>0.{
            let b=scrolled_bounds(c);
            let p=(pixel+0.5)/params.viewport.z;let aa=1./params.viewport.z;
            if all(p>=b.xy-vec2f(aa))&&all(p<=b.xy+b.zw+vec2f(aa)){
                result=over(coverage(pixel,c,b),result);
            }
        }
    }
    if params.viewport.w>=1.{result=over(result,vec4f(17./255.,19./255.,25./255.,1.));}
    // sRGB swapchain encodes linear output. CPU reference composes encoded colors.
    if params.viewport.w>=2.{result=vec4f(linear(result.rgb),result.a);}
    return result;
}
