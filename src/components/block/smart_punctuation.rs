//! Edit > Smart Punctuation: typographic quotes and dashes while typing.
//! `"` and `'` become opening or closing curly quotes (German „…“ ‚…‘ when
//! the UI is German), `--` becomes an en dash and a third `-` an em dash.
//! Code, math, HTML, front matter and source mode keep what is typed.

use std::ops::Range;

use super::{Block, BlockKind};

/// Opening and closing quote characters of a language.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct QuoteStyle {
    double: (char, char),
    single: (char, char),
}

impl QuoteStyle {
    pub(crate) const ENGLISH: Self = Self {
        double: ('\u{201C}', '\u{201D}'),
        single: ('\u{2018}', '\u{2019}'),
    };
    pub(crate) const GERMAN: Self = Self {
        double: ('\u{201E}', '\u{201C}'),
        single: ('\u{201A}', '\u{2018}'),
    };

    /// The style for a UI language id such as `de-DE` or `en`.
    pub(crate) fn for_language(language_id: &str) -> Self {
        if language_id.starts_with("de") {
            Self::GERMAN
        } else {
            Self::ENGLISH
        }
    }
}

const APOSTROPHE: char = '\u{2019}';
const EN_DASH: char = '\u{2013}';
const EM_DASH: char = '\u{2014}';

/// What typing one character turns into.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SmartEdit {
    /// Replace `range` with `text` and put the selection at `selected`
    /// (relative to the inserted text).
    Replace {
        range: Range<usize>,
        text: String,
        selected: Range<usize>,
    },
    /// The typed closing quote already follows the caret: step over it.
    SkipOver(usize),
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SmartOptions {
    pub(crate) quotes: bool,
    pub(crate) dashes: bool,
    pub(crate) auto_pair: bool,
    pub(crate) style: QuoteStyle,
}

/// A quote typed after these (or at the start) opens.
fn opens_after(previous: Option<char>) -> bool {
    previous.is_none_or(|ch| {
        ch.is_whitespace() || "([{<-/\u{2013}\u{2014}\u{201C}\u{201E}\u{2018}\u{201A}".contains(ch)
    })
}

/// Whether the caret sits inside an inline code span on its line (an odd
/// number of backticks before it).
fn inside_inline_code(line_before: &str) -> bool {
    line_before.matches('`').count() % 2 == 1
}

