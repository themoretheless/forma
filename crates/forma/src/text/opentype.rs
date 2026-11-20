//! Safe, bounds-checked OpenType SFNT `GDEF`, `GSUB`, and `GPOS` table engine.
//!
//! Zero `unsafe` and zero panicking slice reads. Supports:
//! - SFNT directory lookup (`find_table`, reused by `color_emoji` for `COLR`/`CPAL`)
//! - `GDEF` glyph class lookup (`Base`, `Ligature`, `Mark`)
//! - `GSUB` Lookup Type 1 (`SingleSubst` formats 1 & 2), Type 4 (`LigatureSubst` format 1),
//!   and Type 7 (`ExtensionSubst`) for `ccmp`, `liga`, `rlig`, `isol`, `init`, `medi`, `fina`
//! - `GPOS` Lookup Type 1 (`SinglePos`), Type 2 (`PairPos` formats 1 & 2 for `kern`),
//!   Type 4 (`MarkToBase` format 1), Type 6 (`MarkToMark` format 1), and Type 9 (`ExtensionPos`)
//! - Script/language lookup selection (`DFLT` + script tag) with `FeatureList` fallback for
//!   compact synthetic SFNT fixtures.

use std::sync::OnceLock;

/// Locates an SFNT table payload by 4-byte tag within raw TTF/OTF bytes.
pub(super) fn find_table(sfnt: &[u8], tag: [u8; 4]) -> Option<&[u8]> {
    let num_tables = usize::from(u16_be(sfnt, 4)?);
    let dir = sfnt.get(12..12 + num_tables.checked_mul(16)?)?;
    for entry in dir.chunks_exact(16) {
        if entry[0..4] == tag {
            let offset = usize::try_from(u32_be(entry, 8)?).ok()?;
            let len = usize::try_from(u32_be(entry, 12)?).ok()?;
            return sfnt.get(offset..offset.checked_add(len)?);
        }
    }
    None
}

#[inline]
pub(super) fn u16_be(bytes: &[u8], offset: usize) -> Option<u16> {
    let slice = bytes.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_be_bytes([slice[0], slice[1]]))
}

#[inline]
pub(super) fn i16_be(bytes: &[u8], offset: usize) -> Option<i16> {
    let slice = bytes.get(offset..offset.checked_add(2)?)?;
    Some(i16::from_be_bytes([slice[0], slice[1]]))
}

