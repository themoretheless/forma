//! Text editing state and visual composition shared by native and WASM hosts.
use crate::{control_state::TextEdit, template::{Content, Text}};
pub(crate) const MAX_VALUE_BYTES:usize=100_000;
fn next_semantic_revision()->u32 {
    static NEXT:std::sync::atomic::AtomicU32=std::sync::atomic::AtomicU32::new(0);
    allocate_semantic_revision(&NEXT)
}
fn allocate_semantic_revision(counter:&std::sync::atomic::AtomicU32)->u32 {
    use std::sync::atomic::Ordering::Relaxed;
    // Zero disables transport caching permanently after the finite key space is exhausted.
    counter.fetch_update(Relaxed,Relaxed,|value|Some(value.saturating_add(1)))
        .expect("update always returns Some").checked_add(1).unwrap_or(0)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Input {
    pub bounds: [f32;4], pub value: String, pub placeholder: String, pub label: String, pub read_only:bool,
    pub color: [u8;4], pub placeholder_color: [u8;4], pub font_size:f32, pub multiline:bool,
}
impl Default for Input { fn default()->Self {Self {bounds:[0.,0.,100.,30.],value:String::new(),placeholder:String::new(),label:String::new(),read_only:false,color:[255;4],placeholder_color:[150,150,150,255],font_size:15.,multiline:false}} }
#[derive(Clone)]
pub(crate) struct Editor {
    pub spec:Input, pub model:TextEdit, pub base:Vec<Content>, pub preedit:String,
    offset:[f32;2],
    preferred_x:Option<f32>,
    focused:bool, reduced:bool, phase:f32, fades:Vec<(std::ops::Range<usize>,f32)>,
    dirty:bool,
    pub semantic_revision:u32,
}
impl Editor {
    pub fn new(spec:Input,base:Vec<Content>)->Result<Self,String> {
        let model=TextEdit::with_multiline(spec.value.clone(),spec.multiline).map_err(|e|e.to_string())?;
        Ok(Self{spec,model,base,preedit:String::new(),offset:[0.;2],preferred_x:None,focused:false,reduced:false,phase:0.,fades:Vec::new(),dirty:true,semantic_revision:next_semantic_revision()})
    }
    pub fn needs_content(&self, focused:bool)->bool {self.dirty || self.focused!=focused}
    pub fn invalidate_content(&mut self) {self.dirty=true;}
    pub fn set_preedit(&mut self, text:&str) {
        if self.spec.read_only&&!text.is_empty(){return;}
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
        self.semantic_revision=next_semantic_revision();
        self.preferred_x=None;
        self.phase=0.;self.dirty=true;
        let x=x-self.spec.bounds[0]+self.offset[0];
        let row=((y-self.spec.bounds[1]-self.top()+self.offset[1])/self.line_height()).floor().max(0.) as usize;
        let mut lines=self.model.value().split('\n');
        let mut line=lines.next().unwrap_or("");let mut start=0;
        for next in lines.take(row) {start+=line.len()+1;line=next;}
        // split('\n') leaves CR attached to its row, but CRLF is one cluster.
        // Exclude the CR from hit targets only when a following LF exists.
        if start+line.len()<self.model.value().len() {line=line.strip_suffix('\r').unwrap_or(line);}
        let pos=crate::text::hit_grapheme(line,self.spec.font_size,x);
        let anchor=if extend {self.model.anchor()} else {start+pos};
        let _=self.model.set_selection(anchor,start+pos);
    }
    pub fn select_utf16(&mut self,anchor:u32,caret:u32)->bool {
        self.select_utf16_in_value(anchor,caret,true)
    }
    pub fn select_original_utf16(&mut self,anchor:u32,caret:u32)->bool {
        self.select_utf16_in_value(anchor,caret,false)
    }
    fn select_utf16_in_value(&mut self,anchor:u32,caret:u32,normalized:bool)->bool {
        let byte=|offset:u32,ceil:bool| {
            let mut units=0;let mut chars=self.model.value().char_indices().peekable();
            while let Some((at,ch))=chars.next(){
                if units>=offset{return at;}
                units+=ch.len_utf16() as u32;
                let mut end=at+ch.len_utf8();
                if normalized && ch=='\r' && chars.peek().is_some_and(|(_,next)|*next=='\n'){end=chars.next().unwrap().0+1;}
                if units>=offset{return if units>offset&&!ceil{at}else{end};}
            }
            self.model.value().len()
        };
        let (a,c)=(byte(anchor,anchor>=caret),byte(caret,caret>=anchor));
        let old=(self.model.anchor(),self.model.caret());
        self.model.set_selection(a,c).expect("mapped scalar boundaries");
        let range=self.model.grapheme_selection();
        let (a,c)=if anchor>caret{(range.end,range.start)}else{(range.start,range.end)};
        self.model.set_selection(a,c).expect("grapheme boundaries");
        let changed=old!=(a,c);
        if changed{self.semantic_revision=next_semantic_revision();self.phase=0.;self.preferred_x=None;self.dirty=true;}
        changed
    }
    pub fn word_key(&mut self,key:&str,shift:bool)->bool {
        if !matches!(key,"ArrowLeft"|"ArrowRight"|"Backspace"|"Delete") {return false;}
        if self.spec.read_only&&matches!(key,"Backspace"|"Delete"){return true;}
        self.preferred_x=None;
        self.phase=0.;self.dirty=true;
        match key {
            "ArrowLeft"=>{self.model.move_words(-1,shift);},
            "ArrowRight"=>{self.model.move_words(1,shift);},
            "Backspace"=>{self.fades.clear();self.model.backspace_word();},
            "Delete"=>{self.fades.clear();self.model.delete_word();},
            _=>unreachable!(),
        }
        self.semantic_revision=next_semantic_revision();
        self.preedit.clear();
        true
    }
    pub fn key(&mut self,key:&str,shift:bool,command:bool)->bool {
        if self.spec.read_only && (matches!(key,"Backspace"|"Delete"|"Enter") || command&&matches!(key.to_ascii_lowercase().as_str(),"z"|"y")){return true;}
        self.phase=0.;self.dirty=true;
        if command {
            if matches!(key.to_lowercase().as_str(),"z"|"y") {self.fades.clear();}
            match key.to_lowercase().as_str() {
                "backspace"=>{self.fades.clear();self.model.delete_to_line_edge(false);},
                "delete"=>{self.fades.clear();self.model.delete_to_line_edge(true);},
                "a"=>{self.model.select_all();}, "z"=>{if shift {self.model.redo();}else{self.model.undo();}},
                "y"=>{self.model.redo();}, _=>return false,
            }
            self.preferred_x=None;
            self.semantic_revision=next_semantic_revision();
            self.preedit.clear();
            return true;
        }
        match key {
            "Backspace"=>{self.fades.clear();self.model.backspace_grapheme();},"Delete"=>{self.fades.clear();self.model.delete_grapheme();},
            "ArrowLeft"=>{self.model.move_graphemes(-1,shift);},"ArrowRight"=>{self.model.move_graphemes(1,shift);},
            "Home"|"End"=>{let pos=self.model.caret();let value=self.model.value();let target=if key=="Home" {value[..pos].rfind('\n').map_or(0,|i|i+1)} else {value[pos..].find('\n').map_or(value.len(),|i|{let end=pos+i;if value[..end].ends_with('\r'){end-1}else{end}})};let anchor=if shift {self.model.anchor()}else{target};let _=self.model.set_selection(anchor,target);},
            "ArrowUp"|"ArrowDown"=>{let p=self.caret_point();let desired=self.preferred_x.unwrap_or(p[0]);let y=p[1]+if key=="ArrowUp"{-self.line_height()}else{self.line_height()};self.hit(self.spec.bounds[0]+desired-self.offset[0],self.spec.bounds[1]+self.top()+y-self.offset[1]+self.line_height()*0.5,shift);self.preferred_x=Some(desired);},
            "Enter"=>{if self.spec.multiline {self.insert("\n");}},
            _=>return false,
        }
        if !matches!(key,"ArrowUp"|"ArrowDown"){self.preferred_x=None;}
        self.semantic_revision=next_semantic_revision();
        self.preedit.clear();
        true
    }
    pub fn insert(&mut self,text:&str) {
        if self.spec.read_only{return;}
        self.semantic_revision=next_semantic_revision();
        self.preferred_x=None;
        self.dirty=true;
        self.preedit.clear();
        let clean=text.replace("\r\n","\n").replace('\r',"\n");
        let clean=if self.spec.multiline {clean} else {clean.replace('\n'," ")};
        if self.model.value().len()-self.model.grapheme_selection().len()+clean.len()<=MAX_VALUE_BYTES {
            let selected=self.model.grapheme_selection();
            // Existing characters keep their age when an insertion shifts their byte offsets.
            self.fades.retain(|(range,_)|range.end<=selected.start || range.start>=selected.end);
            for (range,_) in &mut self.fades {
                if range.start>=selected.end {
                    range.start=range.start-selected.len()+clean.len();
                    range.end=range.end-selected.len()+clean.len();
                }
            }
            let _=self.model.insert_graphemes(&clean);
            if !self.reduced && !clean.is_empty() {self.fades.push((selected.start..selected.start+clean.len(),0.));}
            self.phase=0.;
        }
    }
    pub fn content(&mut self,focused:bool)->Vec<Content> {
        if focused!=self.focused {self.phase=0.;self.focused=focused;self.preferred_x=None;}
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
            let raw_len=line.len();
            let line=if start+raw_len<value.len(){line.strip_suffix('\r').unwrap_or(line)}else{line};
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
            if self.fades.is_empty() {
                out.push(Content::Text{bounds:[px,py,self.width(line),lh],text:Text{text:line.into(),font_size:self.spec.font_size,color:self.spec.color}});
            } else {
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
            }
            start+=raw_len+1;
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
    /// Computes visual soft-wrapped lines for this editor's current value constrained by its bounding width.
    pub fn soft_wrapped_lines(&self)->Vec<crate::text::SoftWrappedLine> {
        crate::text::wrap_shaped_text(self.model.value(),self.spec.font_size,self.spec.bounds[2].max(1.))
    }
    /// Hit-tests `(x, y)` in visual soft-wrapped layout coordinates and updates the editor selection/caret.
    pub fn hit_soft_wrapped(&mut self,x:f32,y:f32,extend:bool) {
        self.semantic_revision=next_semantic_revision();
        self.preferred_x=None;
        self.phase=0.;self.dirty=true;
        let lx=x-self.spec.bounds[0];
        let ly=y-self.spec.bounds[1]-self.top()+self.offset[1];
        let target=crate::text::hit_wrapped_text(self.model.value(),self.spec.font_size,self.spec.bounds[2].max(1.),self.line_height(),lx,ly);
        let anchor=if extend {self.model.anchor()} else {target};
        let _=self.model.set_selection(anchor,target);
    }
}
fn rect(x:f32,y:f32,w:f32,h:f32,color:[u8;4])->Content {Content::Shape{points:vec![[x,y],[x+w,y],[x+w,y+h],[x,y+h]],color}}

#[cfg(test)]
mod animation_tests {
    use super::*;
    #[test]
    fn ui_hit_and_keys_keep_extended_grapheme_clusters_atomic() {
        for cluster in ["e\u{301}","👍🏽","👨‍👩‍👧‍👦","🇬🇪","1\u{fe0f}\u{20e3}"] {
            let value=format!("A{cluster}B");let end=1+cluster.len();
            let mut editor=Editor::new(Input {value:value.clone(),bounds:[0.,0.,500.,30.],..Input::default()},vec![]).unwrap();
            for x in -10..500 {
                editor.hit(x as f32*0.5,15.,false);
                assert!([0,1,end,value.len()].contains(&editor.model.caret()),"hit split {cluster}");
            }
            editor.model.set_selection(end,end).unwrap();
            assert!(editor.key("ArrowLeft",true,false));assert_eq!(editor.model.selected_text(),cluster);
            editor.key("Backspace",false,false);assert_eq!(editor.model.value(),"AB");
            editor.key("z",false,true);assert_eq!(editor.model.value(),value);
            editor.model.set_selection(1,1).unwrap();editor.key("Delete",false,false);assert_eq!(editor.model.value(),"AB");
        }
    }
    #[test]
    fn ui_insert_joined_cluster_and_partial_replacement_keep_fades_valid() {
        let mut editor=Editor::new(Input {value:"\u{301}R".into(),..Input::default()},vec![]).unwrap();
        editor.model.home(false);editor.insert("e");
        assert_eq!(editor.model.caret(),3);editor.content(true);
        editor.model.set_selection(1,3).unwrap();editor.insert("X");
        assert_eq!(editor.model.value(),"XR");assert_eq!(editor.model.caret(),1);
        editor.content(true);editor.tick(100.);editor.content(true);
    }
    #[test]
    fn readonly_blocks_existing_undo_history_without_discarding_it(){
        let mut e=Editor::new(Input{value:"seed".into(),..Input::default()},vec![]).unwrap();e.insert("!");let edited=e.model.value().to_owned();e.spec.read_only=true;
        e.key("z",false,true);e.key("y",false,true);e.insert("blocked");e.set_preedit("blocked");assert_eq!(e.model.value(),edited);assert!(e.preedit.is_empty());
        e.spec.read_only=false;e.key("z",false,true);assert_eq!(e.model.value(),"seed");
    }
    #[test]
    fn command_deletion_targets_logical_line_edges_and_undo_is_atomic(){
        for forward in [false,true]{
            let mut e=Editor::new(Input{value:"one\r\ntwo three\r\nlast".into(),multiline:true,..Input::default()},vec![]).unwrap();
            e.model.set_selection(9,9).unwrap();e.set_preedit("pending");
            assert!(e.key(if forward{"Delete"}else{"Backspace"},false,true));assert!(e.preedit.is_empty());
            assert_eq!(e.model.value(),if forward{"one\r\ntwo \r\nlast"}else{"one\r\nthree\r\nlast"});
            e.key("z",false,true);assert_eq!(e.model.value(),"one\r\ntwo three\r\nlast");assert_eq!(e.model.caret(),9);
        }
    }
    #[test]
    fn vertical_navigation_retains_desired_column_across_short_rows() {
        let mut e=Editor::new(Input{value:"abcdefgh\nx\nabcdefgh".into(),multiline:true,bounds:[0.,0.,300.,100.],..Input::default()},vec![]).unwrap();
        e.model.set_selection(7,7).unwrap();
        e.key("ArrowDown",false,false);assert_eq!(e.model.caret(),10);
        e.key("ArrowDown",false,false);assert_eq!(e.model.caret(),18,"return to original column after short row");
        assert!(!e.key("Shift",true,false));e.key("ArrowUp",true,false);assert!(!e.key("Shift",true,false));e.key("ArrowUp",true,false);assert_eq!(e.model.caret(),7);assert_eq!(e.model.anchor(),18);
        e.key("ArrowRight",false,false);e.key("ArrowUp",false,false);e.key("ArrowUp",false,false);
        assert_eq!(e.model.caret(),7,"horizontal collapse resets desired column");
        e.hit(0.,1.,false);e.key("ArrowDown",false,false);e.key("ArrowDown",false,false);assert_eq!(e.model.caret(),11,"pointer resets desired column");
        e.model.set_selection(7,7).unwrap();e.preferred_x=None;e.key("ArrowDown",false,false);
        e.content(true);e.content(false);e.content(true);assert!(e.preferred_x.is_none(),"focus transition resets desired column");
    }
    #[test]
    fn multiline_crlf_and_lf_have_identical_visible_content_and_selection_geometry() {
        for selected in [false,true] {
            let mut crlf=Editor::new(Input{value:"first\r\nlast".into(),multiline:true,bounds:[0.,0.,200.,80.],..Input::default()},vec![]).unwrap();
            let mut lf=Editor::new(Input{value:"first\nlast".into(),multiline:true,bounds:[0.,0.,200.,80.],..Input::default()},vec![]).unwrap();
            crlf.reduced_motion(true);lf.reduced_motion(true);
            if selected{crlf.model.select_all();lf.model.select_all();}
            assert_eq!(crlf.content(true),lf.content(true),"selected={selected}");
            assert_eq!(crlf.caret_rect(),lf.caret_rect());
            assert_eq!(crlf.model.value(),"first\r\nlast","render must preserve bytes");
        }
    }
    #[test]
    fn multiline_hit_and_vertical_keys_do_not_land_inside_crlf() {
        let mut e=Editor::new(Input{value:"first\r\nlast".into(),multiline:true,bounds:[0.,0.,500.,100.],..Input::default()},vec![]).unwrap();
        e.hit(1000.,1.,false);assert_eq!(e.model.caret(),5,"first row right edge");
        e.hit(1000.,30.,false);assert_eq!(e.model.caret(),11,"last row right edge");
        e.key("ArrowUp",false,false);assert_ne!(e.model.caret(),6);
        e.hit(1000.,1.,false);e.key("ArrowDown",true,false);assert_eq!(e.model.selected_text(),"\r\nlast");
        for row in [1.,30.] {for x in -10..1000 {e.hit(x as f32,row,false);assert_ne!(e.model.caret(),6,"CRLF interior at x{x} row{row}");}}
    }
    #[test]
    fn multiline_end_never_splits_crlf_and_shift_deletion_preserves_line_break() {
        let value="e\u{301}\r\nnext";
        let mut e=Editor::new(Input{value:value.into(),multiline:true,..Input::default()},vec![]).unwrap();
        e.model.home(false);assert!(e.key("End",false,false));
        assert_eq!(e.model.caret(),"e\u{301}".len());
        e.model.home(false);assert!(e.key("End",true,false));assert_eq!(e.model.selected_text(),"e\u{301}");
        e.key("Backspace",false,false);assert_eq!(e.model.value(),"\r\nnext");
        e.key("z",false,true);assert_eq!(e.model.value(),value);
        e.model.set_selection(4,4).unwrap(); // interior scalar boundary between CR and LF
        e.key("End",false,false);assert_eq!(e.model.caret(),3);
        e.model.end(false);e.key("Home",false,false);assert_eq!(e.model.caret(),5);
        e.key("End",false,false);assert_eq!(e.model.caret(),value.len());
    }
    #[test]
    fn explicit_editor_keys_cancel_marked_text_without_recording_it_in_history() {
        let mut e=Editor::new(Input{value:"one two".into(),..Input::default()},vec![]).unwrap();
        e.insert("!");let committed=e.model.value().to_owned();
        e.set_preedit("未確定");assert!(!e.key("unknown",false,false));assert_eq!(e.preedit,"未確定");
        assert!(e.key("z",false,true));assert!(e.preedit.is_empty());assert_eq!(e.model.value(),"one two");
        assert!(e.key("z",true,true));assert_eq!(e.model.value(),committed);assert!(e.preedit.is_empty());
        for key in ["ArrowLeft","ArrowRight","Home","End","Backspace","Delete"] {
            e.set_preedit("未確定");assert!(e.key(key,false,false));assert!(e.preedit.is_empty(),"{key}");
        }
        for key in ["ArrowLeft","ArrowRight","Backspace","Delete"] {
            e.set_preedit("未確定");assert!(e.word_key(key,false));assert!(e.preedit.is_empty(),"word {key}");
        }
        assert!(!e.model.value().contains("未確定"));
    }
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
    #[test]
    fn soft_wrapped_lines_and_dictionary_word_movement_work_together() {
        let mut e=Editor::new(Input{value:"你好世界 Hello world from Forma".into(),multiline:true,bounds:[0.,0.,90.,80.],..Input::default()},vec![]).unwrap();
        let lines=e.soft_wrapped_lines();
        assert!(lines.len()>=2);
        e.hit_soft_wrapped(1.,e.line_height()*1.5,false);
        assert_eq!(e.model.caret(),lines[1].byte_range.start);
        e.model.home(false);
        assert!(e.model.move_words_dictionary(1,false));
        assert_eq!(e.model.caret(),"你好".len(),"dictionary groups CJK compound 你好");
        assert!(e.model.move_words_dictionary(1,false));
        assert_eq!(e.model.caret(),"你好世界".len(),"dictionary groups CJK compound 世界");
        assert!(e.model.backspace_word_dictionary());
        assert!(e.model.value().starts_with("你好 Hello"));
        assert!(e.model.undo());
        assert_eq!(e.model.value(),"你好世界 Hello world from Forma");
    }
}

#[cfg(test)]
mod semantic_epoch_tests {
    #[test]
    fn exhausted_epoch_never_reuses_an_earlier_cache_key() {
        let counter=std::sync::atomic::AtomicU32::new(u32::MAX-1);
        assert_eq!(super::allocate_semantic_revision(&counter),u32::MAX);
        for _ in 0..4 {assert_eq!(super::allocate_semantic_revision(&counter),0);}
        assert_eq!(counter.load(std::sync::atomic::Ordering::Relaxed),u32::MAX);
    }
}
