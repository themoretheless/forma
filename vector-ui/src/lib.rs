use wasm_bindgen::prelude::*;
pub mod markup;
pub mod template;
mod text;
mod shape;
mod raster_cache;
pub mod display_list;
#[cfg(feature="gpu")]
pub mod gpu;

pub const EXAMPLE: &str = include_str!("../examples/Button.ui");
pub const BUTTON_COMPONENT: &str = include_str!("../examples/Button.component.ui");

fn inside(x: f32, y: f32, b: &markup::ButtonSpec) -> bool {
    let radius = b.radius.min(b.width / 2.).min(b.height / 2.);
    let dx = (x - (b.x + b.width / 2.)).abs() - (b.width / 2. - radius);
    let dy = (y - (b.y + b.height / 2.)).abs() - (b.height / 2. - radius);
    dx.max(0.).hypot(dy.max(0.)) + dx.max(dy).min(0.) <= radius
}

fn round_inside(x:f32,y:f32,bounds:[f32;4],radius:f32)->bool {
    let [bx,by,w,h]=bounds;if w<=0.||h<=0.{return false;}let r=radius.min(w/2.).min(h/2.);
    let dx=(x-bx-w/2.).abs()-(w/2.-r);let dy=(y-by-h/2.).abs()-(h/2.-r);
    dx.max(0.).hypot(dy.max(0.))+dx.max(dy).min(0.)<=r
}
fn round_coverage(x:u32,y:u32,scale:f32,bounds:[f32;4],radius:f32)->f32 {
    let [bx,by,w,h]=bounds;if w<=0.||h<=0.{return 0.;}let r=radius.min(w/2.).min(h/2.);
    let dx=((x as f32+0.5)/scale-bx-w/2.).abs()-(w/2.-r);let dy=((y as f32+0.5)/scale-by-h/2.).abs()-(h/2.-r);
    let d=dx.max(0.).hypot(dy.max(0.))+dx.max(dy).min(0.)-r;
    if d < -0.71/scale{return 1.;}if d > 0.71/scale{return 0.;}
    let mut count=0.;for sy in [0.125,0.375,0.625,0.875]{for sx in [0.125,0.375,0.625,0.875]{if round_inside((x as f32+sx)/scale,(y as f32+sy)/scale,bounds,radius){count+=1.;}}}count/16.
}

#[wasm_bindgen]
pub struct Button {
    scene: markup::Scene,
    template: template::Template,
    fill: Animation,
    border: Animation,
    hover: bool,
    down: bool,
    focused: bool,
    clicks: u32,
    scroll_x:f32,
    scroll_y:f32,
    raster_cache:std::cell::RefCell<Option<raster_cache::Cache>>,
    visual_revision:u32,
    raster_builds:std::cell::Cell<u32>,
    raster_paints:std::cell::Cell<u32>,
    display_cache:std::cell::RefCell<Option<display_list::CachedList>>,
}

struct Animation { from:[f32;4], current:[f32;4], target:[f32;4], elapsed:f32, duration:f32 }
impl Animation {
    fn new(color:[u8;4])->Self {let c=color.map(|x|x as f32);Self{from:c,current:c,target:c,elapsed:0.,duration:0.}}
    fn target(&mut self,color:[u8;4],duration:f32){let c=color.map(|x|x as f32);if c!=self.target {self.from=self.current;self.target=c;self.duration=duration;self.elapsed=0.;if duration==0.{self.current=c;}}}
    fn active(&self)->bool{self.current!=self.target&&self.elapsed<self.duration}
    fn tick(&mut self,dt:f32)->bool{if !self.active(){return false;}self.elapsed=(self.elapsed+dt).min(self.duration);let t=self.elapsed/self.duration;let eased=t*t*(3.-2.*t);for k in 0..4{self.current[k]=self.from[k]+(self.target[k]-self.from[k])*eased;}if t==1.{self.current=self.target;}self.active()}
    fn color(&self)->[u8;4]{self.current.map(|x|x.clamp(0.,255.)as u8)}
}