#[inline]
pub(super) fn u32_be(bytes: &[u8], offset: usize) -> Option<u32> {
    let slice = bytes.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

#[inline]
pub(super) fn sub(bytes: &[u8], offset: u16) -> Option<&[u8]> {
    bytes.get(usize::from(offset)..)
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PairAdjustment {
    pub x_placement_1: i16,
    pub y_placement_1: i16,
    pub x_advance_1: i16,
    pub x_placement_2: i16,
    pub y_placement_2: i16,
    pub x_advance_2: i16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MarkOffset {
    pub x: i16,
    pub y: i16,
}

/// Borrowed view over parsed `GDEF`, `GSUB`, and `GPOS` SFNT tables for one font face.
#[derive(Clone, Copy, Debug)]
pub struct OpenTypeTables<'a> {
    gdef: Option<&'a [u8]>,
    gsub: Option<&'a [u8]>,
    gpos: Option<&'a [u8]>,
}

impl<'a> OpenTypeTables<'a> {
    /// Inspects `GDEF`, `GSUB`, and `GPOS` slices within an arbitrary SFNT byte buffer.
    pub fn parse(sfnt: &'a [u8]) -> Self {
        Self {
            gdef: find_table(sfnt, *b"GDEF"),
            gsub: find_table(sfnt, *b"GSUB"),
            gpos: find_table(sfnt, *b"GPOS"),
        }
    }

    pub fn for_bundled(font_index: u8) -> Option<&'static OpenTypeTables<'static>> {
        bundled_tables(font_index)
    }

    /// Returns true if `GPOS` exposes any `kern` pair-positioning lookup.
    pub fn has_gpos_kern(&self) -> bool {
        let Some(gpos) = self.gpos else {
            return false;
        };
        !collect_lookups(gpos, &[], &[*b"kern"]).is_empty()
    }

    /// Queries `GDEF` GlyphClassDef (`1=Base, 2=Ligature, 3=Mark, 4=Component`).
    pub fn is_mark_glyph(&self, gid: u16) -> bool {
        let Some(gdef) = self.gdef else {
            return false;
        };
        let Some(class_off) = u16_be(gdef, 4).filter(|&o| o != 0) else {
            return false;
        };
        let Some(class_def) = sub(gdef, class_off) else {
            return false;
        };
        class_of(class_def, gid) == 3
    }

    /// Applies single-glyph OpenType substitution lookups for `feature` (`isol`, `init`, `medi`,
    /// `fina`, or `ccmp`) across matching `script` tags.
    pub fn single_subst(&self, scripts: &[[u8; 4]], feature: [u8; 4], gid: u16) -> Option<u16> {
        let gsub = self.gsub?;
        let mut current = gid;
        let mut changed = false;
        for lookup_idx in collect_lookups(gsub, scripts, &[feature]) {
            let Some((lookup_flag, subtables)) = lookup_at(gsub, lookup_idx, false) else {
                continue;
            };
            let _ = lookup_flag;
            for (kind, data) in subtables {
                if kind == 1 {
                    if let Some(next) = apply_single_subst(data, current) {
                        current = next;
                        changed = true;
                        break;
                    }
                }
            }
        }
        changed.then_some(current)
    }

    /// Matches a `GSUB` Lookup Type 4 ligature starting at `gids[0]`, respecting `IGNORE_MARKS`
    /// (`0x0008`) when `is_mark(g)` is true. Returns `(ligature_gid, consumed_indices)`.
    pub fn ligature_subst(
        &self,
        scripts: &[[u8; 4]],
        features: &[[u8; 4]],
        gids: &[u16],
        is_mark: &[bool],
    ) -> Option<(u16, Vec<usize>)> {
        let gsub = self.gsub?;
        let &first = gids.first()?;
        for lookup_idx in collect_lookups(gsub, scripts, features) {
            let Some((flag, subtables)) = lookup_at(gsub, lookup_idx, false) else {
                continue;
            };
            let ignore_marks = (flag & 0x0008) != 0;
            if ignore_marks && is_mark.first().copied().unwrap_or(false) {
                continue;
            }
            for (kind, data) in subtables {
                if kind == 4 {
                    if let Some((lig, consumed)) =
                        apply_ligature_subst(data, first, gids, is_mark, ignore_marks, self.gdef)
                    {
                        return Some((lig, consumed));
                    }
                }
            }
        }
        None
    }

    /// Evaluates horizontal pair positioning (`kern`) between `first` and `second`.
    pub fn pair_kern(
        &self,
        scripts: &[[u8; 4]],
        first: u16,
        second: u16,
    ) -> Option<PairAdjustment> {
        let gpos = self.gpos?;
        let mut total = PairAdjustment::default();
        let mut hit = false;
        for lookup_idx in collect_lookups(gpos, scripts, &[*b"kern"]) {
            let Some((_, subtables)) = lookup_at(gpos, lookup_idx, true) else {
                continue;
            };
            for (kind, data) in subtables {
                if kind == 2 {
                    if let Some(adj) = apply_pair_pos(data, first, second) {
                        total.x_placement_1 = total.x_placement_1.saturating_add(adj.x_placement_1);
                        total.y_placement_1 = total.y_placement_1.saturating_add(adj.y_placement_1);
                        total.x_advance_1 = total.x_advance_1.saturating_add(adj.x_advance_1);
                        total.x_placement_2 = total.x_placement_2.saturating_add(adj.x_placement_2);
                        total.y_placement_2 = total.y_placement_2.saturating_add(adj.y_placement_2);
                        total.x_advance_2 = total.x_advance_2.saturating_add(adj.x_advance_2);
                        hit = true;
                        break;
                    }
                }
            }
        }
        hit.then_some(total)
    }

    /// Evaluates `GPOS` `mark` (`MarkToBase`) attachment `(base_anchor - mark_anchor)`.
    pub fn mark_to_base(
        &self,
        scripts: &[[u8; 4]],
        base_gid: u16,
        mark_gid: u16,
    ) -> Option<MarkOffset> {
        let gpos = self.gpos?;
        for lookup_idx in collect_lookups(gpos, scripts, &[*b"mark"]) {
            let Some((_, subtables)) = lookup_at(gpos, lookup_idx, true) else {
                continue;
            };
            for (kind, data) in subtables {
                if kind == 4 {
                    if let Some(delta) = apply_mark_to_base(data, base_gid, mark_gid) {
                        return Some(delta);
                    }
                }
            }
        }
        None
    }

    /// Evaluates `GPOS` `mkmk` (`MarkToMark`) attachment `(base_mark_anchor - mark_anchor)`.
    pub fn mark_to_mark(
        &self,
        scripts: &[[u8; 4]],
        base_mark_gid: u16,
        mark_gid: u16,
    ) -> Option<MarkOffset> {
        let gpos = self.gpos?;
        for lookup_idx in collect_lookups(gpos, scripts, &[*b"mkmk"]) {
            let Some((_, subtables)) = lookup_at(gpos, lookup_idx, true) else {
                continue;
            };
            for (kind, data) in subtables {
                if kind == 6 {
                    if let Some(delta) = apply_mark_to_mark(data, base_mark_gid, mark_gid) {
                        return Some(delta);
                    }
                }
            }
        }
        None
    }
}

fn bundled_tables(font_index: u8) -> Option<&'static OpenTypeTables<'static>> {
    static CACHE: [OnceLock<Option<OpenTypeTables<'static>>>; 7] = [
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
    ];
    CACHE
        .get(usize::from(font_index))?
        .get_or_init(|| super::fonts::font_bytes(font_index).map(OpenTypeTables::parse))
        .as_ref()
}

fn collect_lookups(table: &[u8], scripts: &[[u8; 4]], features: &[[u8; 4]]) -> Vec<u16> {
    let Some(script_off) = u16_be(table, 4) else {
        return Vec::new();
    };
    let Some(feat_off) = u16_be(table, 6) else {
        return Vec::new();
    };
    let Some(feat_list) = sub(table, feat_off) else {
        return Vec::new();
    };
    let feat_count = usize::from(u16_be(feat_list, 0).unwrap_or(0));

    let allowed_indices =
        sub(table, script_off).and_then(|sl| collect_script_features(sl, scripts));

    let mut lookups = Vec::new();
    for i in 0..feat_count {
        if let Some(ref allowed) = allowed_indices {
            if !allowed.is_empty() && !allowed.contains(&(i as u16)) {
                continue;
            }
        }
        let rec_off = 2 + i * 6;
        let Some(tag_bytes) = feat_list.get(rec_off..rec_off + 4) else {
            break;
        };
        let Some(tag): Option<[u8; 4]> = tag_bytes.try_into().ok() else {
            continue;
        };
        if !features.contains(&tag) {
            continue;
        }
        let Some(f_sub_off) = u16_be(feat_list, rec_off + 4) else {
            continue;
        };
        let Some(f_tbl) = sub(feat_list, f_sub_off) else {
            continue;
        };
        let l_cnt = usize::from(u16_be(f_tbl, 2).unwrap_or(0));
        for k in 0..l_cnt {
            if let Some(idx) = u16_be(f_tbl, 4 + k * 2) {
                if !lookups.contains(&idx) {
                    lookups.push(idx);
                }
            }
        }
    }
    lookups.sort_unstable();
    lookups
}

fn collect_script_features(script_list: &[u8], preferred: &[[u8; 4]]) -> Option<Vec<u16>> {
    let count = usize::from(u16_be(script_list, 0)?);
    if count == 0 {
        return None;
    }
    let mut chosen = Vec::new();
    let mut dflt_features = Vec::new();
    for i in 0..count {
        let rec = 2 + i * 6;
        let tag: [u8; 4] = script_list.get(rec..rec + 4)?.try_into().ok()?;
        let off = u16_be(script_list, rec + 4)?;
        let Some(script_tbl) = sub(script_list, off) else {
            continue;
        };
        let Some(lang_off) = u16_be(script_tbl, 0).filter(|&o| o != 0) else {
            continue;
        };
        let Some(lang) = sub(script_tbl, lang_off) else {
            continue;
        };
        let f_cnt = usize::from(u16_be(lang, 4).unwrap_or(0));
        let target = if preferred.contains(&tag) {
            &mut chosen
        } else if tag == *b"DFLT" {
            &mut dflt_features
        } else {
            continue;
        };
        for j in 0..f_cnt {
            if let Some(fi) = u16_be(lang, 6 + j * 2) {
                target.push(fi);
            }
        }
    }
    if !chosen.is_empty() {
        if chosen.len() == dflt_features.len() || dflt_features.is_empty() {
            return Some(chosen);
        }
        chosen.extend(dflt_features);
        Some(chosen)
    } else if !dflt_features.is_empty() {
        Some(dflt_features)
    } else {
        None
    }
}

type LookupSubtables<'a> = (u16, Vec<(u16, &'a [u8])>);

