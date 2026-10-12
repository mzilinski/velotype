//! Text ranges for Typora's selection commands: the word, the sentence and
//! the styled scope around the caret. Offsets are byte offsets into the
//! text they are computed on.

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

use gpui::{Context, EntityInputHandler as _, Window};

use super::Block;
use crate::components::{
    DeleteLine, DeleteStyledScope, DeleteWord, InlineFragment, SelectBlock, SelectLine,
    SelectStyledScope, SelectWord, UndoCaptureKind,
};

/// `offset` clamped to the text and moved back onto a char boundary.
fn char_floor(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

/// The word at `offset` (or the one ending right before it). `None` when the
/// caret touches no word.
pub(crate) fn word_range(text: &str, offset: usize) -> Option<Range<usize>> {
    let offset = char_floor(text, offset);
    let is_word = |segment: &str| segment.chars().any(char::is_alphanumeric);
    let mut previous = None;
    for (start, segment) in text.split_word_bound_indices() {
        let end = start + segment.len();
        if !is_word(segment) {
            if start >= offset {
                break;
            }
            continue;
        }
        if start <= offset && offset < end {
            return Some(start..end);
        }
        if end == offset {
            previous = Some(start..end);
        }
        if start > offset {
            break;
        }
    }
    previous
}

/// The sentence around `offset`: it ends after `.`, `!`, `?` (or their CJK
/// forms) followed by whitespace, or at a line break. Surrounding
/// whitespace is not included.
pub(crate) fn sentence_range(text: &str, offset: usize) -> Range<usize> {
    let offset = char_floor(text, offset);
    let is_terminator = |ch: char| matches!(ch, '.' | '!' | '?' | '。' | '！' | '？');
    let mut boundaries = vec![0];
    let mut chars = text.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        let after = index + ch.len_utf8();
        if ch == '\n' {
            boundaries.push(after);
        } else if is_terminator(ch) {
            let cjk = !ch.is_ascii();
            let followed_by_space = chars.peek().is_none_or(|(_, next)| next.is_whitespace());
            if cjk || followed_by_space {
                boundaries.push(after);
            }
        }
    }
    boundaries.push(text.len());
    // A caret right after a sentence end belongs to that sentence; one at
    // the start of a line belongs to the line.
    let at_line_start = offset == 0 || text[..offset].ends_with('\n');
    let start = boundaries
        .iter()
        .copied()
        .filter(|boundary| *boundary < offset || (at_line_start && *boundary == offset))
        .max()
        .unwrap_or(0);
    let end = boundaries
        .iter()
        .copied()
        .filter(|boundary| *boundary >= offset && *boundary > start)
        .min()
        .unwrap_or(text.len());
    let slice = &text[start..end];
    let lead = slice.len() - slice.trim_start().len();
    let trail = slice.len() - slice.trim_end().len();
    start + lead..(end - trail).max(start + lead)
}

/// The run of fragments around `offset` that share its inline formatting
/// (bold, link, code, ...). `None` for plain text.
pub(crate) fn styled_scope_range(
    fragments: &[InlineFragment],
    offset: usize,
) -> Option<Range<usize>> {
    let mut ranges = Vec::with_capacity(fragments.len());
    let mut position = 0;
    for fragment in fragments {
        ranges.push(position..position + fragment.text.len());
        position += fragment.text.len();
    }
    let is_plain = |fragment: &InlineFragment| {
        fragment.style == Default::default()
            && fragment.link.is_none()
            && fragment.math.is_none()
            && fragment.html_style.is_none()
            && fragment.footnote.is_none()
    };
    // Prefer the fragment the caret is in, else the one ending at it.
    let index = ranges
        .iter()
        .position(|range| range.start <= offset && offset < range.end)
        .or_else(|| ranges.iter().position(|range| range.end == offset))?;
    if is_plain(&fragments[index]) {
        return None;
    }
    let same = |other: &InlineFragment| {
        let current = &fragments[index];
        other.style == current.style && other.link == current.link && other.math == current.math
    };
    let mut first = index;
    while first > 0 && same(&fragments[first - 1]) {
        first -= 1;
    }
    let mut last = index;
    while last + 1 < fragments.len() && same(&fragments[last + 1]) {
        last += 1;
    }
    Some(ranges[first].start..ranges[last].end)
}

impl Block {
    fn word_range_at_caret(&self) -> Option<Range<usize>> {
        word_range(self.display_text(), self.cursor_offset())
    }

