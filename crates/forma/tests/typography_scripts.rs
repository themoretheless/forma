use std::collections::BTreeMap;
use std::path::Path;

use forma::display_list::DisplayList;
use forma::text::{
    FontFamily, OpenTypeTables, RasterCanvas, ResolvedDirection, TextDirection, TextDraw,
    VectorScratch, colr_cpal_layers, draw_text_in, layout_runs, measure_shaped_text,
    resolve_direction, shape_text, shape_text_in, shape_text_with_dir, vector_colored_glyphs,
    wrap_text_ranges,
};
use forma::{Button, Runtime};

fn push_u16(buf: &mut Vec<u8>, v: u16) {
    buf.extend_from_slice(&v.to_be_bytes());
}
fn push_i16(buf: &mut Vec<u8>, v: i16) {
    buf.extend_from_slice(&v.to_be_bytes());
}
fn push_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_be_bytes());
}

fn build_sfnt(tables: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    let count = tables.len() as u16;
    push_u32(&mut out, 0x0001_0000);
    push_u16(&mut out, count);
    push_u16(&mut out, 16);
    push_u16(&mut out, 0);
    push_u16(&mut out, 0);
    let mut offset = 12 + tables.len() * 16;
    for (tag, data) in tables {
        out.extend_from_slice(tag);
        push_u32(&mut out, 0);
        push_u32(&mut out, offset as u32);
        push_u32(&mut out, data.len() as u32);
        offset += (data.len() + 3) & !3;
    }
    for (_, data) in tables {
        out.extend_from_slice(data);
        while !out.len().is_multiple_of(4) {
            out.push(0);
        }
    }
    out
}

