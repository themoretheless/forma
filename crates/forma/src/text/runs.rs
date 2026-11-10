//! Layout of one paragraph made of styled runs: every run keeps its own face, the paragraph
//! wraps as a whole. Words may span runs (`bo` + `ld` with no space between is one word), so a
//! break never falls inside a word unless the word alone is wider than the line.

use super::{FontFamily, measure_text_in};
use std::ops::Range;

/// A placed piece of one run on one visual line. Adjacent pieces of the same run on the same line
/// are merged, so a single-style line is one fragment.
#[derive(Clone, Debug, PartialEq)]
pub struct RunFragment {
    /// Index of the run in the input slice.
    pub run: usize,
    /// UTF-8 byte range inside that run's text.
    pub range: Range<usize>,
    /// Visual line, from 0.
    pub line: usize,
    /// Left edge in logical pixels from the paragraph's left edge.
    pub x: f32,
    /// Advance width in logical pixels, measured with the run's face.
    pub width: f32,
}

/// Result of [`layout_runs`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunLayout {
    pub fragments: Vec<RunFragment>,
    /// Number of visual lines (at least 1).
    pub lines: usize,
    /// Widest line in logical pixels.
    pub width: f32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Word,
    Space,
    Newline,
}

fn class(ch: char) -> Class {
    match ch {
        '\n' => Class::Newline,
        ' ' | '\t' => Class::Space,
        _ => Class::Word,
    }
}

struct Atom {
    run: usize,
    range: Range<usize>,
    class: Class,
    width: f32,
}

struct Placer<'a> {
    runs: &'a [(FontFamily, &'a str)],
    font_size: f32,
    max_width: Option<f32>,
    out: RunLayout,
    line: usize,
    x: f32,
}

impl Placer<'_> {
    fn place(&mut self, run: usize, range: Range<usize>, width: f32) {
        if let Some(last) = self.out.fragments.last_mut() {
            if last.run == run && last.line == self.line && last.range.end == range.start {
                last.range.end = range.end;
                last.width += width;
                self.x += width;
                self.out.width = self.out.width.max(self.x);
                return;
            }
        }
        self.out.fragments.push(RunFragment {
            run,
            range,
            line: self.line,
            x: self.x,
            width,
        });
        self.x += width;
        self.out.width = self.out.width.max(self.x);
    }
    fn new_line(&mut self) {
        self.line += 1;
        self.x = 0.;
    }
    fn fits(&self, width: f32) -> bool {
        self.max_width.is_none_or(|max| self.x + width <= max)
    }
    /// A word wider than the whole line: break between characters.
    fn place_by_chars(&mut self, atom: &Atom) {
        let (family, text) = self.runs[atom.run];
        for (offset, ch) in text[atom.range.clone()].char_indices() {
            let start = atom.range.start + offset;
            let end = start + ch.len_utf8();
            let width = measure_text_in(family, &text[start..end], self.font_size);
            if self.x > 0. && !self.fits(width) {
                self.new_line();
            }
            self.place(atom.run, start..end, width);
        }
    }
    fn place_word(&mut self, word: &[Atom], spaces: &[Atom]) {
        let width: f32 = word.iter().map(|a| a.width).sum();
        let gap: f32 = spaces.iter().map(|a| a.width).sum();
        if self.x > 0. && !self.fits(gap + width) {
            self.new_line();
        } else if self.x > 0. {
            for space in spaces {
                self.place(space.run, space.range.clone(), space.width);
            }
        }
        if self.max_width.is_some_and(|max| width > max) {
            for atom in word {
                self.place_by_chars(atom);
            }
        } else {
            for atom in word {
                self.place(atom.run, atom.range.clone(), atom.width);
            }
        }
    }
}

/// Lay out `runs` (face and text of each run) as one paragraph. `max_width` enables soft wrapping
/// at spaces; `\n` always starts a new line. Spaces at a soft break are dropped, spaces at the
/// start of a hard line are kept. Widths are exactly [`measure_text_in`] of each fragment.
pub fn layout_runs(
    runs: &[(FontFamily, &str)],
    font_size: f32,
    max_width: Option<f32>,
) -> RunLayout {
    let mut atoms = Vec::new();
    for (run, &(_, text)) in runs.iter().enumerate() {
        let mut start = 0;
        let mut current: Option<Class> = None;
        for (i, ch) in text.char_indices() {
            let c = class(ch);
            // Each newline is its own atom; other classes group.
            if let Some(k) = current.filter(|&k| k != c || c == Class::Newline) {
                atoms.push((run, start..i, k));
                start = i;
            }
            current = Some(c);
        }
        if let Some(k) = current {
            atoms.push((run, start..text.len(), k));
        }
    }
    let atoms: Vec<Atom> = atoms
        .into_iter()
        .map(|(run, range, class)| {
            let (family, text) = runs[run];
            let width = if class == Class::Newline {
                0.
            } else {
                measure_text_in(family, &text[range.clone()], font_size)
            };
            Atom {
                run,
                range,
                class,
                width,
            }
        })
        .collect();
    let mut placer = Placer {
        runs,
        font_size,
        max_width: max_width.filter(|w| w.is_finite() && *w > 0.),
        out: RunLayout::default(),
        line: 0,
        x: 0.,
    };
    let mut i = 0;
    let mut spaces: Vec<Atom> = Vec::new();
    let mut line_start = true;
    while i < atoms.len() {
        match atoms[i].class {
            Class::Newline => {
                spaces.clear();
                placer.new_line();
                line_start = true;
                i += 1;
            }
            Class::Space => {
                let atom = &atoms[i];
                if line_start {
                    // Indentation after a hard break or at the very start is content.
                    placer.place(atom.run, atom.range.clone(), atom.width);
                } else {
                    spaces.push(Atom {
                        run: atom.run,
                        range: atom.range.clone(),
                        class: atom.class,
                        width: atom.width,
                    });
                }
                i += 1;
            }
            Class::Word => {
                let end = atoms[i..]
                    .iter()
                    .position(|a| a.class != Class::Word)
                    .map_or(atoms.len(), |p| i + p);
                placer.place_word(&atoms[i..end], &spaces);
                spaces.clear();
                line_start = false;
                i = end;
            }
        }
    }
    placer.out.lines = placer.line + 1;
    placer.out
}