impl Button {
    pub fn from_source(source: &str) -> Result<Self, String> {
        Self::from_sources(source,BUTTON_COMPONENT)
    }
    pub fn from_sources(source:&str, component:&str)->Result<Self,String>{
        let mut scene=markup::parse(source)?;
        let template=template::parse(component,&scene.button)?;
        scene.button=template.props.clone();
        scene.content_width=(scene.button.x+scene.button.width+scene.padding[1]).min(f32::MAX).max(scene.width);
        scene.content_height=(scene.button.y+scene.button.height+scene.padding[2]).min(f32::MAX).max(scene.height);
        scene.button.radius=template.radius;
        let fill=Animation::new(template.fill.as_ref().map_or([0;4],|b|if scene.button.disabled{b.disabled.unwrap_or(b.color)}else{b.color}));
        let border=Animation::new(template.border.as_ref().map_or([0;4],|b|b.brush.color));
        Ok(Self {scene,template,fill,border,hover:false,down:false,focused:false,clicks:0,scroll_x:0.,scroll_y:0.,raster_cache:std::cell::RefCell::new(None),visual_revision:0,raster_builds:std::cell::Cell::new(0),raster_paints:std::cell::Cell::new(0),display_cache:std::cell::RefCell::new(None)})
    }
    pub fn load_source(&mut self, source: &str) -> Result<(), String> {
        // Commit only a valid scene; editor keeps the last good preview on error.
        let mut next=Self::from_source(source)?;
        next.clicks=self.clicks;
        next.visual_revision=self.visual_revision.wrapping_add(1);
        *self=next;
        Ok(())
    }
    fn update_colors(&mut self){
        let before=(self.fill.color(),self.border.color());
        let target=|brush:&template::Brush|{
            if self.scene.button.disabled{brush.disabled.unwrap_or(brush.color)}
            else if self.down&&self.hover{brush.pressed.unwrap_or(brush.color)}
            else if self.hover{brush.hover.or(if self.focused{brush.focus}else{None}).unwrap_or(brush.color)}
            else if self.focused{brush.focus.unwrap_or(brush.color)}else{brush.color}
        };
        if let Some(b)=&self.template.fill {self.fill.target(target(b),b.duration_ms);}
        if let Some(b)=&self.template.border {self.border.target(target(&b.brush),b.brush.duration_ms);}
        if before!=(self.fill.color(),self.border.color()){self.visual_revision=self.visual_revision.wrapping_add(1);}
    }
}