/// The smart edit for typing `typed` over `range` in `text`, if any.
pub(crate) fn smart_edit(
    text: &str,
    range: &Range<usize>,
    typed: &str,
    options: SmartOptions,
) -> Option<SmartEdit> {
    let mut chars = typed.chars();
    let ch = chars.next()?;
    if chars.next().is_some() || range.end > text.len() {
        return None;
    }
    let line_start = text[..range.start].rfind('\n').map_or(0, |index| index + 1);
    let line_before = &text[line_start..range.start];
    if inside_inline_code(line_before) {
        return None;
    }
    let previous = line_before.chars().next_back();
    match ch {
        '"' | '\'' if options.quotes => {
            let (open, close) = if ch == '"' {
                options.style.double
            } else {
                options.style.single
            };
            if !range.is_empty() {
                let inner = &text[range.clone()];
                if inner.contains('\n') {
                    return None;
                }
                return Some(SmartEdit::Replace {
                    range: range.clone(),
                    text: format!("{open}{inner}{close}"),
                    selected: open.len_utf8()..open.len_utf8() + inner.len(),
                });
            }
            let next = text[range.end..].chars().next();
            // In German the closing single quote differs from the
            // apostrophe; it closes only a quote opened on this line.
            let quote_open = close != APOSTROPHE
                && line_before.rfind(open).is_some_and(|opened| {
                    line_before
                        .rfind(close)
                        .is_none_or(|closed| closed < opened)
                });
            if ch == '\'' && previous.is_some_and(char::is_alphanumeric) && !quote_open {
                // don't, Peter's: the apostrophe, unless it closes a pair.
                if options.auto_pair && next == Some(close) && close == APOSTROPHE {
                    return Some(SmartEdit::SkipOver(close.len_utf8()));
                }
                return Some(insert(range, APOSTROPHE.to_string(), APOSTROPHE.len_utf8()));
            }
            if options.auto_pair && next == Some(close) && !opens_after(previous) {
                return Some(SmartEdit::SkipOver(close.len_utf8()));
            }
            if !opens_after(previous) {
                return Some(insert(range, close.to_string(), close.len_utf8()));
            }
            let next_allows_pair =
                next.is_none_or(|next| next.is_whitespace() || ")]},.;:!?".contains(next));
            if options.auto_pair && next_allows_pair {
                return Some(insert(range, format!("{open}{close}"), open.len_utf8()));
            }
            Some(insert(range, open.to_string(), open.len_utf8()))
        }
        '-' if options.dashes && range.is_empty() => {
            let start = range.start;
            match previous? {
                EN_DASH => Some(SmartEdit::Replace {
                    range: start - EN_DASH.len_utf8()..start,
                    text: EM_DASH.to_string(),
                    selected: EM_DASH.len_utf8()..EM_DASH.len_utf8(),
                }),
                '-' => {
                    let before = &line_before[..line_before.len() - 1];
                    // Leave rules (`---`), arrows and HTML comments alone,
                    // and need some text before the dashes.
                    let has_text = before
                        .chars()
                        .any(|ch| !ch.is_whitespace() && !"-|>".contains(ch));
                    if !has_text || before.ends_with('-') || before.contains("<!") {
                        return None;
                    }
                    Some(SmartEdit::Replace {
                        range: start - 1..start,
                        text: EN_DASH.to_string(),
                        selected: EN_DASH.len_utf8()..EN_DASH.len_utf8(),
                    })
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn insert(range: &Range<usize>, text: String, caret: usize) -> SmartEdit {
    SmartEdit::Replace {
        range: range.clone(),
        text,
        selected: caret..caret,
    }
}

impl Block {
    /// Smart punctuation for one typed character in this block, honouring
    /// the block kind and edit mode.
    pub(crate) fn smart_punctuation_edit(
        &self,
        typed: &str,
        range: &Range<usize>,
        options: SmartOptions,
    ) -> Option<SmartEdit> {
        if (!options.quotes && !options.dashes)
            || self.marked_range.is_some()
            || self.uses_raw_text_editing()
        {
            return None;
        }
        let prose = matches!(
            self.kind(),
            BlockKind::Paragraph
                | BlockKind::Heading { .. }
                | BlockKind::BulletedListItem
                | BlockKind::TaskListItem { .. }
                | BlockKind::NumberedListItem
                | BlockKind::Quote
                | BlockKind::Callout(_)
                | BlockKind::FootnoteDefinition
        );
        if !prose {
            return None;
        }
        smart_edit(self.display_text(), range, typed, options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(style: QuoteStyle) -> SmartOptions {
        SmartOptions {
            quotes: true,
            dashes: true,
            auto_pair: false,
            style,
        }
    }

    /// Types `typed` at the end of `text` and returns the new text.
    fn type_at_end(text: &str, typed: char, options: SmartOptions) -> String {
        let end = text.len();
        match smart_edit(text, &(end..end), &typed.to_string(), options) {
            Some(SmartEdit::Replace {
                range, text: new, ..
            }) => {
                format!("{}{new}{}", &text[..range.start], &text[range.end..])
            }
            Some(SmartEdit::SkipOver(_)) => text.to_string(),
            None => format!("{text}{typed}"),
        }
    }

    fn type_all(input: &str, options: SmartOptions) -> String {
        input
            .chars()
            .fold(String::new(), |text, ch| type_at_end(&text, ch, options))
    }

    #[test]
    fn curly_quotes_by_language() {
        let en = options(QuoteStyle::ENGLISH);
        assert_eq!(
            type_all("\"Hi,\" she said", en),
            "\u{201C}Hi,\u{201D} she said"
        );
        assert_eq!(type_all("'quoted'", en), "\u{2018}quoted\u{2019}");
        let de = options(QuoteStyle::GERMAN);
        assert_eq!(
            type_all("Er sagt \"Hallo\".", de),
            "Er sagt \u{201E}Hallo\u{201C}."
        );
        assert_eq!(type_all("'so'", de), "\u{201A}so\u{2018}");
    }

    #[test]
    fn apostrophes_stay_apostrophes() {
        let de = options(QuoteStyle::GERMAN);
        assert_eq!(type_all("geht's", de), "geht\u{2019}s");
        assert_eq!(
            type_all("don't", options(QuoteStyle::ENGLISH)),
            "don\u{2019}t"
        );
    }

    #[test]
    fn dashes_need_text_and_skip_rules() {
        let en = options(QuoteStyle::ENGLISH);
        assert_eq!(type_all("a -- b", en), "a \u{2013} b");
        assert_eq!(type_all("a --- b", en), "a \u{2014} b");
        assert_eq!(type_all("---", en), "---");
        assert_eq!(type_all("<!-- x -->", en), "<!-- x -->");
        assert_eq!(type_all("one-two", en), "one-two");
    }

    #[test]
    fn inline_code_is_left_alone() {
        let en = options(QuoteStyle::ENGLISH);
        assert_eq!(type_all("`a -- \"b\"` c", en), "`a -- \"b\"` c");
    }

    #[test]
    fn auto_pair_inserts_and_steps_over_curly_pairs() {
        let mut en = options(QuoteStyle::ENGLISH);
        en.auto_pair = true;
        let text = "say ";
        let range = text.len()..text.len();
        assert_eq!(
            smart_edit(text, &range, "\"", en),
            Some(SmartEdit::Replace {
                range: range.clone(),
                text: "\u{201C}\u{201D}".into(),
                selected: 3..3,
            })
        );
        let text = "say \u{201C}hi\u{201D}";
        let caret = text.len() - '\u{201D}'.len_utf8();
        assert_eq!(
            smart_edit(text, &(caret..caret), "\"", en),
            Some(SmartEdit::SkipOver(3))
        );
    }

    #[test]
    fn selections_are_wrapped() {
        let en = options(QuoteStyle::ENGLISH);
        assert_eq!(
            smart_edit("a word b", &(2..6), "\"", en),
            Some(SmartEdit::Replace {
                range: 2..6,
                text: "\u{201C}word\u{201D}".into(),
                selected: 3..7,
            })
        );
    }
}
