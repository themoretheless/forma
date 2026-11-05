//! Exhaustive fixed-weight outline/advance comparison; not a full raster/sequence gate.
use ttf_parser::{Face,GlyphId,OutlineBuilder,Tag};
#[derive(Default)]
struct Outline(Vec<(u8,[f32;6])>);
impl OutlineBuilder for Outline {
 fn move_to(&mut self,x:f32,y:f32){self.0.push((0,[x,y,0.,0.,0.,0.]));}
 fn line_to(&mut self,x:f32,y:f32){self.0.push((1,[x,y,0.,0.,0.,0.]));}
 fn quad_to(&mut self,x:f32,y:f32,a:f32,b:f32){self.0.push((2,[x,y,a,b,0.,0.]));}
 fn curve_to(&mut self,x:f32,y:f32,a:f32,b:f32,c:f32,d:f32){self.0.push((3,[x,y,a,b,c,d]));}
 fn close(&mut self){self.0.push((4,[0.;6]));}
}
fn main(){
 let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/fallback");let mut rows=Vec::new();
 let candidate_root=std::env::var_os("FORMA_FONT_CANDIDATE_DIR").map(std::path::PathBuf::from).unwrap_or_else(||root.clone());
 for (original,derived) in [("NotoSansSC.ttf","FormaCJK-Regular.ttf"),("NotoEmoji.ttf","FormaEmoji-Regular.ttf")] {
  let original_bytes=std::fs::read(root.join(original)).unwrap();let derived_bytes=std::fs::read(candidate_root.join(derived)).unwrap();
  let mut old=Face::parse(&original_bytes,0).unwrap();assert!(old.set_variation(Tag::from_bytes(b"wght"),400.).is_some());let new=Face::parse(&derived_bytes,0).unwrap();
  assert_eq!(old.number_of_glyphs(),new.number_of_glyphs());let ratio=old.units_per_em() as f32/new.units_per_em() as f32;
  let mut points=std::collections::BTreeSet::new();for table in old.tables().cmap.unwrap().subtables{table.codepoints(|ch|{points.insert(ch);});}
  for &code in &points {if let Some(ch)=char::from_u32(code){assert_eq!(old.glyph_index(ch),new.glyph_index(ch),"cmap {code}");}}
  let(mut a,mut b)=(Outline::default(),Outline::default());let mut max_coordinate_error=0_f32;let mut max_advance_error=0_f32;let mut max_vertical_advance_error=0_f32;let mut topology_mismatches=Vec::new();let mut outline_glyphs=0;
  for index in 0..old.number_of_glyphs(){let id=GlyphId(index);a.0.clear();b.0.clear();
   let present=old.outline_glyph(id,&mut a).is_some();assert_eq!(present,new.outline_glyph(id,&mut b).is_some());if present{outline_glyphs+=1;}
   if a.0.len()!=b.0.len()||a.0.iter().zip(&b.0).any(|(x,y)|x.0!=y.0){topology_mismatches.push(index);}else{for(x,y)in a.0.iter().zip(&b.0){for(v,w)in x.1.iter().zip(y.1){assert!(v.is_finite()&&w.is_finite());max_coordinate_error=max_coordinate_error.max((v-w*ratio).abs());}}}
   match(old.glyph_hor_advance(id),new.glyph_hor_advance(id)){(Some(x),Some(y))=>max_advance_error=max_advance_error.max((x as f32-y as f32*ratio).abs()),(None,None)=>{},_=>panic!("advance availability changed")}
   match(old.glyph_ver_advance(id),new.glyph_ver_advance(id)){(Some(x),Some(y))=>max_vertical_advance_error=max_vertical_advance_error.max((x as f32-y as f32*ratio).abs()),(None,None)=>{},_=>panic!("vertical advance availability changed")}
  }
  let mut global_metrics=Vec::new();
  for (name,a,b) in [
   ("ascender",Some(old.ascender()),Some(new.ascender())),
   ("descender",Some(old.descender()),Some(new.descender())),
   ("line_gap",Some(old.line_gap()),Some(new.line_gap())),
   ("x_height",old.x_height(),new.x_height()),
   ("capital_height",old.capital_height(),new.capital_height()),
  ] {
   let error=match(a,b){(Some(x),Some(y))=>Some((x as f32-y as f32*ratio).abs()),(None,None)=>None,_=>panic!("global metric availability changed: {name}")};
   global_metrics.push(serde_json::json!({"name":name,"original":a,"candidate":b,"normalized_error":error}));
  }
  rows.push(serde_json::json!({"original":original,"derived":derived,"original_upem":old.units_per_em(),"candidate_upem":new.units_per_em(),"coordinate_error_limit_original_units":0.5*ratio+0.001,"glyphs":old.number_of_glyphs(),"cmap_codepoints":points.len(),"outline_glyphs":outline_glyphs,"max_coordinate_error_font_units":max_coordinate_error,"max_advance_error_font_units":max_advance_error,"max_vertical_advance_error_font_units":max_vertical_advance_error,"global_metrics":global_metrics,"topology_mismatches":topology_mismatches}));
 }
 println!("{}",serde_json::json!({"scope":"All cmap mappings and glyph outlines at weight400 via ttf-parser; no raster/sequence qualification","rows":rows}));
 for row in &rows {
  for metric in row["global_metrics"].as_array().unwrap(){if let Some(error)=metric["normalized_error"].as_f64(){assert_eq!(error,0.,"global metric changed: {}",metric["name"]);}}
  assert!(row["topology_mismatches"].as_array().unwrap().is_empty(), "outline topology changed");
  assert_eq!(row["max_advance_error_font_units"].as_f64().unwrap(),0.,"normalized horizontal advances changed");
  assert_eq!(row["max_vertical_advance_error_font_units"].as_f64().unwrap(),0.,"normalized vertical advances changed");
  assert!(row["max_coordinate_error_font_units"].as_f64().unwrap()<=row["coordinate_error_limit_original_units"].as_f64().unwrap(),"outline rounding exceeds normalized half-unit plus f32 margin");
 }

}