fn lookup_at(table: &[u8], lookup_idx: u16, is_gpos: bool) -> Option<LookupSubtables<'_>> {
    let list_off = u16_be(table, 8)?;
    let list = sub(table, list_off)?;
    let count = u16_be(list, 0)?;
    if lookup_idx >= count {
        return None;
    }
    let l_off = u16_be(list, 2 + usize::from(lookup_idx) * 2)?;
    let lookup = sub(list, l_off)?;
    let lookup_type = u16_be(lookup, 0)?;
    let lookup_flag = u16_be(lookup, 2)?;
    let sub_count = usize::from(u16_be(lookup, 4)?);
    let ext_type = if is_gpos { 9 } else { 7 };
    let mut subtables = Vec::with_capacity(sub_count);
    for i in 0..sub_count {
        let Some(s_off) = u16_be(lookup, 6 + i * 2) else {
            continue;
        };
        let Some(subtable) = sub(lookup, s_off) else {
            continue;
        };
        if lookup_type == ext_type {
            if u16_be(subtable, 0) == Some(1) {
                if let (Some(inner_type), Some(inner_off)) =
                    (u16_be(subtable, 2), u32_be(subtable, 4))
                {
                    if let Ok(inner_usize) = usize::try_from(inner_off) {
                        if let Some(inner) = subtable.get(inner_usize..) {
                            subtables.push((inner_type, inner));
                        }
                    }
                }
            }
        } else {
            subtables.push((lookup_type, subtable));
        }
    }
    Some((lookup_flag, subtables))
}