#[wasm_bindgen]
impl Button {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self { Self::from_source(EXAMPLE).expect("built-in markup must be valid") }
    pub fn load(&mut self, source: &str) -> Result<(), JsValue> {
        self.load_source(source).map_err(|e| JsValue::from_str(&e))
    }
    pub fn load_component(&mut self,source:&str,component:&str)->Result<(),JsValue>{
        let mut next=Self::from_sources(source,component).map_err(|e|JsValue::from_str(&e))?;
        next.clicks=self.clicks;next.visual_revision=self.visual_revision.wrapping_add(1);*self=next;Ok(())
    }
    pub fn width(&self) -> f32 { self.scene.width }
    pub fn height(&self) -> f32 { self.scene.height }
    pub fn overflow(&self)->String{if self.scene.clip{"hidden".into()}else{"visible".into()}}
    pub fn clipped(&self)->bool{self.scene.clip}
    pub fn frame_radius(&self)->f32{self.scene.radius}
    pub fn render_width(&self)->f32{if !self.scene.clip&&!self.scene.scroll{self.scene.content_width}else{self.scene.width}}
    pub fn render_height(&self)->f32{if !self.scene.clip&&!self.scene.scroll{self.scene.content_height}else{self.scene.height}}
    pub fn scrollable(&self)->bool{self.scene.scroll}
    pub fn viewport(&self)->Vec<f32>{let p=self.scene.padding;if self.scene.scroll{vec![p[3],p[0],(self.width()-p[1]-p[3]).max(0.),(self.height()-p[0]-p[2]).max(0.)]}else{vec![0.,0.,self.width(),self.height()]}}
    pub fn scroll_offset(&self)->Vec<f32>{vec![self.scroll_x,self.scroll_y]}
    pub fn scroll(&mut self,dx:f32,dy:f32){if self.scene.scroll&&dx.is_finite()&&dy.is_finite(){let before=(self.scroll_x,self.scroll_y);let v=self.viewport();self.scroll_x=(self.scroll_x+dx).clamp(0.,(self.scene.button.width-v[2]).max(0.));self.scroll_y=(self.scroll_y+dy).clamp(0.,(self.scene.button.height-v[3]).max(0.));if before!=(self.scroll_x,self.scroll_y){self.visual_revision=self.visual_revision.wrapping_add(1);}self.down=false;self.hover=false;self.update_colors();}}
    pub fn label(&self) -> String { let mut labels:Vec<String>=self.template.text.iter().map(|t|t.text.clone()).collect();for c in &self.template.content{if let template::Content::Text{text,..}=c{labels.push(text.text.clone());}}if labels.is_empty(){self.scene.button.text.clone()}else{labels.join(" ")} }
    pub fn is_focused(&self)->bool{self.focused}
    pub fn preserve_interaction(&mut self, previous:&Button){
        if self.scene.button.key!=previous.scene.button.key{return;}
        self.clicks=previous.clicks;
        self.focused=previous.focused&&!self.disabled();
        self.hover=previous.hover;
        // A replaced subtree must not inherit an unfinished click gesture.
        self.down=false;self.update_colors();
    }
    pub fn key(&self) -> String { self.scene.button.key.clone() }
    pub fn action(&self) -> String { if self.template.clickable{self.scene.button.action.clone().unwrap_or_default()}else{String::new()} }
    pub fn disabled(&self) -> bool { self.scene.button.disabled }
    pub fn bounds(&self) -> Vec<f32> { let b=&self.scene.button; vec![b.x-self.scroll_x,b.y-self.scroll_y,b.width,b.height] }
    pub fn hit(&self, x: f32, y: f32) -> bool {let v=self.viewport();(!self.scene.clip||round_inside(x,y,[0.,0.,self.width(),self.height()],self.scene.radius))&&(!self.scene.scroll||(x>=v[0]&&y>=v[1]&&x<v[0]+v[2]&&y<v[1]+v[3]))&&inside(x+self.scroll_x,y+self.scroll_y,&self.scene.button) }
    pub fn focus(&mut self, focused: bool) { self.focused=focused; if !focused { self.down=false; } self.update_colors(); }
    pub fn visual_revision(&self)->u32{self.visual_revision}
    pub fn raster_stats(&self)->Vec<u32>{vec![self.raster_builds.get(),self.raster_paints.get()]}
    pub fn is_animating(&self)->bool{self.fill.active()||self.border.active()}
    pub fn tick(&mut self,delta_ms:f32)->bool{let before=(self.fill.color(),self.border.color());let dt=if delta_ms.is_finite(){delta_ms.clamp(0.,1000.)}else{0.};let a=self.fill.tick(dt);let b=self.border.tick(dt);if before!=(self.fill.color(),self.border.color()){self.visual_revision=self.visual_revision.wrapping_add(1);}a||b}
    pub fn pointer(&mut self, x: f32, y: f32, kind: u8) {
        self.hover=self.hit(x,y);
        if self.disabled()||!self.template.clickable { self.down=false; return; }
        match kind {
            1 => self.down=self.hover,
            2 => { if self.down&&self.hover { self.activate(); } self.down=false; }
            3 => { self.down=false;self.hover=false; }
            _ => {}
        }
        self.update_colors();
    }
    pub fn activate(&mut self) { if !self.disabled()&&self.template.clickable { self.clicks=self.clicks.saturating_add(1); } }
    pub fn clicks(&self) -> u32 { self.clicks }
    pub fn pixels(&self,width:u32,height:u32,scale:f32)->Vec<u8> {
        self.raster_cached(width,height,scale,true)
    }
    /// Studio paints its rounded artboard background separately from children.
    /// This avoids clipping overflowing children just to round the background.
    pub fn content_pixels(&self,width:u32,height:u32,scale:f32)->Vec<u8> {
        self.raster_cached(width,height,scale,false)
    }
    pub fn background_color(&self)->String {
        let c=self.scene.background;format!("#{:02x}{:02x}{:02x}{:02x}",c[0],c[1],c[2],c[3])
    }
    #[cfg(test)]
    fn raster_reference(&self,width:u32,height:u32,scale:f32,with_background:bool)->Vec<u8> {
        if width==0||height==0||width>4096||height>4096||width as u64*height as u64>8_388_608||!scale.is_finite()||scale<=0. { return Vec::new(); }
        let mut translated=self.scene.button.clone();translated.x-=self.scroll_x;translated.y-=self.scroll_y;
        let b=&translated;
        let mut out=vec![0;width as usize*height as usize*4];
        // Compose children first; background rounding must not clip overflow.
        let base=[0u8;4];
        let color=self.fill.color();
        let border=self.border.color();
        let border_width=self.template.border.as_ref().map_or(0.,|b|b.width);
        let viewport=self.viewport();
        for y in 0..height {for x in 0..width {
            let mut rgb=[0.;3];let mut coverage_alpha=0.;
            for (sx,sy) in [(0.25,0.25),(0.75,0.25),(0.25,0.75),(0.75,0.75)] {
                let px=(x as f32+sx)/scale;let py=(y as f32+sy)/scale;
                let in_button=inside(px,py,b);
                let r=b.radius.min(b.width/2.).min(b.height/2.);
                let dx=(px-(b.x+b.width/2.)).abs()-(b.width/2.-r);
                let dy=(py-(b.y+b.height/2.)).abs()-(b.height/2.-r);
                let distance=dx.max(0.).hypot(dy.max(0.))+dx.max(dy).min(0.)-r;
                let on_border=in_button&&distance>=-border_width&&border_width>0.;
                let fill_alpha=if in_button{color[3]as f32/255.}else{0.};
                let stroke_alpha=if on_border{border[3]as f32/255.}else{0.};
                let alpha=(base[3]as f32/255.*(1.-fill_alpha)+fill_alpha)*(1.-stroke_alpha)+stroke_alpha;
                coverage_alpha+=alpha/4.;
                for k in 0..3 {
                    let bg=base[k] as f32*(base[3] as f32/255.);
                    let alpha=if in_button { color[3] as f32/255. } else { 0. };
                    let filled=bg*(1.-alpha)+color[k] as f32*alpha;
                    let stroke_alpha=if on_border{border[3]as f32/255.}else{0.};
                    rgb[k]+=(filled*(1.-stroke_alpha)+border[k]as f32*stroke_alpha)/4.;
                }
            }
            let i=((y*width+x)*4) as usize;
            if coverage_alpha>0.{for k in 0..3 {out[i+k]=(rgb[k]/coverage_alpha).clamp(0.,255.).round()as u8;}}
            out[i+3]=(coverage_alpha*255.).round()as u8;
        }}
        let r=b.radius.min(b.width/2.).min(b.height/2.);
        if let Some(t)=&self.template.text{text::draw_text(&mut out,width,height,scale,&t.text,t.font_size,[b.x+r*0.3,b.y+r*0.3,b.width-r*0.6,b.height-r*0.6],t.color);}
        let mut groups:Vec<(Vec<u8>,[f32;4],f32)>=Vec::new();
        for c in &self.template.content {match c {
            template::Content::Clip{bounds,radius}=>{groups.push((std::mem::replace(&mut out,vec![0;width as usize*height as usize*4]),[bounds[0]+b.x,bounds[1]+b.y,bounds[2],bounds[3]],*radius));},
            template::Content::ClipEnd=>{if let Some((mut parent,bounds,radius))=groups.pop(){
                for y in 0..height{for x in 0..width{let i=((y*width+x)*4)as usize;let fg=out[i+3]as f32/255.*round_coverage(x,y,scale,bounds,radius);let retained=parent[i+3]as f32/255.*(1.-fg);let alpha=fg+retained;if alpha>0.{for k in 0..3{parent[i+k]=((out[i+k]as f32*fg+parent[i+k]as f32*retained)/alpha).round()as u8;}}parent[i+3]=(alpha*255.).round()as u8;}}
                out=parent;
            }},
            template::Content::Text{bounds,text:t}=>text::draw_text(&mut out,width,height,scale,&t.text,t.font_size,[b.x+bounds[0],b.y+bounds[1],bounds[2],bounds[3]],t.color),
            template::Content::Shape{points,color}=>shape::draw(&mut out,width,height,scale,points,[b.x,b.y],*color),
        }}
        // Clip the complete composed child, including glyphs and border, to the Frame.
        for y in 0..height {for x in 0..width {
            let px=(x as f32+0.5)/scale;let py=(y as f32+0.5)/scale;let i=((y*width+x)*4)as usize;
            let [vx,vy,vw,vh]=[viewport[0],viewport[1],viewport[2],viewport[3]];
            if self.scene.scroll&&(px<vx||py<vy||px>=vx+vw||py>=vy+vh) {out[i..i+4].copy_from_slice(&base);}
            if self.scene.scroll&&vw>0.&&vh>0. {
                let ch=self.scene.button.height;let cw=self.scene.button.width;
                let vertical=ch>vh&&px>=(vx+vw-5.).max(vx)&&px<vx+vw&&py>=vy+self.scroll_y/ch*vh&&py<vy+(self.scroll_y+vh)/ch*vh;
                let horizontal=cw>vw&&py>=(vy+vh-5.).max(vy)&&py<vy+vh&&px>=vx+self.scroll_x/cw*vw&&px<vx+(self.scroll_x+vw)/cw*vw;
                if vertical||horizontal{out[i..i+4].copy_from_slice(&[110,130,170,255]);}
            }
            let coverage=round_coverage(x,y,scale,[0.,0.,self.width(),self.height()],self.scene.radius);
            if with_background {
                let bg=self.scene.background;let fg=out[i+3]as f32/255.;let retained=bg[3]as f32/255.*(if self.scene.clip{1.}else{coverage})*(1.-fg);let alpha=fg+retained;
                if alpha>0.{for k in 0..3{out[i+k]=((out[i+k]as f32*fg+bg[k]as f32*retained)/alpha).round()as u8;}}
                out[i+3]=(alpha*255.).round()as u8;
            }
            if self.scene.clip {out[i+3]=(out[i+3]as f32*coverage).round()as u8;if out[i+3]==0{out[i..i+4].fill(0);}}
        }}
        out
    }
}

