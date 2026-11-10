//! Single-line text editing with undo, word and grapheme navigation.

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TextSnapshot {
    value: String,
    caret: usize,
    anchor: usize,
}

/// Single-line editor. Caret/anchor are UTF-8 byte offsets, always char boundaries.
/// Each edit is one undo step; selection movement alone does not create history.
pub(super) fn word_edge(value: &str, at: usize, forward: bool, count: usize) -> usize {
    use unicode_segmentation::UnicodeSegmentation;
    let tokens = value
        .split_word_bound_indices()
        .filter(|(_, text)| !text.chars().all(char::is_whitespace));
    if forward {
        tokens
            .filter(|(start, text)| start + text.len() > at)
            .nth(count - 1)
            .map_or(value.len(), |(start, text)| {
                grapheme_edge(value, start + text.len(), true, false)
            })
    } else {
        tokens
            .rev()
            .filter(|(start, _)| *start < at)
            .nth(count - 1)
            .map_or(0, |(start, _)| grapheme_edge(value, start, false, false))
    }
}

/// Built-in compact CJK/Thai/Japanese compound dictionary for locale-aware word segmentation.
/// When contiguous CJK ideographs or Thai syllables match a known compound phrase, [`segment_words_dictionary`]
/// groups them as a single word stop instead of splitting every individual Han ideograph.
pub(super) const CJK_THAI_COMPOUNDS: &[&str] = &[
    "你好",
    "世界",
    "中文",
    "漢字",
    "汉字",
    "测试",
    "測試",
    "中华人民共和国",
    "人工智能",
    "浏览器",
    "渲染器",
    "无障碍",
    "输入法",
    "日本語",
    "未確定",
    "確定",
    "東京",
    "文字列",
    "สวัสดี",
    "ประเทศไทย",
    "ภาษาไทย",
    "คอมพิวเตอร์",
];

pub(super) fn is_han_or_thai(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    matches!(first as u32, 0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0x0E00..=0x0E7F)
}

/// Segments `value` into `(byte_offset, slice)` word tokens using UAX #29 combined with
/// dictionary-backed compound matching for unspaced scripts (Chinese/Japanese Han compounds and Thai words).
pub fn segment_words_dictionary(value: &str) -> Vec<(usize, &str)> {
    use unicode_segmentation::UnicodeSegmentation;
    let raw: Vec<(usize, &str)> = value.split_word_bound_indices().collect();
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        let (start, token) = raw[i];
        if is_han_or_thai(token) {
            let rest = &value[start..];
            if let Some(matched) = CJK_THAI_COMPOUNDS
                .iter()
                .copied()
                .filter(|w| rest.starts_with(w))
                .max_by_key(|w| w.len())
            {
                let end = start + matched.len();
                let mut j = i + 1;
                while j < raw.len() && raw[j].0 < end {
                    j += 1;
                }
                out.push((start, &value[start..end]));
                i = j;
                continue;
            }
        }
        out.push((start, token));
        i += 1;
    }
    out
}

pub(super) fn word_edge_dictionary(value: &str, at: usize, forward: bool, count: usize) -> usize {
    if value.is_ascii() {
        return word_edge(value, at, forward, count);
    }
    let tokens = segment_words_dictionary(value);
    if forward {
        tokens
            .into_iter()
            .filter(|(_, text)| !text.chars().all(char::is_whitespace))
            .filter(|(start, text)| start + text.len() > at)
            .nth(count - 1)
            .map_or(value.len(), |(start, text)| {
                grapheme_edge(value, start + text.len(), true, false)
            })
    } else {
        tokens
            .into_iter()
            .rev()
            .filter(|(_, text)| !text.chars().all(char::is_whitespace))
            .filter(|(start, _)| *start < at)
            .nth(count - 1)
            .map_or(0, |(start, _)| grapheme_edge(value, start, false, false))
    }
}

