// Same vector rasterizer in native wgpu and browser WebGPU. No input textures.
struct Command { info:vec4f, bounds:vec4f, color:vec4f, extra:vec4f, reserved:vec4f }
struct Params { viewport:vec4f, fill:vec4f, border:vec4f }
@group(0) @binding(0) var<uniform> params:Params;
@group(0) @binding(1) var<storage,read> commands:array<Command>;
@group(0) @binding(2) var<storage,read> edges:array<vec4f>;
@group(0) @binding(3) var<storage,read> tiles:array<u32>;

@group(0) @binding(4) var<storage,read> paints:array<vec4f>;

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
fn clip_coverage(pixel:vec2f,c:Command)->f32 {
    if any(c.bounds.zw<=vec2f(0.)){return 0.;}
    let scale=params.viewport.z;
    if c.extra.y==1.{
        let p=(pixel+0.5)/scale;
        return select(0.,1.,all(p>=c.bounds.xy)&&all(p<c.bounds.xy+c.bounds.zw));
    }
    return edge_coverage(distance((pixel+0.5)/scale,c.bounds,c.extra.x));
}
fn path_inside(p:vec2f,c:Command)->bool {
    var winding=0i;
    for(var i=u32(c.info.z);i<u32(c.info.z+c.info.w);i++){
        let e=edges[i];
        if (e.y<=p.y&&e.w>p.y)||(e.w<=p.y&&e.y>p.y){
            let x=e.x+(p.y-e.y)*(e.z-e.x)/(e.w-e.y);
            if x>p.x{winding+=select(-1,1,e.w>e.y);}
        }
    }
    if c.info.y==1.{return winding!=0;}return (abs(winding)%2)==1;
}
fn coverage(pixel:vec2f,c:Command)->vec4f {
    if c.info.x==5.{
        let p=(pixel+0.5)/params.viewport.z;
        return premul(c.color)*select(0.,1.,all(p>=c.bounds.xy)&&all(p<c.bounds.xy+c.bounds.zw));
    }
    if c.info.x!=1.{
        let p=(pixel+0.5)/params.viewport.z;
        let d=distance(p,c.bounds,c.extra.x);
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
    var out=vec4f(0.);
    for(var y=0u;y<2u;y++){for(var x=0u;x<2u;x++){
        let p=(pixel+(vec2f(f32(x),f32(y))+0.5)/2.)/params.viewport.z;
        if path_inside(p,c){out+=premul(c.color)*0.25;}
    }}return out;
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
            parents[depth]=result;masks[depth]=clip_coverage(pixel,c);depth+=1u;
            result=vec4f(0.);visible*=masks[depth-1u];
        }else if c.info.x==3.{
            depth-=1u;result=over(result*masks[depth],parents[depth]);
            visible=1.;for(var j=0u;j<depth;j++){visible*=masks[j];}
        }else if visible>0.{
            let p=(pixel+0.5)/params.viewport.z;let aa=1./params.viewport.z;
            if all(p>=c.bounds.xy-vec2f(aa))&&all(p<=c.bounds.xy+c.bounds.zw+vec2f(aa)){
                result=over(coverage(pixel,c),result);
            }
        }
    }
    if params.viewport.w>=1.{result=over(result,vec4f(17./255.,19./255.,25./255.,1.));}
    // sRGB swapchain encodes linear output. CPU reference composes encoded colors.
    if params.viewport.w>=2.{result=vec4f(linear(result.rgb),result.a);}
    return result;
}