    fn sentence_range_at_caret(&self) -> Range<usize> {
        sentence_range(self.display_text(), self.cursor_offset())
    }

    /// The formatted run around the caret, or the word when the text there
    /// is plain (or the block edits raw text).
    fn styled_scope_at_caret(&self) -> Option<Range<usize>> {
        if self.uses_raw_text_editing() {
            return self.word_range_at_caret();
        }
        let caret = self.cursor_offset();
        let clean_caret = self.current_to_clean_range(caret..caret).start;
        styled_scope_range(&self.record.title.fragments, clean_caret)
            .map(|range| self.clean_to_current_range(range))
            .or_else(|| self.word_range_at_caret())
    }

    fn select_range_in_block(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        self.move_to(range.start, cx);
        self.select_to(range.end, cx);
    }

    fn delete_range_in_block(
        &mut self,
        range: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(range) = range.filter(|range| !range.is_empty()) else {
            return;
        };
        if self.read_only {
            return;
        }
        self.select_range_in_block(range, cx);
        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        self.replace_text_in_range(None, "", window, cx);
    }

    pub(crate) fn on_select_word(
        &mut self,
        _: &SelectWord,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(range) = self.word_range_at_caret() {
            self.select_range_in_block(range, cx);
        }
    }

    pub(crate) fn on_select_line(
        &mut self,
        _: &SelectLine,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.sentence_range_at_caret();
        self.select_range_in_block(range, cx);
    }

    pub(crate) fn on_select_styled_scope(
        &mut self,
        _: &SelectStyledScope,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(range) = self.styled_scope_at_caret() {
            self.select_range_in_block(range, cx);
        }
    }

    pub(crate) fn on_select_block(
        &mut self,
        _: &SelectBlock,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let len = self.visible_len();
        self.select_range_in_block(0..len, cx);
    }

    pub(crate) fn on_delete_word(
        &mut self,
        _: &DeleteWord,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.word_range_at_caret();
        self.delete_range_in_block(range, window, cx);
    }

    pub(crate) fn on_delete_line(
        &mut self,
        _: &DeleteLine,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = Some(self.sentence_range_at_caret());
        self.delete_range_in_block(range, window, cx);
    }

    pub(crate) fn on_delete_styled_scope(
        &mut self,
        _: &DeleteStyledScope,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.styled_scope_at_caret();
        self.delete_range_in_block(range, window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::InlineTextTree;

    #[test]
    fn words_around_the_caret() {
        let text = "Hallo schöne Welt!";
        assert_eq!(word_range(text, 0), Some(0..5));
        assert_eq!(word_range(text, 3), Some(0..5));
        // Right after a word still selects it.
        assert_eq!(word_range(text, 5), Some(0..5));
        assert_eq!(word_range(text, 8), Some(6..13));
        assert_eq!(&text[word_range(text, 15).unwrap()], "Welt");
        assert_eq!(word_range("  ", 1), None);
    }

    #[test]
    fn sentences_split_at_terminators_and_lines() {
        let text = "First one. Second, longer one? Third\nNext line.";
        let pick = |offset| &text[sentence_range(text, offset)];
        assert_eq!(pick(2), "First one.");
        assert_eq!(pick(15), "Second, longer one?");
        assert_eq!(pick(33), "Third");
        assert_eq!(pick(40), "Next line.");
        // Right after the full stop, at the end and at a line start.
        assert_eq!(pick(10), "First one.");
        assert_eq!(pick(text.len()), "Next line.");
        assert_eq!(pick(37), "Next line.");
        // A dot inside a word (version, URL) is no sentence end.
        assert_eq!(
            &"v1.2 is out. Yes"[sentence_range("v1.2 is out. Yes", 1)],
            "v1.2 is out."
        );
        assert_eq!(
            &"中文。第二句"[sentence_range("中文。第二句", 12)],
            "第二句"
        );
    }

    #[test]
    fn styled_scope_covers_the_formatted_run() {
        let tree = InlineTextTree::from_markdown("plain **bold text** and [a link](x) end");
        let clean = tree.visible_text();
        let bold = styled_scope_range(&tree.fragments, clean.find("text").unwrap()).unwrap();
        assert_eq!(&clean[bold], "bold text");
        let link = styled_scope_range(&tree.fragments, clean.find("link").unwrap()).unwrap();
        assert_eq!(&clean[link], "a link");
        assert_eq!(styled_scope_range(&tree.fragments, 1), None);
    }
}
