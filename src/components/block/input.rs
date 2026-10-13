//! GPUI [`EntityInputHandler`] implementation for Block.
//!
//! GPUI supplies UTF-16 code-unit ranges; block selections and marked ranges
//! use UTF-8 byte offsets in the current display text. Code-language focus
//! routes input to a separate UTF-8 selection, while cross-block replacement
//! is handed to the editor.

use std::ops::Range;

use gpui::*;

use super::Block;
use super::element;
use crate::components::{BlockEvent, UndoCaptureKind};

impl EntityInputHandler for Block {
    /// Returns the requested display or code-language text and its normalized UTF-16 range.
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        if self.code_language_focus_handle.is_focused(_window) {
            let range = self.code_language_range_from_utf16(&range_utf16);
            actual_range.replace(self.code_language_range_to_utf16(&range));
            return Some(self.code_language_text()[range].to_string());
        }

        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.display_text()[range].to_string())
    }

    /// Reports the focused field's selection in UTF-16 code units.
    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        if self.code_language_focus_handle.is_focused(_window) {
            return Some(UTF16Selection {
                range: self.code_language_range_to_utf16(&self.code_language_selected_range),
                reversed: self.code_language_selection_reversed,
            });
        }

        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    /// Reports the focused field's IME composition range in UTF-16 code units.
    fn marked_text_range(
        &self,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        if self.code_language_focus_handle.is_focused(window) {
            return self
                .code_language_marked_range
                .as_ref()
                .map(|range| self.code_language_range_to_utf16(range));
        }

        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    /// Clears the focused field's composition without changing its text.
    fn unmark_text(&mut self, window: &mut Window, _cx: &mut Context<Self>) {
        if self.code_language_focus_handle.is_focused(window) {
            self.code_language_marked_range = None;
            return;
        }

        self.marked_range = None;
    }

    /// Replaces an explicit UTF-16 range, or the marked/selected range when absent.
    ///
    /// Cross-block selections are delegated to the editor; code-language edits
    /// use that field's independent text and selection.
    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.code_language_focus_handle.is_focused(_window) {
            let visible_range = range_utf16
                .as_ref()
                .map(|range| self.code_language_range_from_utf16(range))
                .or(self.code_language_marked_range.clone())
                .unwrap_or(self.code_language_selected_range.clone());
            self.replace_code_language_text_in_range(visible_range, new_text, None, false, cx);
            return;
        }

        if self.editor_selection_range.is_some() {
            cx.emit(BlockEvent::RequestReplaceCrossBlockSelection {
                text: new_text.to_string(),
                selected_range_relative: None,
                mark_inserted_text: false,
                undo_kind: UndoCaptureKind::CoalescibleText,
            });
            return;
        }

        let visible_range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        let writing = crate::config::EditorSettings::writing(cx);
        let smart = super::smart_punctuation::SmartOptions {
            quotes: writing.smart_quotes,
            dashes: writing.smart_dashes,
            auto_pair: writing.auto_pair,
            style: super::smart_punctuation::QuoteStyle::for_language(
                cx.try_global::<crate::i18n::I18nManager>()
                    .map_or("en", |i18n| i18n.current_language_id()),
            ),
        };
        match self.smart_punctuation_edit(new_text, &visible_range, smart) {
            Some(super::smart_punctuation::SmartEdit::SkipOver(len)) => {
                self.move_to(visible_range.end + len, cx);
                return;
            }
            Some(super::smart_punctuation::SmartEdit::Replace {
                range,
                text,
                selected,
            }) => {
                self.prepare_undo_capture(UndoCaptureKind::CoalescibleText, cx);
                self.replace_text_in_visible_range(range, &text, Some(selected), false, cx);
                return;
            }
            None => {}
        }
        if writing.auto_pair {
            match self.auto_pair_edit(new_text, &visible_range) {
                Some(AutoPairEdit::SkipOver) => {
                    self.move_to(visible_range.end + new_text.len(), cx);
                    return;
                }
                Some(AutoPairEdit::Insert { text, selected }) => {
                    self.prepare_undo_capture(UndoCaptureKind::CoalescibleText, cx);
                    self.replace_text_in_visible_range(
                        visible_range,
                        &text,
                        Some(selected),
                        false,
                        cx,
                    );
                    return;
                }
                None => {}
            }
        }
        self.prepare_undo_capture(UndoCaptureKind::CoalescibleText, cx);
        self.replace_text_in_visible_range(visible_range, new_text, None, false, cx);
    }

    /// Replaces text for IME composition and selects within the inserted text.
    ///
    /// Both supplied ranges use UTF-16 code units; the new selection is relative
    /// to the raw `new_text`. For code-language input, its endpoints are mapped
    /// through newline flattening before applying the normalized insertion.
    /// A nonempty insertion remains marked for the focused field.
    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.code_language_focus_handle.is_focused(_window) {
            let visible_range = range_utf16
                .as_ref()
                .map(|range| self.code_language_range_from_utf16(range))
                .or(self.code_language_marked_range.clone())
                .unwrap_or(self.code_language_selected_range.clone());
            let selected_range_relative = new_selected_range_utf16.as_ref().map(|range_utf16| {
                let raw = crate::text_offsets::utf16_range_to_utf8(new_text, range_utf16);
                let normalized_offset = |offset: usize| {
                    new_text[..offset]
                        .replace("\r\n", " ")
                        .replace(['\r', '\n'], " ")
                        .len()
                };
                normalized_offset(raw.start)..normalized_offset(raw.end)
            });

            self.replace_code_language_text_in_range(
                visible_range,
                new_text,
                selected_range_relative,
                !new_text.is_empty(),
                cx,
            );
            return;
        }

        if self.editor_selection_range.is_some() {
            let selected_range_relative = new_selected_range_utf16
                .as_ref()
                .map(|range_utf16| Self::utf16_range_to_utf8_in(new_text, range_utf16))
                .map(|relative| relative.start..relative.end);
            cx.emit(BlockEvent::RequestReplaceCrossBlockSelection {
                text: new_text.to_string(),
                selected_range_relative,
                mark_inserted_text: !new_text.is_empty(),
                undo_kind: UndoCaptureKind::CoalescibleText,
            });
            return;
        }

        self.prepare_undo_capture(UndoCaptureKind::CoalescibleText, cx);
        let visible_range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        let selected_range_relative = new_selected_range_utf16
            .as_ref()
            .map(|range_utf16| Self::utf16_range_to_utf8_in(new_text, range_utf16))
            .map(|relative| relative.start..relative.end);

        self.replace_text_in_visible_range(
            visible_range,
            new_text,
            selected_range_relative,
            !new_text.is_empty(),
            cx,
        );
    }

    /// Resolves a UTF-16 range to the focused field's last laid-out bounds.
    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        if self.code_language_focus_handle.is_focused(_window) {
            let line = self.code_language_last_layout.as_ref()?;
            let range = self.code_language_range_from_utf16(&range_utf16);
            let start_x = line.x_for_index(range.start);
            let end_x = line.x_for_index(range.end);
            return Some(Bounds::from_corners(
                point(bounds.left() + start_x, bounds.top()),
                point(bounds.left() + end_x, bounds.bottom()),
            ));
        }

        let lines = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        let line_height = self.last_line_height;
        let text = self.display_text();
        element::range_bounds(lines, bounds, line_height, text, range, self.text_align())
    }

    /// Hit-tests the focused field and returns a UTF-16 code-unit offset.
    fn character_index_for_point(
        &mut self,
        pt: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        if self.code_language_focus_handle.is_focused(_window) {
            let index = self.code_language_index_for_mouse_position(pt);
            return Some(Self::utf8_to_utf16_in(self.code_language_text(), index));
        }

        let bounds = self.last_bounds?;
        let lines = self.last_layout.as_ref()?;
        let text = self.display_text();
        let ranges = element::hard_line_ranges(text);
        let relative = Point {
            x: pt.x - bounds.left(),
            y: pt.y - bounds.top(),
        };
        let (line_idx, y_in_line) =
            element::wrapped_line_for_y(lines, self.last_line_height, relative.y)?;
        let (Some(layout), Some(range)) = (lines.get(line_idx), ranges.get(line_idx)) else {
            return None;
        };
        let origin_x = element::aligned_line_left(layout, bounds, self.text_align());
        let utf8_offset_in_line = match layout
            .closest_index_for_position(point(pt.x - origin_x, y_in_line), self.last_line_height)
        {
            Ok(idx) | Err(idx) => idx,
        };
        let utf8_index = crate::text_offsets::floor_char_boundary(
            text,
            range.start + utf8_offset_in_line.min(range.len()),
        );
        Some(Self::utf8_to_utf16_in(text, utf8_index))
    }
}

