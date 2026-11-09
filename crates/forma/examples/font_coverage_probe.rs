//! Verify bundled fallback font provenance/outline capability before renderer integration.
use ttf_parser::{Face, OutlineBuilder, Tag};
#[derive(Default)]
struct OutlineCount(usize);
impl OutlineBuilder for OutlineCount {
    fn move_to(&mut self, _: f32, _: f32) {
        self.0 += 1;
    }
    fn line_to(&mut self, _: f32, _: f32) {
        self.0 += 1;
    }
    fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) {
        self.0 += 1;
    }
    fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {
        self.0 += 1;
    }
    fn close(&mut self) {}
}
fn main() {
    let mut results = Vec::new();
    for (name, bytes, chars) in [
        (
            "NotoSansSC",
            include_bytes!("../assets/fallback/NotoSansSC.ttf").as_slice(),
            "你好中文日本語",
        ),
        (
            "NotoEmoji",
            include_bytes!("../assets/fallback/NotoEmoji.ttf").as_slice(),
            "🙂🦀❤☀",
        ),
    ] {
        let mut face = Face::parse(bytes, 0).expect("valid bundled font");
        assert!(face.set_variation(Tag::from_bytes(b"wght"), 400.).is_some());
        let mut glyphs = Vec::new();
        for ch in chars.chars() {
            let id = face.glyph_index(ch).expect("sample glyph covered");
            let advance = face.glyph_hor_advance(id).expect("horizontal advance");
            let mut contours = OutlineCount::default();
            assert!(face.outline_glyph(id, &mut contours).is_some());
            assert!(contours.0 > 0);
            assert!(advance > 0);
            glyphs.push(serde_json::json!({"scalar":ch.to_string(),"glyph_id":id.0,"advance_units":advance,"outline_segments":contours.0}));
        }
        results.push(serde_json::json!({"font":name,"bytes":bytes.len(),"units_per_em":face.units_per_em(),"weight":400,"glyphs":glyphs}));
    }
    println!(
        "{}",
        serde_json::json!({"status":"passed","scope":"Bundled fonts and ttf-parser outlines only; renderer/font selection not yet integrated","results":results})
    );
}