fn build_synthetic_opentype_sfnt() -> Vec<u8> {
    // GDEF: version 1.0, GlyphClassDef at off 12 -> ClassDef fmt 1: gid 50 -> class 3 (Mark)
    let mut gdef = Vec::new();
    push_u32(&mut gdef, 0x0001_0000);
    push_u16(&mut gdef, 12); // glyphClassDefOffset
    push_u16(&mut gdef, 0);
    push_u16(&mut gdef, 0);
    push_u16(&mut gdef, 0);
    // ClassDef Format 1 at 12: start=50, count=1, [3]
    push_u16(&mut gdef, 1);
    push_u16(&mut gdef, 50);
    push_u16(&mut gdef, 1);
    push_u16(&mut gdef, 3);

    // GSUB: compute exact header offsets dynamically
    // ScriptList (1 script DFLT -> LangSys default -> [0, 1])
    let mut script_list = Vec::new();
    push_u16(&mut script_list, 1);
    script_list.extend_from_slice(b"DFLT");
    push_u16(&mut script_list, 8);
    // Script at 8:
    push_u16(&mut script_list, 4); // defaultLangSys
    push_u16(&mut script_list, 0);
    // LangSys at +4:
    push_u16(&mut script_list, 0);
    push_u16(&mut script_list, 0xFFFF);
    push_u16(&mut script_list, 2);
    push_u16(&mut script_list, 0);
    push_u16(&mut script_list, 1);

    // FeatureList (`isol` -> [0], `liga` -> [1])
    let mut feature_list = Vec::new();
    push_u16(&mut feature_list, 2);
    feature_list.extend_from_slice(b"isol");
    push_u16(&mut feature_list, 14);
    feature_list.extend_from_slice(b"liga");
    push_u16(&mut feature_list, 20);
    push_u16(&mut feature_list, 0);
    push_u16(&mut feature_list, 1);
    push_u16(&mut feature_list, 0);
    push_u16(&mut feature_list, 0);
    push_u16(&mut feature_list, 1);
    push_u16(&mut feature_list, 1);

    // LookupList: Lookup 0 (Ext7 -> SingleSubst2: 10->100), Lookup 1 (Lig4 IGNORE_MARKS: [10,11]->200)
    let mut l0 = Vec::new();
    push_u16(&mut l0, 7);
    push_u16(&mut l0, 0);
    push_u16(&mut l0, 1);
    push_u16(&mut l0, 8);
    push_u16(&mut l0, 1);
    push_u16(&mut l0, 1);
    push_u32(&mut l0, 8);
    push_u16(&mut l0, 2); // SingleSubst fmt 2
    push_u16(&mut l0, 8); // cov off
    push_u16(&mut l0, 1);
    push_u16(&mut l0, 100);
    push_u16(&mut l0, 1); // Cov fmt 1
    push_u16(&mut l0, 1);
    push_u16(&mut l0, 10);

    let mut l1 = Vec::new();
    push_u16(&mut l1, 4);
    push_u16(&mut l1, 0x0008);
    push_u16(&mut l1, 1);
    push_u16(&mut l1, 8);
    push_u16(&mut l1, 1); // LigSubst fmt 1
    push_u16(&mut l1, 8); // cov off
    push_u16(&mut l1, 1); // set count
    push_u16(&mut l1, 14); // set off
    push_u16(&mut l1, 1); // Cov [10]
    push_u16(&mut l1, 1);
    push_u16(&mut l1, 10);
    push_u16(&mut l1, 1); // LigatureSet
    push_u16(&mut l1, 4);
    push_u16(&mut l1, 200);
    push_u16(&mut l1, 2);
    push_u16(&mut l1, 11);

    let mut gsub_lookups = Vec::new();
    let l0_off = 6u16;
    let l1_off = l0_off + l0.len() as u16;
    push_u16(&mut gsub_lookups, 2);
    push_u16(&mut gsub_lookups, l0_off);
    push_u16(&mut gsub_lookups, l1_off);
    gsub_lookups.extend_from_slice(&l0);
    gsub_lookups.extend_from_slice(&l1);

    let mut gsub = Vec::new();
    let s_off = 10u16;
    let f_off = s_off + script_list.len() as u16;
    let l_off = f_off + feature_list.len() as u16;
    push_u32(&mut gsub, 0x0001_0000);
    push_u16(&mut gsub, s_off);
    push_u16(&mut gsub, f_off);
    push_u16(&mut gsub, l_off);
    gsub.extend_from_slice(&script_list);
    gsub.extend_from_slice(&feature_list);
    gsub.extend_from_slice(&gsub_lookups);

    // GPOS: ScriptList=10, FeatureList=12 (`kern`=[0], `mark`=[1], `mkmk`=[2]), LookupList
    let mut gpos = Vec::new();
    push_u32(&mut gpos, 0x0001_0000);
    push_u16(&mut gpos, 10); // ScriptList (empty, count=0 -> falls back to FeatureList)
    push_u16(&mut gpos, 12); // FeatureList
    push_u16(&mut gpos, 50); // LookupList (12 + 2 + 3*6 + 3*6 = 50)
    push_u16(&mut gpos, 0); // ScriptList count=0
    // FeatureList at 12 (len 38):
    push_u16(&mut gpos, 3);
    gpos.extend_from_slice(b"kern");
    push_u16(&mut gpos, 20);
    gpos.extend_from_slice(b"mark");
    push_u16(&mut gpos, 26);
    gpos.extend_from_slice(b"mkmk");
    push_u16(&mut gpos, 32);
    for l in [0u16, 1, 2] {
        push_u16(&mut gpos, 0);
        push_u16(&mut gpos, 1);
        push_u16(&mut gpos, l);
    }
    // LookupList at 50: count=3, offs placeholders:
    let list_start = gpos.len();
    push_u16(&mut gpos, 3);
    push_u16(&mut gpos, 0);
    push_u16(&mut gpos, 0);
    push_u16(&mut gpos, 0);

    // Lookup 0: PairPos fmt 2 (ClassDef) wrapped in ExtensionPos type 9
    let l0_off = (gpos.len() - list_start) as u16;
    gpos[list_start + 2..list_start + 4].copy_from_slice(&l0_off.to_be_bytes());
    push_u16(&mut gpos, 9); // ExtPos
    push_u16(&mut gpos, 0);
    push_u16(&mut gpos, 1);
    push_u16(&mut gpos, 8);
    // Extension fmt 1 -> inner type 2, off 8:
    push_u16(&mut gpos, 1);
    push_u16(&mut gpos, 2);
    push_u32(&mut gpos, 8);
    // PairPos fmt 2 (relative base):
    // fmt=2(0), cov=32(2), vf1=0x0004(4, XAdv), vf2=0(6), cd1=38(8), cd2=46(10), c1_cnt=2(12), c2_cnt=2(14)
    // matrix: [0, 0, 0, -45] (16..24), wait len = 16 + 4*2 = 24 -> cov at 24!
    push_u16(&mut gpos, 2);
    push_u16(&mut gpos, 24);
    push_u16(&mut gpos, 0x0004);
    push_u16(&mut gpos, 0);
    push_u16(&mut gpos, 30);
    push_u16(&mut gpos, 38);
    push_u16(&mut gpos, 2);
    push_u16(&mut gpos, 2);
    push_i16(&mut gpos, 0);
    push_i16(&mut gpos, 0);
    push_i16(&mut gpos, 0);
    push_i16(&mut gpos, -45); // class1=1, class2=1 -> -45 units
    // Cov fmt 1 at 24 (6 bytes): [20]
    push_u16(&mut gpos, 1);
    push_u16(&mut gpos, 1);
    push_u16(&mut gpos, 20);
    // ClassDef1 fmt 1 at 30 (8 bytes): gid 20 -> class 1
    push_u16(&mut gpos, 1);
    push_u16(&mut gpos, 20);
    push_u16(&mut gpos, 1);
    push_u16(&mut gpos, 1);
    // ClassDef2 fmt 1 at 38 (8 bytes): gid 21 -> class 1
    push_u16(&mut gpos, 1);
    push_u16(&mut gpos, 21);
    push_u16(&mut gpos, 1);
    push_u16(&mut gpos, 1);

    // Lookup 1: MarkToBase type 4 (base gid 10 anchor (300, 650), mark gid 50 anchor (50, 450) -> (250, 200))
    let l1_off = (gpos.len() - list_start) as u16;
    gpos[list_start + 4..list_start + 6].copy_from_slice(&l1_off.to_be_bytes());
    push_u16(&mut gpos, 4);
    push_u16(&mut gpos, 0);
    push_u16(&mut gpos, 1);
    push_u16(&mut gpos, 8);
    // MarkToBase fmt 1: fmt(0), markCov(2)=12, baseCov(4)=18, clsCnt(6)=1, markArr(8)=24, baseArr(10)=36
    push_u16(&mut gpos, 1);
    push_u16(&mut gpos, 12);
    push_u16(&mut gpos, 18);
    push_u16(&mut gpos, 1);
    push_u16(&mut gpos, 24);
    push_u16(&mut gpos, 36);
    // markCov (6): [50]
    push_u16(&mut gpos, 1);
    push_u16(&mut gpos, 1);
    push_u16(&mut gpos, 50);
    // baseCov (6): [10]
    push_u16(&mut gpos, 1);
    push_u16(&mut gpos, 1);
    push_u16(&mut gpos, 10);
    // markArray (12): count=1, rec=(cls=0, anchorOff=6), Anchor fmt 1 (1, 50, 450)
    push_u16(&mut gpos, 1);
    push_u16(&mut gpos, 0);
    push_u16(&mut gpos, 6);
    push_u16(&mut gpos, 1);
    push_i16(&mut gpos, 50);
    push_i16(&mut gpos, 450);
    // baseArray (10): count=1, rec=(anchorOff=4), Anchor fmt 1 (1, 300, 650)
    push_u16(&mut gpos, 1);
    push_u16(&mut gpos, 4);
    push_u16(&mut gpos, 1);
    push_i16(&mut gpos, 300);
    push_i16(&mut gpos, 650);

    // Lookup 2: MarkToMark type 6 (mark1 gid 50 anchor (100, 700), mark2 gid 51 anchor (20, 500) -> (80, 200))
    let l2_off = (gpos.len() - list_start) as u16;
    gpos[list_start + 6..list_start + 8].copy_from_slice(&l2_off.to_be_bytes());
    push_u16(&mut gpos, 6);
    push_u16(&mut gpos, 0);
    push_u16(&mut gpos, 1);
    push_u16(&mut gpos, 8);
    push_u16(&mut gpos, 1);
    push_u16(&mut gpos, 12);
    push_u16(&mut gpos, 18);
    push_u16(&mut gpos, 1);
    push_u16(&mut gpos, 24);
    push_u16(&mut gpos, 36);
    push_u16(&mut gpos, 1);
    push_u16(&mut gpos, 1);
    push_u16(&mut gpos, 51);
    push_u16(&mut gpos, 1);
    push_u16(&mut gpos, 1);
    push_u16(&mut gpos, 50);
    push_u16(&mut gpos, 1);
    push_u16(&mut gpos, 0);
    push_u16(&mut gpos, 6);
    push_u16(&mut gpos, 1);
    push_i16(&mut gpos, 20);
    push_i16(&mut gpos, 500);
    push_u16(&mut gpos, 1);
    push_u16(&mut gpos, 4);
    push_u16(&mut gpos, 1);
    push_i16(&mut gpos, 100);
    push_i16(&mut gpos, 700);

    // COLR v0 + CPAL v0: base gid 77 -> layer (gid 78, entry 0=#FF4422) + layer (gid 79, entry 0xFFFF=fg)
    let mut colr = Vec::new();
    push_u16(&mut colr, 0); // version 0
    push_u16(&mut colr, 1); // numBaseGlyphRecords
    push_u32(&mut colr, 14); // baseGlyphRecordsOffset
    push_u32(&mut colr, 20); // layerRecordsOffset
    push_u16(&mut colr, 2); // numLayerRecords
    push_u16(&mut colr, 77);
    push_u16(&mut colr, 0);
    push_u16(&mut colr, 2);
    push_u16(&mut colr, 78);
    push_u16(&mut colr, 0);
    push_u16(&mut colr, 79);
    push_u16(&mut colr, 0xFFFF);

    let mut cpal = Vec::new();
    push_u16(&mut cpal, 0); // version 0
    push_u16(&mut cpal, 1); // numPaletteEntries
    push_u16(&mut cpal, 1); // numPalettes
    push_u16(&mut cpal, 1); // numColorRecords
    push_u32(&mut cpal, 14); // colorRecordsArrayOffset
    push_u16(&mut cpal, 0); // colorRecordIndices[0]
    // BGRA: blue=0x22, green=0x44, red=0xFF, alpha=0xFF
    cpal.extend_from_slice(&[0x22, 0x44, 0xFF, 0xFF]);

    build_sfnt(&[
        (*b"GDEF", gdef),
        (*b"GSUB", gsub),
        (*b"GPOS", gpos),
        (*b"COLR", colr),
        (*b"CPAL", cpal),
    ])
}