fn coverage_index(cov: &[u8], gid: u16) -> Option<u16> {
    match u16_be(cov, 0)? {
        1 => {
            let count = usize::from(u16_be(cov, 2)?);
            let glyphs = cov.get(4..4 + count * 2)?;
            let mut lo = 0usize;
            let mut hi = count;
            while lo < hi {
                let mid = (lo + hi) / 2;
                let g = u16_be(glyphs, mid * 2)?;
                if g < gid {
                    lo = mid + 1;
                } else if g > gid {
                    hi = mid;
                } else {
                    return u16::try_from(mid).ok();
                }
            }
            None
        }
        2 => {
            let count = usize::from(u16_be(cov, 2)?);
            let ranges = cov.get(4..4 + count * 6)?;
            let mut lo = 0usize;
            let mut hi = count;
            while lo < hi {
                let mid = (lo + hi) / 2;
                let base = mid * 6;
                let start = u16_be(ranges, base)?;
                let end = u16_be(ranges, base + 2)?;
                if gid < start {
                    hi = mid;
                } else if gid > end {
                    lo = mid + 1;
                } else {
                    let start_cov = u16_be(ranges, base + 4)?;
                    return start_cov.checked_add(gid - start);
                }
            }
            None
        }
        _ => None,
    }
}

fn class_of(class_def: &[u8], gid: u16) -> u16 {
    match u16_be(class_def, 0) {
        Some(1) => {
            let (Some(start), Some(count)) = (u16_be(class_def, 2), u16_be(class_def, 4)) else {
                return 0;
            };
            if gid < start || gid >= start.saturating_add(count) {
                return 0;
            }
            u16_be(class_def, 6 + usize::from(gid - start) * 2).unwrap_or(0)
        }
        Some(2) => {
            let count = usize::from(u16_be(class_def, 2).unwrap_or(0));
            let Some(ranges) = class_def.get(4..4 + count * 6) else {
                return 0;
            };
            let mut lo = 0usize;
            let mut hi = count;
            while lo < hi {
                let mid = (lo + hi) / 2;
                let base = mid * 6;
                let (Some(start), Some(end), Some(cls)) = (
                    u16_be(ranges, base),
                    u16_be(ranges, base + 2),
                    u16_be(ranges, base + 4),
                ) else {
                    return 0;
                };
                if gid < start {
                    hi = mid;
                } else if gid > end {
                    lo = mid + 1;
                } else {
                    return cls;
                }
            }
            0
        }
        _ => 0,
    }
}

fn apply_single_subst(data: &[u8], gid: u16) -> Option<u16> {
    let fmt = u16_be(data, 0)?;
    let cov = sub(data, u16_be(data, 2)?)?;
    let idx = coverage_index(cov, gid)?;
    match fmt {
        1 => {
            let delta = i16_be(data, 4)?;
            Some(gid.wrapping_add_signed(delta))
        }
        2 => {
            let count = u16_be(data, 4)?;
            if idx >= count {
                return None;
            }
            u16_be(data, 6 + usize::from(idx) * 2)
        }
        _ => None,
    }
}

