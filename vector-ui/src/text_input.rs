//! Text editing state and visual composition shared by native and WASM hosts.
use crate::{control_state::TextEdit, template::{Content, Text}};
#[derive(Debug, Clone, PartialEq)]
pub struct Input {
    pub bounds: [f32;4], pub value: String, pub placeholder: String,
    pub color: [u8;4], pub placeholder_color: [u8;4], pub font_size:f32, pub multiline:bool,
}
impl Default for Input { fn default()->Self {Self {bounds:[0.,0.,100.,30.],value:String::new(),placeholder:String::new(),color:[255;4],placeholder_color:[150,150,150,255],font_size:15.,multiline:false}} }
#[derive(Clone)]
pub(crate) struct Editor {
    pub spec:Input, pub model:TextEdit, pub base:Vec<Content>, pub preedit:String,
    offset:[f32;2],
    focused:bool, reduced:bool, phase:f32, fades:Vec<(std::ops::Range<usize>,f32)>,
    dirty:bool,
}
impl Editor {
    pub fn new(spec:Input,base:Vec<Content>)->Result<Self,String> {
        let model=TextEdit::with_multiline(spec.value.clone(),spec.multiline).map_err(|e|e.to_string())?;
        Ok(Self{spec,model,base,preedit:String::new(),offset:[0.;2],focused:false,reduced:false,phase:0.,fades:Vec::new(),dirty:true})
    }
    pub fn needs_content(&self, focused:bool)->bool {self.dirty || self.focused!=focused}
    pub fn invalidate_content(&mut self) {self.dirty=true;}
    pub fn set_preedit(&mut self, text:&str) {
        if self.preedit!=text {self.preedit.clear();self.preedit.push_str(text);self.dirty=true;}
    }
    pub fn is_animating(&self)->bool { !self.reduced && (self.focused || !self.fades.is_empty()) }
    pub fn reduced_motion(&mut self,reduced:bool) {self.dirty|=self.reduced!=reduced;self.reduced=reduced;if reduced {self.fades.clear();self.phase=0.;}}
    pub fn tick(&mut self,dt:f32) {
        if !dt.is_finite() || dt<=0. || self.reduced {return;}
        let before=(self.spec.color[3] as f32*self.caret_alpha()).round() as u8;
        if self.focused {self.phase=(self.phase+dt)%1100.;}
        self.dirty|=!self.fades.is_empty() || (self.focused && before!=(self.spec.color[3] as f32*self.caret_alpha()).round() as u8);
        for (_,age) in &mut self.fades {*age+=dt;}
        self.fades.retain(|(_,age)|*age<220.);
    }
    fn caret_alpha(&self)->f32 {
        if self.reduced {1.} else {(1.+(self.phase/1100.*std::f32::consts::TAU).cos())*0.5}
    }
    fn width(&self,s:&str)->f32 {crate::text::measure_line(s,self.spec.font_size)[0]}
    fn line_height(&self)->f32 {self.spec.font_size*1.5}
    fn caret_point(&self)->[f32;2] {
        let prefix=&self.model.value()[..self.model.caret()];
        [self.width(prefix.rsplit('\n').next().unwrap_or("")),prefix.bytes().filter(|b|*b==b'\n').count() as f32*self.line_height()]
    }
    fn top(&self)->f32 {if self.spec.multiline {0.} else {(self.spec.bounds[3]-self.line_height())*0.5}}
    pub fn hit(&mut self,x:f32,y:f32,extend:bool) {
        self.phase=0.;self.dirty=true;
        let x=x-self.spec.bounds[0]+self.offset[0];
        let row=((y-self.spec.bounds[1]-self.top()+self.offset[1])/self.line_height()).floor().max(0.) as usize;
        let mut lines=self.model.value().split('\n');
        let mut line=lines.next().unwrap_or("");let mut start=0;
        for next in lines.take(row) {start+=line.len()+1;line=next;}
        let pos=crate::text::hit_character(line,self.spec.font_size,x);
        let anchor=if extend {self.model.anchor()} else {start+pos};
        let _=self.model.set_selection(anchor,start+pos);
    }
    pub fn key(&mut self,key:&str,shift:bool,command:bool)->bool {
        self.phase=0.;self.dirty=true;
        if command {
            if matches!(key.to_lowercase().as_str(),"z"|"y") {self.fades.clear();}
            match key.to_lowercase().as_str() {
                "a"=>{self.model.select_all();}, "z"=>{if shift {self.model.redo();}else{self.model.undo();}},
                "y"=>{self.model.redo();}, _=>return false,
            }
            return true;
        }
        match key {
            "Backspace"=>{self.fades.clear();self.model.backspace();},"Delete"=>{self.fades.clear();self.model.delete();},
            "ArrowLeft"=>{self.model.left(shift);},"ArrowRight"=>{self.model.right(shift);},
            "Home"|"End"=>{let pos=self.model.caret();let value=self.model.value();let target=if key=="Home" {value[..pos].rfind('\n').map_or(0,|i|i+1)} else {value[pos..].find('\n').map_or(value.len(),|i|pos+i)};let anchor=if shift {self.model.anchor()}else{target};let _=self.model.set_selection(anchor,target);},
            "ArrowUp"|"ArrowDown"=>{let p=self.caret_point();let y=p[1]+if key=="ArrowUp"{-self.line_height()}else{self.line_height()};self.hit(self.spec.bounds[0]+p[0]-self.offset[0],self.spec.bounds[1]+self.top()+y-self.offset[1]+self.line_height()*0.5,shift);},
            "Enter"=>{if self.spec.multiline {self.insert("\n");}},
            _=>return false,
        }
        true
    }
    pub fn insert(&mut self,text:&str) {
        self.dirty=true;
        self.preedit.clear();
        let clean=text.replace("\r\n","\n").replace('\r',"\n");
        let clean=if self.spec.multiline {clean} else {clean.replace('\n'," ")};
        if self.model.value().len()-self.model.selection().len()+clean.len()<=100_000 {
            let selected=self.model.selection();
            // Existing characters keep their age when an insertion shifts their byte offsets.
            self.fades.retain(|(range,_)|range.end<=selected.start || range.start>=selected.end);
            for (range,_) in &mut self.fades {
                if range.start>=selected.end {
                    range.start=range.start-selected.len()+clean.len();
                    range.end=range.end-selected.len()+clean.len();
                }
            }
            let _=self.model.insert(&clean);
            if !self.reduced && !clean.is_empty() {self.fades.push((selected.start..selected.start+clean.len(),0.));}
            self.phase=0.;
        }
    }
    pub fn content(&mut self,focused:bool)->Vec<Content> {
        if focused!=self.focused {self.phase=0.;self.focused=focused;}
        let mut out=self.base.clone();let [x,y,w,h]=self.spec.bounds;let lh=self.line_height();
        let p=self.caret_point();
        if focused {
            self.offset[0]=self.offset[0].min(p[0]).max((p[0]+self.width(&self.preedit)+2.-w).max(0.));
            self.offset[1]=self.offset[1].min(p[1]).max((p[1]+lh-h).max(0.));
        }
        out.push(Content::Clip{bounds:[x,y,w,h],radius:0.});
        let value=self.model.value();let selection=self.model.selection();
        let mut start=0;
        for (row,line) in value.split('\n').enumerate() {
            let py=y+self.top()+row as f32*lh-self.offset[1];
            let px=x-self.offset[0];
            if focused && !selection.is_empty() {
                let a=selection.start.max(start);let b=selection.end.min(start+line.len());
                if a<=b && selection.end>start && selection.start<=start+line.len() {
                    let left=self.width(&line[..a.saturating_sub(start).min(line.len())]);
                    let right=self.width(&line[..b.saturating_sub(start).min(line.len())]);
                    out.push(rect(px+left,py,(right-left).max(if selection.end>start+line.len(){5.}else{0.}),lh,[222,106,25,90]));
                }
            }
            let mut cuts=vec![0,line.len()];
            for (range,_) in &self.fades {
                if range.start<start+line.len() && range.end>start {
                    cuts.push(range.start.saturating_sub(start));
                    cuts.push((range.end-start).min(line.len()));
                }
            }
            cuts.sort_unstable();cuts.dedup();
            for run in cuts.windows(2) {
                let text=&line[run[0]..run[1]];
                let mut color=self.spec.color;
                if let Some((_,age))=self.fades.iter().find(|(range,_)|range.contains(&(start+run[0]))) {
                    let t=(*age/220.).clamp(0.,1.);
                    color[3]=(color[3] as f32*t*t*(3.-2.*t)).round() as u8;
                }
                out.push(Content::Text{bounds:[px+self.width(&line[..run[0]]),py,self.width(text),lh],text:Text{text:text.into(),font_size:self.spec.font_size,color}});
            }
            start+=line.len()+1;
        }
        if value.is_empty()&&self.preedit.is_empty() {out.push(Content::Text{bounds:[x,y+self.top(),self.width(&self.spec.placeholder),lh],text:Text{text:self.spec.placeholder.clone(),font_size:self.spec.font_size,color:self.spec.placeholder_color}});}
        if focused {
            let cx=x+p[0]-self.offset[0];let cy=y+self.top()+p[1]-self.offset[1];let pre=self.width(&self.preedit);
            if !self.preedit.is_empty() {
                out.push(Content::Text{bounds:[cx,cy,pre,lh],text:Text{text:self.preedit.clone(),font_size:self.spec.font_size,color:self.spec.color}});
                out.push(rect(cx,cy+lh-1.,pre,1.,self.spec.color));
            }
            let mut color=self.spec.color;
            color[3]=(color[3] as f32*self.caret_alpha()).round() as u8;
            out.push(rect(cx+pre,cy,1.,lh,color));
        }
        out.push(Content::ClipEnd);self.dirty=false;out
    }
    pub fn caret_rect(&self)->[f32;4] {let p=self.caret_point();[self.spec.bounds[0]+p[0]-self.offset[0],self.spec.bounds[1]+self.top()+p[1]-self.offset[1],1.,self.line_height()]}
}
fn rect(x:f32,y:f32,w:f32,h:f32,color:[u8;4])->Content {Content::Shape{points:vec![[x,y],[x+w,y],[x+w,y+h],[x,y+h]],color}}