#[cfg(test)]mod tests {
    use super::*;
    fn compare_reference(b:&Button,w:u32,h:u32,s:f32){
        for background in [false,true]{
            let reference=b.raster_reference(w,h,s,background);let cached=b.raster_cached(w,h,s,background);
            for (index,(a,b)) in reference.chunks_exact(4).zip(cached.chunks_exact(4)).enumerate(){
                // Compare premultiplied channels: an RGB of alpha=0 is undefined.
                for k in 0..4{let aa=if k==3{a[3]as f32}else{a[k]as f32*a[3]as f32/255.};let bb=if k==3{b[3]as f32}else{b[k]as f32*b[3]as f32/255.};assert!((aa-bb).abs()<=2.,"pixel {index} channel {k}: {a:?} vs {b:?}");}
            }
        }
    }
    #[test]fn cached_raster_preserves_coverage_text_and_clips(){
        let mut b=Button::new();for s in [1.,1.25,2.]{compare_reference(&b,(400.*s)as u32,(200.*s)as u32,s);}
        b.pointer(100.,90.,0);b.tick(55.);compare_reference(&b,400,200,1.);
        let source="component Demo { Frame { width:64; height:48; radius:12; clip:true; padding:2; background:#12345680; Scroll { Button { width:100; height:60; } } } }";
        let component="component Button { Rectangle { radius:12; background:#ff000080; Border { width:2; background:#aabbcc80; } ContentClip { x:5; y:2; width:45; height:40; radius:5; } ContentShape { points:'0 0 70 0 70 50 0 50'; color:#abcdef80; } ContentText { x:1; y:2; width:55; height:30; text:'Тест'; color:#ffffffa0; fontSize:16; } ContentClipEnd {} } }";
        let mut b=Button::from_sources(source,component).unwrap();compare_reference(&b,80,60,1.25);b.scroll(10.,4.);compare_reference(&b,80,60,1.25);
    }
    #[test]fn unchanged_hover_does_not_dirty_or_rebuild_static_layers(){
        let mut b=Button::new();let first=b.pixels(400,200,1.);assert_eq!(b.raster_stats(),vec![1,1]);
        b.pointer(100.,90.,0);assert!(b.is_animating());b.tick(1000.);let hovered=b.pixels(400,200,1.);assert_ne!(first,hovered);assert_eq!(b.raster_stats(),vec![1,2]);
        let revision=b.visual_revision();for i in 0..100{b.pointer(101.+i as f32,95.,0);assert!(!b.is_animating());assert_eq!(b.visual_revision(),revision);}
        assert_eq!(hovered,b.pixels(400,200,1.));assert_eq!(b.raster_stats(),vec![1,2]);
        b.pixels(800,400,2.);assert_eq!(b.raster_stats(),vec![2,3]);
        b.load_source(&EXAMPLE.replace("Найти документы","Новый текст")).unwrap();assert_eq!(b.raster_stats(),vec![0,0]);assert_ne!(hovered,b.pixels(400,200,1.));
    }
    const OVERFLOW:&str="component Demo { Frame { width:100; height:80; padding:10; Button { width:150; height:120; background:#ff0000; text:''; } } }";
    #[test]fn overflow_is_valid_layout(){let b=Button::from_source(OVERFLOW).unwrap();assert_eq!(b.render_width(),170.);assert_eq!(b.render_height(),140.);assert!(b.hit(130.,60.));}
    #[test]fn hidden_overflow_clips_pixels_and_hit_testing(){let b=Button::from_source(&OVERFLOW.replace("padding:10;","padding:10; overflow:hidden;")).unwrap();assert_eq!(b.render_width(),100.);assert!(!b.hit(130.,60.));let p=b.pixels(170,140,1.);assert_eq!(&p[(60*170+130)*4..(60*170+130)*4+4],&[0,0,0,0],"Outside Frame is transparent, not an extended square background");}
    #[test]fn scroll_is_an_element_and_clamps_offsets(){let source=OVERFLOW.replace("Button {","Scroll { Button {").replace("} } }","} } } }");let mut b=Button::from_source(&source).unwrap();assert!(b.scrollable());assert_eq!(b.viewport(),vec![10.,10.,80.,60.]);assert_eq!(b.render_width(),100.);let before=b.pixels(100,80,1.);b.scroll(500.,500.);assert_eq!(b.scroll_offset(),vec![70.,60.]);assert_ne!(before,b.pixels(100,80,1.));assert!(b.hit(40.,40.));assert!(!b.hit(95.,40.));b.scroll(-500.,-500.);assert_eq!(b.scroll_offset(),vec![0.,0.]);}
    #[test]fn scroll_is_not_an_overflow_value(){assert!(markup::parse(&OVERFLOW.replace("padding:10;","overflow:scroll;")).is_err());}
    #[test]fn clicks_require_press_and_release_inside(){let mut b=Button::new();b.pointer(100.,90.,2);assert_eq!(b.clicks(),0);b.pointer(100.,90.,1);b.pointer(0.,0.,2);assert_eq!(b.clicks(),0);b.pointer(100.,90.,1);b.pointer(100.,90.,2);assert_eq!(b.clicks(),1);}
    #[test]fn markup_controls_pixels_and_geometry(){let mut b=Button::new();let old=b.pixels(400,200,1.);b.load_source(&EXAMPLE.replace("#8ca5ff","#ff0000").replace("padding: 65 70","padding: 20 70")).unwrap();assert_ne!(old,b.pixels(400,200,1.));assert!(b.hit(100.,30.));assert_eq!(b.action(),"actions.search");}
    #[test]fn disabled_markup_blocks_events(){let mut b=Button::from_source(&EXAMPLE.replace("disabled: false","disabled: true")).unwrap();b.activate();b.pointer(100.,90.,1);b.pointer(100.,90.,2);assert_eq!(b.clicks(),0);}
    #[test]fn malformed_markup_keeps_last_scene(){let mut b=Button::new();assert!(b.load_source("component Bad {}").is_err());assert_eq!(b.label(),"Найти документы");}
    #[test]fn guard_raster_size(){assert!(Button::new().pixels(u32::MAX,100,1.).is_empty());}
    #[test]fn frame_clip_rounds_content_and_hit_area(){
        let source="component Demo { Frame { width:40; height:40; radius:12; clip:true; background:#000; Button { width:60; height:60; } } }";
        let template="component Button { Rectangle { background:#ffffff; PointerArea { clicked -> events.clicked(); } } }";
        let b=Button::from_sources(source,template).unwrap();assert!(b.clipped());assert_eq!(b.render_width(),40.);assert!(!b.hit(1.,1.));assert!(b.hit(20.,1.));assert!(!b.hit(45.,20.));
        let pixels=b.content_pixels(60,60,1.);assert_eq!(pixels[3],0);assert_eq!(pixels[(20*60+45)*4+3],0);assert_eq!(pixels[(20*60+20)*4+3],255);assert!(pixels.chunks_exact(4).any(|p|p[3]>0&&p[3]<255));
        let open=Button::from_sources(&source.replace("clip:true","clip:false"),template).unwrap();assert_eq!(open.render_width(),60.);assert!(open.hit(1.,1.));assert!(open.hit(45.,20.));assert_eq!(open.content_pixels(60,60,1.)[3],255);
        assert!(Button::from_sources(&source.replace("clip:true","clip:1"),template).is_err());
        assert!(Button::from_sources(&source.replace("clip:true","clip:true; overflow:visible"),template).is_err());
    }
    #[test]fn nested_frame_clip_does_not_clip_siblings(){
        let source="component Demo { Frame { width:40; height:40; Button { width:40; height:40; } } }";
        let template="component Button { Rectangle { ContentClip { x:0; y:0; width:20; height:20; radius:8; } ContentShape { points:'0 0 40 0 40 40 0 40'; color:#ff0000; } ContentClipEnd {} ContentShape { points:'30 30 40 30 40 40 30 40'; color:#00ff00; } } }";
        let b=Button::from_sources(source,template).unwrap();let p=b.content_pixels(40,40,1.);assert_eq!(p[3],0);assert_eq!(&p[(10*40+10)*4..(10*40+10)*4+4],&[255,0,0,255]);assert_eq!(p[(25*40+25)*4+3],0);assert_eq!(&p[(35*40+35)*4..(35*40+35)*4+4],&[0,255,0,255]);
        assert!(Button::from_sources(source,&template.replace("ContentClipEnd {}","")).is_err());
    }
    #[test]fn transparent_artboard_has_no_square_background_and_keeps_overflow(){
        let source="component Demo { Frame { width:40; height:40; padding:0; background:#123456; Button { width:60; height:60; radius:16; } } }";
        let component="component Button { Rectangle { radius:props.radius; background:#ffffff; } }";
        let button=Button::from_sources(source,component).unwrap();
        let transparent=button.content_pixels(60,60,1.);
        assert_eq!(&transparent[0..4],&[0,0,0,0]);
        assert_eq!(transparent[(30*60+50)*4+3],255,"visible overflow must survive");
        assert!(transparent.chunks_exact(4).any(|p|p[3]>0&&p[3]<255&&p[0]==255&&p[1]==255&&p[2]==255),"edge coverage must not introduce a dark halo");
        let opaque=button.pixels(60,60,1.);assert_eq!(&opaque[0..4],&[18,52,86,255]);
        let hidden=Button::from_sources(&source.replace("padding:0;","padding:0; overflow:hidden;"),component).unwrap().content_pixels(60,60,1.);
        assert_eq!(&hidden[(30*60+50)*4..(30*60+50)*4+4],&[0,0,0,0]);
    }
    #[test]fn composed_content_is_rasterized_and_keeps_button_events(){
        let source="component Demo { Frame { width:100; height:60; padding:0; background:#000; Button { width:100; height:60; clicked -> actions.search(); } } }";
        let component="component Button { Rectangle { ContentShape { points:'5 5 20 5 20 20 5 20'; color:#ff0000; } ContentText { x:25; y:0; width:70; height:30; text:'A'; color:#ffffff; fontSize:20; } ContentText { x:25; y:30; width:70; height:30; text:'Б'; color:#ffffff; fontSize:20; } PointerArea { clicked -> events.clicked(); } } }";
        let mut button=Button::from_sources(source,component).unwrap();let pixels=button.pixels(100,60,1.);
        assert_eq!(&pixels[(10*100+10)*4..(10*100+10)*4+3],&[255,0,0]);
        assert!(pixels.chunks_exact(4).any(|p|p[0]>100&&p[1]>100&&p[2]>100));
        assert_eq!(button.label(),"A Б");button.activate();assert_eq!(button.clicks(),1);assert_eq!(button.action(),"actions.search");
        assert!(Button::from_sources(source,&component.replace("5 5 20 5 20 20 5 20","NaN 1 2 3 4 5")).is_err());
    }
    #[test]fn brush_transition_is_time_driven(){let mut b=Button::new();let initial=b.fill.color();b.pointer(100.,90.,0);assert_eq!(b.fill.color(),initial);assert!(b.tick(70.));assert_ne!(b.fill.color(),initial);assert!(b.fill.color()!=[168,186,255,255]);b.tick(1000.);assert_eq!(b.fill.color(),[168,186,255,255]);}
    #[test]fn component_controls_primitives_and_event_forwarding(){let template=BUTTON_COMPONENT.replace("clicked -> events.clicked();","").replace("text: props.text;","text: 'Из компонента';");let mut b=Button::from_sources(EXAMPLE,&template).unwrap();assert_eq!(b.label(),"Из компонента");b.activate();assert_eq!(b.clicks(),0);assert!(b.action().is_empty());}
}