pub(super) fn grapheme_edge(value: &str, at: usize, forward: bool, strict: bool) -> usize {
    let mut cursor = unicode_segmentation::GraphemeCursor::new(at, value.len(), true);
    if !strict && cursor.is_boundary(value, 0).expect("complete UTF-8 string") {
        return at;
    }
    if forward {
        cursor
            .next_boundary(value, 0)
            .expect("complete UTF-8 string")
            .unwrap_or(value.len())
    } else {
        cursor
            .prev_boundary(value, 0)
            .expect("complete UTF-8 string")
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    multiline: bool,
    pub(super) current: TextSnapshot,
    undo: Vec<TextSnapshot>,
    redo: Vec<TextSnapshot>,
}
impl TextEdit {
    pub fn new(value: String) -> Result<Self, ModelError> {
        Self::with_multiline(value, false)
    }
    pub fn with_multiline(value: String, multiline: bool) -> Result<Self, ModelError> {
        if !multiline {
            validate_line(&value)?;
        }
        let caret = value.len();
        Ok(Self {
            multiline,
            current: TextSnapshot {
                value,
                caret,
                anchor: caret,
            },
            undo: vec![],
            redo: vec![],
        })
    }
    pub fn value(&self) -> &str {
        &self.current.value
    }
    pub fn caret(&self) -> usize {
        self.current.caret
    }
    pub fn anchor(&self) -> usize {
        self.current.anchor
    }
    pub fn selection(&self) -> Range<usize> {
        self.caret().min(self.anchor())..self.caret().max(self.anchor())
    }
    pub fn selected_text(&self) -> &str {
        &self.value()[self.selection()]
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn set_selection(&mut self, anchor: usize, caret: usize) -> Result<bool, ModelError> {
        for offset in [anchor, caret] {
            if !self.value().is_char_boundary(offset) {
                return Err(ModelError::InvalidTextBoundary(offset));
            }
        }
        let changed = self.anchor() != anchor || self.caret() != caret;
        self.current.anchor = anchor;
        self.current.caret = caret;
        Ok(changed)
    }
    pub fn select_all(&mut self) -> bool {
        self.set_selection(0, self.value().len()).unwrap()
    }
    fn move_to(&mut self, caret: usize, extend: bool) -> bool {
        self.set_selection(if extend { self.anchor() } else { caret }, caret)
            .unwrap()
    }
    pub fn home(&mut self, extend: bool) -> bool {
        self.move_to(0, extend)
    }
    pub fn end(&mut self, extend: bool) -> bool {
        self.move_to(self.value().len(), extend)
    }
    pub fn left(&mut self, extend: bool) -> bool {
        self.move_chars(-1, extend)
    }
    pub fn right(&mut self, extend: bool) -> bool {
        self.move_chars(1, extend)
    }
    /// Move by Unicode scalars; collapsing a selection consumes the first step.
    pub fn move_chars(&mut self, delta: isize, extend: bool) -> bool {
        if delta == 0 {
            return false;
        }
        let mut count = delta.unsigned_abs();
        let start = if !extend && !self.selection().is_empty() {
            count -= 1;
            if delta < 0 {
                self.selection().start
            } else {
                self.selection().end
            }
        } else {
            self.caret()
        };
        let target = if count == 0 {
            start
        } else if delta < 0 {
            self.value()[..start]
                .char_indices()
                .rev()
                .nth(count - 1)
                .map_or(0, |(i, _)| i)
        } else {
            self.value()[start..]
                .char_indices()
                .nth(count)
                .map_or(self.value().len(), |(i, _)| start + i)
        };
        self.move_to(target, extend)
    }
    /// Move by extended grapheme clusters; scalar movement remains available
    /// through move_chars/left/right for byte-oriented integrations.
    pub fn move_graphemes(&mut self, delta: isize, extend: bool) -> bool {
        if delta == 0 {
            return false;
        }
        let mut count = delta.unsigned_abs();
        let start = if !extend && !self.selection().is_empty() {
            count -= 1;
            grapheme_edge(
                self.value(),
                if delta < 0 {
                    self.selection().start
                } else {
                    self.selection().end
                },
                delta > 0,
                false,
            )
        } else {
            self.caret()
        };
        let mut target = start;
        for _ in 0..count {
            let next = grapheme_edge(self.value(), target, delta > 0, true);
            if next == target {
                break;
            }
            target = next;
        }
        self.move_to(target, extend)
    }
    /// Move by non-whitespace UAX29 word-boundary segments. Punctuation and
    /// emoji are stops too; destinations are rounded to grapheme boundaries.
    pub fn move_words(&mut self, delta: isize, extend: bool) -> bool {
        if delta == 0 {
            return false;
        }
        let mut count = delta.unsigned_abs();
        let start = if !extend && !self.selection().is_empty() {
            count -= 1;
            grapheme_edge(
                self.value(),
                if delta < 0 {
                    self.selection().start
                } else {
                    self.selection().end
                },
                delta > 0,
                false,
            )
        } else {
            self.caret()
        };
        let target = if count == 0 {
            start
        } else {
            word_edge(self.value(), start, delta > 0, count)
        };
        self.move_to(target, extend)
    }
    /// Move by dictionary-aware word segments (`segment_words_dictionary`), grouping CJK/Thai
    /// compound words while retaining UAX #29 + grapheme boundaries for all other scripts.
    pub fn move_words_dictionary(&mut self, delta: isize, extend: bool) -> bool {
        if delta == 0 {
            return false;
        }
        let mut count = delta.unsigned_abs();
        let start = if !extend && !self.selection().is_empty() {
            count -= 1;
            grapheme_edge(
                self.value(),
                if delta < 0 {
                    self.selection().start
                } else {
                    self.selection().end
                },
                delta > 0,
                false,
            )
        } else {
            self.caret()
        };
        let target = if count == 0 {
            start
        } else {
            word_edge_dictionary(self.value(), start, delta > 0, count)
        };
        self.move_to(target, extend)
    }
    pub fn backspace_word_dictionary(&mut self) -> bool {
        self.erase_word_dictionary(false)
    }
    pub fn delete_word_dictionary(&mut self) -> bool {
        self.erase_word_dictionary(true)
    }
    fn erase_word_dictionary(&mut self, forward: bool) -> bool {
        let range = if !self.selection().is_empty() {
            self.grapheme_selection()
        } else if forward {
            grapheme_edge(self.value(), self.caret(), false, false)
                ..word_edge_dictionary(self.value(), self.caret(), true, 1)
        } else {
            word_edge_dictionary(self.value(), self.caret(), false, 1)
                ..grapheme_edge(self.value(), self.caret(), true, false)
        };
        let changed = self.replace(range, "");
        let caret = grapheme_edge(self.value(), self.caret(), true, false);
        self.move_to(caret, false) || changed
    }
    /// Delete to a logical LF/CRLF line edge, preserving the line separator.
    /// A nonempty selection takes precedence and expands to grapheme boundaries.
    pub fn delete_to_line_edge(&mut self, forward: bool) -> bool {
        let range = if !self.selection().is_empty() {
            self.grapheme_selection()
        } else {
            let lo = grapheme_edge(self.value(), self.caret(), false, false);
            let hi = grapheme_edge(self.value(), self.caret(), true, false);
            let hi = if &self.value()[lo..hi] == "\r\n" {
                lo
            } else {
                hi
            };
            if forward {
                let end = self.value()[hi..]
                    .find('\n')
                    .map_or(self.value().len(), |n| {
                        let at = hi + n;
                        if self.value()[..at].ends_with('\r') {
                            at - 1
                        } else {
                            at
                        }
                    });
                lo..end.max(hi)
            } else {
                let start = self.value()[..lo].rfind('\n').map_or(0, |n| n + 1);
                start..hi
            }
        };
        let changed = self.replace(range, "");
        let caret = grapheme_edge(self.value(), self.caret(), true, false);
        self.move_to(caret, false) || changed
    }
    pub fn backspace_word(&mut self) -> bool {
        self.erase_word(false)
    }
    pub fn delete_word(&mut self) -> bool {
        self.erase_word(true)
    }
    fn erase_word(&mut self, forward: bool) -> bool {
        let range = if !self.selection().is_empty() {
            self.grapheme_selection()
        } else if forward {
            grapheme_edge(self.value(), self.caret(), false, false)
                ..word_edge(self.value(), self.caret(), true, 1)
        } else {
            word_edge(self.value(), self.caret(), false, 1)
                ..grapheme_edge(self.value(), self.caret(), true, false)
        };
        let changed = self.replace(range, "");
        let caret = grapheme_edge(self.value(), self.caret(), true, false);
        self.move_to(caret, false) || changed
    }
    /// Byte range used by grapheme-aware replacement. A collapsed caret is
    /// advanced to a valid boundary; partial selections expand to whole clusters.
    pub fn grapheme_selection(&self) -> Range<usize> {
        let range = self.selection();
        if range.is_empty() {
            let at = grapheme_edge(self.value(), range.start, true, false);
            at..at
        } else {
            grapheme_edge(self.value(), range.start, false, false)
                ..grapheme_edge(self.value(), range.end, true, false)
        }
    }
    pub fn insert_graphemes(&mut self, text: &str) -> Result<bool, ModelError> {
        if !self.multiline {
            validate_line(text)?;
        }
        let changed = self.replace(self.grapheme_selection(), text);
        // Inserted text can join a following combining mark/ZWJ sequence.
        let caret = grapheme_edge(self.value(), self.caret(), true, false);
        Ok(self.move_to(caret, false) || changed)
    }
    pub fn backspace_grapheme(&mut self) -> bool {
        self.erase_grapheme(false)
    }
    pub fn delete_grapheme(&mut self) -> bool {
        self.erase_grapheme(true)
    }
    fn erase_grapheme(&mut self, forward: bool) -> bool {
        let selected = self.selection();
        let range = if !selected.is_empty() {
            self.grapheme_selection()
        } else {
            let lo = grapheme_edge(self.value(), self.caret(), false, false);
            let hi = grapheme_edge(self.value(), self.caret(), true, false);
            if lo != hi {
                lo..hi
            } else if forward {
                lo..grapheme_edge(self.value(), hi, true, true)
            } else {
                grapheme_edge(self.value(), lo, false, true)..hi
            }
        };
        let changed = self.replace(range, "");
        let caret = grapheme_edge(self.value(), self.caret(), true, false);
        self.move_to(caret, false) || changed
    }
    fn replace(&mut self, range: Range<usize>, text: &str) -> bool {
        let caret = range.start + text.len();
        if &self.value()[range.clone()] == text {
            return self.set_selection(caret, caret).unwrap();
        }
        if self.undo.len() == 100 {
            self.undo.remove(0);
        }
        self.undo.push(self.current.clone());
        self.redo.clear();
        self.current.value.replace_range(range, text);
        self.current.caret = caret;
        self.current.anchor = caret;
        true
    }
    pub fn insert(&mut self, text: &str) -> Result<bool, ModelError> {
        if !self.multiline {
            validate_line(text)?;
        }
        Ok(self.replace(self.selection(), text))
    }
    pub fn backspace(&mut self) -> bool {
        let range = if self.selection().is_empty() {
            self.value()[..self.caret()]
                .char_indices()
                .next_back()
                .map_or(0, |(i, _)| i)..self.caret()
        } else {
            self.selection()
        };
        self.replace(range, "")
    }
    pub fn delete(&mut self) -> bool {
        let range = if self.selection().is_empty() {
            self.caret()
                ..self.caret()
                    + self.value()[self.caret()..]
                        .chars()
                        .next()
                        .map_or(0, char::len_utf8)
        } else {
            self.selection()
        };
        self.replace(range, "")
    }
    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.undo.pop() else {
            return false;
        };
        self.redo
            .push(std::mem::replace(&mut self.current, previous));
        true
    }
    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.undo.push(std::mem::replace(&mut self.current, next));
        true
    }
    /// Replace externally supplied text and reset local edit history.
    pub fn reset(&mut self, value: String) -> Result<bool, ModelError> {
        if !self.multiline {
            validate_line(&value)?;
        }
        let changed =
            self.value() != value || self.caret() != value.len() || self.anchor() != value.len();
        *self = Self::new(value)?;
        Ok(changed)
    }
}
pub(super) fn validate_line(value: &str) -> Result<(), ModelError> {
    if value.contains(['\r', '\n']) {
        Err(ModelError::MultilineText)
    } else {
        Ok(())
    }
}