fn dominant_rgb(pixels: &[u8]) -> (f32, f32, f32, usize) {
    let mut r = 0u64;
    let mut g = 0u64;
    let mut b = 0u64;
    let mut count = 0usize;
    for px in pixels.chunks_exact(4) {
        if px[3] >= 64 {
            r += u64::from(px[0]);
            g += u64::from(px[1]);
            b += u64::from(px[2]);
            count += 1;
        }
    }
    if count == 0 {
        return (0., 0., 0., 0);
    }
    (
        r as f32 / count as f32,
        g as f32 / count as f32,
        b as f32 / count as f32,
        count,
    )
}

fn render_text_pixels(text: &str, size: f32, fg: [u8; 4]) -> Vec<u8> {
    let mut pixels = vec![0u8; 96 * 96 * 4];
    draw_text_in(
        FontFamily::Sans,
        &mut RasterCanvas::full(&mut pixels, 96, 96, 1.0),
        text,
        size,
        [0., 0., 96., 96.],
        fg,
    );
    pixels
}

fn build_record() -> String {
    let mut out = String::new();

    // 1. RTL paragraph layout & BiDi navigation in .ui
    assert_eq!(
        resolve_direction("hello שלום", TextDirection::Auto),
        ResolvedDirection::Ltr
    );
    assert_eq!(
        resolve_direction("שלום hello", TextDirection::Auto),
        ResolvedDirection::Rtl
    );
    assert_eq!(
        resolve_direction("hello", TextDirection::Rtl),
        ResolvedDirection::Rtl
    );

    let files = BTreeMap::from([(
        "App.ui".into(),
        r#"component App {
            Column {
                width: 300;
                height: 160;
                Text { key: 'rtl_lbl'; width: 280; height: 32; dir: 'rtl'; text: 'שלום abc'; color: #ffffff; }
                TextInput { key: 'rtl_in'; width: 280; height: 40; direction: 'rtl'; value: 'שלום abc'; }
            }
        }"#
        .into(),
    )]);
    let mut rt = Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
    rt.focus_control(1);
    rt.text_key("Home", false, false);
    let rtl_home_x = rt.text_caret_bounds()[0];
    rt.text_key("End", false, false);
    let rtl_end_x = rt.text_caret_bounds()[0];
    assert!(
        rtl_home_x > rtl_end_x + 20.0,
        "RTL input Home ({rtl_home_x}) must sit visually right of End ({rtl_end_x})"
    );
    let rtl_ui_px = rt.pixels(300, 160, 1.0);
    assert!(rtl_ui_px.chunks_exact(4).any(|p| p[3] > 0));

    let mut stops = Vec::new();
    rt.text_key("Home", false, false);
    stops.push(rt.text_selection_utf16()[1]);
    for _ in 0..8 {
        rt.text_key("ArrowLeft", false, false);
        stops.push(rt.text_selection_utf16()[1]);
    }
    let unique_stops: std::collections::BTreeSet<u32> = stops.iter().copied().collect();
    assert_eq!(
        unique_stops.len(),
        9,
        "every boundary in 'שלום abc' must be distinct: {stops:?}"
    );
    out.push_str(&format!("rtl.bijection_stops={stops:?}\n"));

    // 2. Extended Arabic + vocalized Lam-Alef
    let persian = "پچژگکی";
    let urdu = "ٹڈڑںہھے";
    let pashto = "ځبڅ";
    let vocal_lam = "سلَام";
    assert_eq!(shape_text("لَا", 20.0).len(), 2);
    assert_eq!(shape_text(vocal_lam, 20.0).len(), 4);
    let shaped_pashto = shape_text(pashto, 20.0);
    assert_eq!(shaped_pashto.len(), 3);
    assert_eq!(shaped_pashto[0].font_index, 6);
    assert_ne!(shaped_pashto[0].glyph_id, shaped_pashto[2].glyph_id);
    out.push_str(&format!(
        "arabic.extended_gids=persian:{},urdu:{},pashto:{:?},lam_alef:{}\n",
        shape_text(persian, 20.0).len(),
        shape_text(urdu, 20.0).len(),
        shaped_pashto.iter().map(|g| g.glyph_id).collect::<Vec<_>>(),
        shape_text(vocal_lam, 20.0).len()
    ));

    // 3. OpenType GPOS/GSUB (synthetic SFNT + bundled fonts)
    let sfnt = build_synthetic_opentype_sfnt();
    let tables = OpenTypeTables::parse(&sfnt);
    assert!(tables.is_mark_glyph(50));
    assert!(!tables.is_mark_glyph(10));
    assert_eq!(tables.single_subst(&[*b"DFLT"], *b"isol", 10), Some(100));
    assert_eq!(
        tables.ligature_subst(
            &[*b"DFLT"],
            &[*b"liga"],
            &[10, 50, 11],
            &[false, true, false]
        ),
        Some((200, vec![0, 2]))
    );
    assert_eq!(
        tables.pair_kern(&[*b"DFLT"], 20, 21).map(|a| a.x_advance_1),
        Some(-45)
    );
    assert_eq!(
        tables.mark_to_base(&[*b"DFLT"], 10, 50).map(|d| (d.x, d.y)),
        Some((250, 200))
    );
    assert_eq!(
        tables.mark_to_mark(&[*b"DFLT"], 50, 51).map(|d| (d.x, d.y)),
        Some((80, 200))
    );
    let bold_kern = shape_text_in(FontFamily::SansBold, "AV", 24.0);
    let unkerned = measure_shaped_text("To", 24.0);
    let rtl_mixed =
        shape_text_with_dir(FontFamily::Sans, "مرحبا 42 عالم", 20.0, TextDirection::Auto);
    out.push_str(&format!(
        "opentype.synthetic=ok; bold_AV_adv={:.2}; To_w={:.2}; arabic_bidi_glyphs={}\n",
        bold_kern.iter().map(|g| g.x_advance).sum::<f32>(),
        unkerned,
        rtl_mixed.len()
    ));

    // 4. Color Emoji (COLR/CPAL v0 + FormaEmoji multi-tone + VS15 + skin tones + DisplayList parity)
    let colr_layers = colr_cpal_layers(&sfnt, 77, 0, [10, 20, 30, 255]).unwrap();
    assert_eq!(
        colr_layers,
        vec![(78, [255, 68, 34, 255]), (79, [10, 20, 30, 255])]
    );
    let scaled_colr = colr_cpal_layers(&sfnt, 77, 0, [10, 20, 30, 128]).unwrap();
    assert_eq!(scaled_colr[0].1[3], 128);

    let smiley_px = render_text_pixels("🙂", 48.0, [255, 255, 255, 255]);
    let (sr, sg, sb, scount) = dominant_rgb(&smiley_px);
    assert!(
        sr > sb + 50.0 && sg > sb + 20.0 && scount > 100,
        "smiley must render yellow: ({sr},{sg},{sb})"
    );

    let heart_color = render_text_pixels("❤", 48.0, [255, 255, 255, 255]);
    let (hr, hg, hb, _) = dominant_rgb(&heart_color);
    assert!(
        hr > hg + 80.0 && hr > hb + 80.0,
        "❤ must render crimson: ({hr},{hg},{hb})"
    );

    let heart_mono = render_text_pixels("❤\u{FE0E}", 48.0, [255, 255, 255, 255]);
    let (mr, mg, mb, _) = dominant_rgb(&heart_mono);
    assert!(
        (mr - mg).abs() < 2.0 && (mg - mb).abs() < 2.0,
        "VS15 ❤︎ must remain monochrome: ({mr},{mg},{mb})"
    );

    let light_hand = render_text_pixels("👍🏻", 48.0, [255, 255, 255, 255]);
    let dark_hand = render_text_pixels("👍🏿", 48.0, [255, 255, 255, 255]);
    let (lr, lg, lb, _) = dominant_rgb(&light_hand);
    let (dr, dg, db, _) = dominant_rgb(&dark_hand);
    assert!(
        lr + lg + lb > dr + dg + db + 80.0,
        "light tone must exceed dark tone"
    );

    let mut scratch = VectorScratch::prewarm();
    let mut dl_colors = Vec::new();
    vector_colored_glyphs(
        &TextDraw {
            family: FontFamily::Sans,
            dir: TextDirection::Ltr,
            text: "🙂",
            font_size: 48.0,
            rect: [0., 0., 96., 96.],
            scale: 1.0,
            color: [255, 255, 255, 255],
        },
        &mut scratch,
        |_, color| dl_colors.push(color),
    );
    assert!(
        dl_colors.len() >= 2 && dl_colors[0][0] > dl_colors[0][2],
        "vector_colored_glyphs must emit multi-tone layers: {dl_colors:?}"
    );

    let emoji_btn = Button::from_sources(
        "component App { Frame { width: 100; height: 60; Button { width: 80; height: 48; text: '🙂'; } } }",
        forma::BUTTON_COMPONENT,
    )
    .unwrap();
    let dl = DisplayList::build(&emoji_btn, 1.0, false);
    assert!(dl.commands.len() / forma::display_list::STRIDE >= 2);
    let dl_has_gold = dl
        .commands
        .chunks_exact(forma::display_list::STRIDE)
        .any(|c| c[0] == 1.0 && c[8] > c[10] + 0.3);
    assert!(
        dl_has_gold,
        "DisplayList must emit warm gold path command for emoji fill"
    );
    out.push_str(&format!("color_emoji.dl_layers={dl_colors:?}\n"));

    // 5. UAX #14 / CJK Kinsoku line-breaking (`wrap` & `runs`)
    let kinsoku = "「こんにちは。」";
    let two_cell = measure_shaped_text("「こん", 16.0) + 1.0;
    let wrapped = wrap_text_ranges(kinsoku, 16.0, two_cell);
    let wrapped_strs: Vec<&str> = wrapped.iter().map(|(r, _)| &kinsoku[r.clone()]).collect();
    for line in &wrapped_strs {
        assert!(
            !line.starts_with('。') && !line.starts_with('」'),
            "kinsoku violated on {wrapped_strs:?}"
        );
        assert!(
            !line.ends_with('「'),
            "opening punct orphaned on {wrapped_strs:?}"
        );
    }

    let nbsp = "Hello\u{00A0}World test";
    let nbsp_w = wrap_text_ranges(nbsp, 16.0, measure_shaped_text("Hello", 16.0) + 5.0);
    let nbsp_strs: Vec<&str> = nbsp_w.iter().map(|(r, _)| &nbsp[r.clone()]).collect();
    assert!(
        !nbsp_strs[0].ends_with("Hello"),
        "NBSP must not break before World: {nbsp_strs:?}"
    );

    let punct = "foo (bar)!";
    let punct_w = wrap_text_ranges(punct, 16.0, measure_shaped_text("foo (bar)", 16.0) + 0.5);
    let punct_strs: Vec<&str> = punct_w.iter().map(|(r, _)| &punct[r.clone()]).collect();
    assert_eq!(punct_strs, vec!["foo ", "(bar)!"]);

    let run_kinsoku = layout_runs(
        &[
            (FontFamily::Sans, "「こん"),
            (FontFamily::SansBold, "にちは。」"),
        ],
        16.0,
        Some(two_cell),
    );
    let run_lines: Vec<String> = (0..run_kinsoku.lines)
        .map(|ln| {
            run_kinsoku
                .fragments
                .iter()
                .filter(|f| f.line == ln)
                .map(|f| {
                    let s = if f.run == 0 {
                        "「こん"
                    } else {
                        "にちは。」"
                    };
                    &s[f.range.clone()]
                })
                .collect::<String>()
        })
        .collect();
    for line in &run_lines {
        assert!(
            !line.starts_with('。') && !line.starts_with('」'),
            "runs kinsoku violated: {run_lines:?}"
        );
    }
    out.push_str(&format!(
        "linebreak.kinsoku={wrapped_strs:?}; punct={punct_strs:?}; runs={run_lines:?}\n"
    ));

    out
}

#[test]
fn typography_scripts_match_golden_record() {
    let actual = build_record();
    let golden_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/typography_scripts.txt");
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(golden_path.parent().unwrap()).unwrap();
        std::fs::write(&golden_path, &actual).unwrap();
    }
    let expected = std::fs::read_to_string(&golden_path)
        .unwrap_or_else(|e| panic!("missing golden {}: {e}", golden_path.display()));
    assert_eq!(actual, expected);
}