#[cfg(test)]
mod animation_tests {
    use super::*;
    #[test]
    fn idle_content_is_clean_and_quantized_caret_changes_are_never_skipped() {
        let mut e=Editor::new(Input::default(),vec![]).unwrap();
        assert!(e.needs_content(false));e.content(false);
        e.tick(16.);assert!(!e.needs_content(false));
        assert!(e.needs_content(true));
        let mut cached=e.content(true);
        for _ in 0..1100 {
            e.tick(1.);
            if e.needs_content(true) {cached=e.content(true);}
            assert_eq!(cached,e.clone().content(true));
        }
        e.set_preedit("я");assert!(e.needs_content(true));e.content(true);
        e.set_preedit("я");assert!(!e.needs_content(true));
        e.set_preedit("");assert!(e.needs_content(true));e.content(true);
        e.reduced_motion(true);assert!(e.needs_content(true));e.content(true);
        e.tick(300.);assert!(!e.needs_content(true));
        assert!(e.needs_content(false));
    }
    #[test]
    fn multiline_hit_clamps_rows_and_retains_unicode_byte_boundaries() {
        let spec=Input{value:"A\nБ🙂\n".into(),multiline:true,..Input::default()};
        let mut e=Editor::new(spec,vec![]).unwrap();
        e.hit(-10.,30.,false);assert_eq!(e.model.caret(),2);
        e.hit(1000.,30.,false);assert_eq!(e.model.caret(),8);
        e.hit(1000.,10000.,false);assert_eq!(e.model.caret(),9);
        e.hit(-10.,-10.,true);assert_eq!(e.model.caret(),0);assert_eq!(e.model.anchor(),9);
    }
    #[test]
    fn caret_is_smooth_resets_and_stops_without_focus() {
        let mut e=Editor::new(Input::default(),vec![]).unwrap();
        e.content(true); assert!(e.is_animating()); assert_eq!(e.caret_alpha(),1.);
        e.tick(275.); assert!((e.caret_alpha()-0.5).abs()<0.001);
        e.tick(275.); assert!(e.caret_alpha()<0.001);
        e.insert("я"); assert_eq!(e.caret_alpha(),1.);
        e.tick(220.); e.content(false); assert!(!e.is_animating());
        e.content(true);e.reduced_motion(true);assert!(!e.is_animating());assert_eq!(e.caret_alpha(),1.);
    }
    #[test]
    fn new_unicode_runs_fade_independently_and_replacement_keeps_valid_offsets() {
        let mut e=Editor::new(Input::default(),vec![]).unwrap();
        e.insert("я");e.tick(110.);e.insert("🙂");
        let content=e.content(true);
        let runs:Vec<_>=content.iter().filter_map(|c|if let Content::Text{text,..}=c {Some((text.text.as_str(),text.color[3]))}else{None}).collect();
        assert_eq!(runs,vec![("я",128),("🙂",0)]);
        assert_eq!(e.model.value(),"я🙂");
        e.model.set_selection(0,2).unwrap();e.insert("abc");
        e.content(true);assert_eq!(e.model.value(),"abc🙂");
        e.tick(220.);assert!(e.fades.is_empty());
        e.reduced_motion(true);e.insert("Z");assert!(e.fades.is_empty());
    }
}