fn is_mark_gdef_or_hint(gdef: Option<&[u8]>, is_mark: &[bool], index: usize, gid: u16) -> bool {
    if is_mark.get(index).copied().unwrap_or(false) {
        return true;
    }
    let Some(gdef) = gdef else {
        return false;
    };
    u16_be(gdef, 4)
        .filter(|&o| o != 0)
        .and_then(|o| sub(gdef, o))
        .is_some_and(|cd| class_of(cd, gid) == 3)
}

fn apply_ligature_subst(
    data: &[u8],
    first: u16,
    gids: &[u16],
    is_mark: &[bool],
    ignore_marks: bool,
    gdef: Option<&[u8]>,
) -> Option<(u16, Vec<usize>)> {
    if u16_be(data, 0)? != 1 {
        return None;
    }
    let cov = sub(data, u16_be(data, 2)?)?;
    let set_idx = usize::from(coverage_index(cov, first)?);
    let set_count = usize::from(u16_be(data, 4)?);
    if set_idx >= set_count {
        return None;
    }
    let set_off = u16_be(data, 6 + set_idx * 2)?;
    let lig_set = sub(data, set_off)?;
    let lig_count = usize::from(u16_be(lig_set, 0)?);

    for i in 0..lig_count {
        let Some(lig_off) = u16_be(lig_set, 2 + i * 2) else {
            continue;
        };
        let Some(lig) = sub(lig_set, lig_off) else {
            continue;
        };
        let (Some(lig_gid), Some(comp_count)) = (u16_be(lig, 0), u16_be(lig, 2)) else {
            continue;
        };
        if comp_count < 2 {
            continue;
        }
        let mut consumed = Vec::with_capacity(usize::from(comp_count));
        consumed.push(0);
        let mut pos = 1usize;
        let mut matched = true;
        for c in 0..usize::from(comp_count - 1) {
            let Some(expected) = u16_be(lig, 4 + c * 2) else {
                matched = false;
                break;
            };
            while ignore_marks
                && pos < gids.len()
                && is_mark_gdef_or_hint(gdef, is_mark, pos, gids[pos])
            {
                pos += 1;
            }
            if pos >= gids.len() || gids[pos] != expected {
                matched = false;
                break;
            }
            consumed.push(pos);
            pos += 1;
        }
        if matched {
            return Some((lig_gid, consumed));
        }
    }
    None
}

fn value_record_byte_len(format: u16) -> usize {
    let mut count = 0usize;
    for bit in 0..8u16 {
        if (format & (1 << bit)) != 0 {
            count += 2;
        }
    }
    count
}

fn read_value_record(bytes: &[u8], offset: usize, format: u16) -> Option<(i16, i16, i16, i16)> {
    let need = value_record_byte_len(format);
    if offset.checked_add(need)? > bytes.len() {
        return None;
    }
    let mut cur = offset;
    let x_place = if (format & 0x0001) != 0 {
        let v = i16_be(bytes, cur)?;
        cur += 2;
        v
    } else {
        0
    };
    let y_place = if (format & 0x0002) != 0 {
        let v = i16_be(bytes, cur)?;
        cur += 2;
        v
    } else {
        0
    };
    let x_adv = if (format & 0x0004) != 0 {
        let v = i16_be(bytes, cur)?;
        cur += 2;
        v
    } else {
        0
    };
    let y_adv = if (format & 0x0008) != 0 {
        i16_be(bytes, cur)?
    } else {
        0
    };
    Some((x_place, y_place, x_adv, y_adv))
}