/// Result of Typora-style auto-pairing for one typed character.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AutoPairEdit {
    /// Replace the target range with `text`, selecting `selected` (relative
    /// to the start of `text`) afterwards.
    Insert {
        text: String,
        selected: Range<usize>,
    },
    /// The typed closer already follows the caret: step over it.
    SkipOver,
}

/// Closing partner for brackets and quotes; backticks pair only in raw text,
/// where they cannot open an inline code span mid-typing.
fn auto_pair_closer(open: char, raw: bool) -> Option<char> {
    match open {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        '"' => Some('"'),
        '\'' => Some('\''),
        '`' if raw => Some('`'),
        _ => None,
    }
}

impl Block {
    /// Decides how a single typed character interacts with auto-pairing:
    /// wrap a selection, insert an opener with its closer, or step over a
    /// closer that is already there. `None` means a plain insertion.
    pub(crate) fn auto_pair_edit(&self, typed: &str, range: &Range<usize>) -> Option<AutoPairEdit> {
        let mut chars = typed.chars();
        let ch = chars.next()?;
        if chars.next().is_some() || self.marked_range.is_some() {
            return None;
        }
        let raw = self.uses_raw_text_editing();
        let code = self.kind().is_code_block();
        let text = self.display_text();
        if range.end > text.len() {
            return None;
        }

        if !range.is_empty() {
            let close = auto_pair_closer(ch, true)
                .or_else(|| (!code && matches!(ch, '*' | '_' | '~' | '=')).then_some(ch))?;
            let inner = &text[range.clone()];
            if inner.contains('\n') && !raw {
                return None;
            }
            return Some(AutoPairEdit::Insert {
                text: format!("{ch}{inner}{close}"),
                selected: ch.len_utf8()..ch.len_utf8() + inner.len(),
            });
        }

        let cursor = range.start;
        let next = text[cursor..].chars().next();
        let previous = text[..cursor].chars().next_back();
        if matches!(ch, ')' | ']' | '}' | '"' | '\'' | '`') && next == Some(ch) {
            return Some(AutoPairEdit::SkipOver);
        }
        let close = auto_pair_closer(ch, raw)?;
        let next_allows_pair =
            next.is_none_or(|next| next.is_whitespace() || ")]},.;:!?".contains(next));
        // Don't pair the apostrophe in "don't" or quotes glued to a word.
        let quote_after_word =
            matches!(ch, '"' | '\'' | '`') && previous.is_some_and(char::is_alphanumeric);
        if !next_allows_pair || quote_after_word {
            return None;
        }
        Some(AutoPairEdit::Insert {
            text: format!("{ch}{close}"),
            selected: ch.len_utf8()..ch.len_utf8(),
        })
    }

    /// Range covering an empty auto-pair around the caret (`(|)`), which
    /// Backspace removes as a whole.
    pub(crate) fn empty_pair_around_cursor(&self) -> Option<Range<usize>> {
        if !self.selected_range.is_empty() || self.marked_range.is_some() {
            return None;
        }
        let cursor = self.selected_range.start;
        let text = self.display_text();
        let open = text.get(..cursor)?.chars().next_back()?;
        let close = auto_pair_closer(open, true)?;
        text.get(cursor..)?
            .starts_with(close)
            .then(|| cursor - open.len_utf8()..cursor + close.len_utf8())
    }
}