fn apply_pair_pos(data: &[u8], first: u16, second: u16) -> Option<PairAdjustment> {
    let fmt = u16_be(data, 0)?;
    let cov = sub(data, u16_be(data, 2)?)?;
    let cov_idx = usize::from(coverage_index(cov, first)?);
    let vf1 = u16_be(data, 4)?;
    let vf2 = u16_be(data, 6)?;
    let len1 = value_record_byte_len(vf1);
    let len2 = value_record_byte_len(vf2);

    match fmt {
        1 => {
            let count = usize::from(u16_be(data, 8)?);
            if cov_idx >= count {
                return None;
            }
            let pair_set_off = u16_be(data, 10 + cov_idx * 2)?;
            let pair_set = sub(data, pair_set_off)?;
            let pair_count = usize::from(u16_be(pair_set, 0)?);
            let stride = 2 + len1 + len2;
            let mut lo = 0usize;
            let mut hi = pair_count;
            while lo < hi {
                let mid = (lo + hi) / 2;
                let rec = 2 + mid * stride;
                let g2 = u16_be(pair_set, rec)?;
                if g2 < second {
                    lo = mid + 1;
                } else if g2 > second {
                    hi = mid;
                } else {
                    let (xp1, yp1, xa1, _) = read_value_record(pair_set, rec + 2, vf1)?;
                    let (xp2, yp2, xa2, _) = read_value_record(pair_set, rec + 2 + len1, vf2)?;
                    return Some(PairAdjustment {
                        x_placement_1: xp1,
                        y_placement_1: yp1,
                        x_advance_1: xa1,
                        x_placement_2: xp2,
                        y_placement_2: yp2,
                        x_advance_2: xa2,
                    });
                }
            }
            None
        }
        2 => {
            let class_def_1 = sub(data, u16_be(data, 8)?)?;
            let class_def_2 = sub(data, u16_be(data, 10)?)?;
            let class1_count = usize::from(u16_be(data, 12)?);
            let class2_count = usize::from(u16_be(data, 14)?);
            let c1 = usize::from(class_of(class_def_1, first));
            let c2 = usize::from(class_of(class_def_2, second));
            if c1 >= class1_count || c2 >= class2_count {
                return None;
            }
            let stride = len1 + len2;
            let rec = 16 + (c1.checked_mul(class2_count)?.checked_add(c2)?).checked_mul(stride)?;
            let (xp1, yp1, xa1, _) = read_value_record(data, rec, vf1)?;
            let (xp2, yp2, xa2, _) = read_value_record(data, rec + len1, vf2)?;
            if xp1 == 0 && yp1 == 0 && xa1 == 0 && xp2 == 0 && yp2 == 0 && xa2 == 0 {
                return None;
            }
            Some(PairAdjustment {
                x_placement_1: xp1,
                y_placement_1: yp1,
                x_advance_1: xa1,
                x_placement_2: xp2,
                y_placement_2: yp2,
                x_advance_2: xa2,
            })
        }
        _ => None,
    }
}

fn read_anchor(base_table: &[u8], offset: u16) -> Option<(i16, i16)> {
    if offset == 0 {
        return None;
    }
    let anchor = sub(base_table, offset)?;
    let fmt = u16_be(anchor, 0)?;
    if !matches!(fmt, 1..=3) {
        return None;
    }
    Some((i16_be(anchor, 2)?, i16_be(anchor, 4)?))
}

fn apply_mark_to_base(data: &[u8], base_gid: u16, mark_gid: u16) -> Option<MarkOffset> {
    if u16_be(data, 0)? != 1 {
        return None;
    }
    let mark_cov = sub(data, u16_be(data, 2)?)?;
    let base_cov = sub(data, u16_be(data, 4)?)?;
    let class_count = usize::from(u16_be(data, 6)?);
    let mark_array = sub(data, u16_be(data, 8)?)?;
    let base_array = sub(data, u16_be(data, 10)?)?;

    let mark_idx = usize::from(coverage_index(mark_cov, mark_gid)?);
    let base_idx = usize::from(coverage_index(base_cov, base_gid)?);

    let mark_count = usize::from(u16_be(mark_array, 0)?);
    let base_count = usize::from(u16_be(base_array, 0)?);
    if mark_idx >= mark_count || base_idx >= base_count {
        return None;
    }
    let mark_class = usize::from(u16_be(mark_array, 2 + mark_idx * 4)?);
    let mark_anchor_off = u16_be(mark_array, 4 + mark_idx * 4)?;
    if mark_class >= class_count {
        return None;
    }
    let (mx, my) = read_anchor(mark_array, mark_anchor_off)?;

    let base_rec_off = 2 + (base_idx.checked_mul(class_count)?.checked_add(mark_class)?) * 2;
    let base_anchor_off = u16_be(base_array, base_rec_off)?;
    let (bx, by) = read_anchor(base_array, base_anchor_off)?;

    Some(MarkOffset {
        x: bx.saturating_sub(mx),
        y: by.saturating_sub(my),
    })
}

fn apply_mark_to_mark(data: &[u8], mark1_gid: u16, mark2_gid: u16) -> Option<MarkOffset> {
    // Identical layout to MarkToBase Format 1 (mark1 = base mark array at offset 10).
    apply_mark_to_base(data, mark1_gid, mark2_gid)
}
