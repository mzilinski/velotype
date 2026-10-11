//! Typora-style Paragraph and Format commands as edits of the Markdown source.
//!
//! Each command maps the whole document source plus the selection (as source
//! byte offsets) to a new source and selection. The editor applies the result
//! as one undo step and rebuilds the blocks, so container blocks such as
//! quotes and lists never have to be restructured by hand.

use std::ops::Range;

/// New document source and the selection to restore in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SourceEdit {
    pub(super) source: String,
    pub(super) selection: Range<usize>,
}

/// Line-prefix block styles toggled over the selected lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LineStyle {
    Quote,
    BulletList,
    OrderedList,
    TaskList,
}

/// Byte ranges (without the line break) of the lines touched by `selection`.
fn selected_lines(source: &str, selection: &Range<usize>) -> Vec<Range<usize>> {
    let start = selection.start.min(source.len());
    let end = selection.end.min(source.len()).max(start);
    let mut lines = Vec::new();
    let mut line_start = 0;
    for line in source.split('\n') {
        if line_start > end {
            break;
        }
        let line_end = line_start + line.len();
        // A caret selects its own line; a range ending right at the start of
        // a line does not include that line.
        let touches = if start == end {
            line_start <= start && start <= line_end
        } else {
            line_start < end && start <= line_end
        };
        if touches {
            lines.push(line_start..line_end);
        }
        line_start = line_end + 1;
    }
    lines
}

/// Replaces the first `old_len` bytes of each listed line with a new prefix
/// and maps the selection through the change.
fn rewrite_prefixes(
    source: &str,
    selection: &Range<usize>,
    rewrites: &[(Range<usize>, usize, String)],
) -> SourceEdit {
    let mut out = String::with_capacity(source.len() + rewrites.len() * 4);
    let mut last = 0;
    // (old line start, old prefix len, new line start, new prefix len)
    let mut map = Vec::with_capacity(rewrites.len());
    for (line, old_len, prefix) in rewrites {
        out.push_str(&source[last..line.start]);
        let new_start = out.len();
        out.push_str(prefix);
        map.push((line.start, line.end, *old_len, new_start, prefix.len()));
        last = line.start + old_len;
    }
    out.push_str(&source[last..]);
    let map_offset = |offset: usize| -> usize {
        let mut shift: isize = 0;
        for &(start, end, old_len, new_start, new_len) in &map {
            if offset < start {
                break;
            }
            if offset <= end {
                let in_line = offset - start;
                return new_start + new_len + in_line.saturating_sub(old_len);
            }
            shift += new_len as isize - old_len as isize;
        }
        (offset as isize + shift).max(0) as usize
    };
    let selection = map_offset(selection.start)..map_offset(selection.end);
    SourceEdit {
        source: out,
        selection,
    }
}

/// Length of a list marker at the start of `line` (after indentation),
/// including a task box, and whether it is ordered / a task.
fn list_marker(line: &str) -> Option<(usize, bool, bool)> {
    let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
    let rest = &line[indent..];
    let marker_len = if rest.starts_with(['-', '*', '+']) {
        1
    } else {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 || digits > 9 || !rest[digits..].starts_with(['.', ')']) {
            return None;
        }
        digits + 1
    };
    let after = &rest[marker_len..];
    if !(after.is_empty() || after.starts_with(' ')) {
        return None;
    }
    let ordered = !rest.starts_with(['-', '*', '+']);
    let spaces = after.len() - after.trim_start_matches(' ').len();
    let mut len = indent + marker_len + spaces;
    let mut task = false;
    let tail = &line[len..];
    if !ordered
        && (tail.starts_with("[ ]") || tail.starts_with("[x]") || tail.starts_with("[X]"))
        && (tail.len() == 3 || tail[3..].starts_with(' '))
    {
        task = true;
        len += 3 + usize::from(tail.len() > 3);
    }
    Some((len, ordered, task))
}

fn quote_prefix_len(line: &str) -> Option<usize> {
    let rest = line.strip_prefix('>')?;
    Some(1 + usize::from(rest.starts_with(' ')))
}

/// Toggles a quote or list style on the selected lines. When every non-empty
/// line already has the style it is removed; otherwise other list markers
/// are replaced and the style is added (ordered lists are renumbered).
pub(super) fn toggle_line_style(
    source: &str,
    selection: Range<usize>,
    style: LineStyle,
) -> Option<SourceEdit> {
    let lines = selected_lines(source, &selection);
    if lines.is_empty() {
        return None;
    }
    let text = |line: &Range<usize>| &source[line.clone()];
    let has_style = |line: &str| match style {
        LineStyle::Quote => quote_prefix_len(line).is_some(),
        LineStyle::BulletList => {
            list_marker(line).is_some_and(|(_, ordered, task)| !ordered && !task)
        }
        LineStyle::OrderedList => list_marker(line).is_some_and(|(_, ordered, _)| ordered),
        LineStyle::TaskList => list_marker(line).is_some_and(|(_, _, task)| task),
    };
    let remove = lines
        .iter()
        .filter(|line| !text(line).trim().is_empty())
        .all(|line| has_style(text(line)));

    let mut number = 0;
    let rewrites = lines
        .iter()
        .filter_map(|line| {
            let line_text = text(line);
            let blank = line_text.trim().is_empty();
            if style == LineStyle::Quote {
                return if remove {
                    quote_prefix_len(line_text).map(|len| (line.clone(), len, String::new()))
                } else if blank {
                    Some((line.clone(), line_text.len(), ">".to_string()))
                } else {
                    Some((line.clone(), 0, "> ".to_string()))
                };
            }
            if blank {
                return None;
            }
            let existing = list_marker(line_text);
            let indent = line_text.len() - line_text.trim_start_matches([' ', '\t']).len();
            let old_len = existing.map_or(indent, |(len, _, _)| len);
            let keep_indent = line_text[..indent].to_string();
            if remove {
                return Some((line.clone(), old_len, keep_indent));
            }
            number += 1;
            let marker = match style {
                LineStyle::BulletList => "- ".to_string(),
                LineStyle::OrderedList => format!("{number}. "),
                LineStyle::TaskList => "- [ ] ".to_string(),
                LineStyle::Quote => unreachable!(),
            };
            Some((line.clone(), old_len, format!("{keep_indent}{marker}")))
        })
        .collect::<Vec<_>>();
    if rewrites.is_empty() {
        return None;
    }
    Some(rewrite_prefixes(source, &selection, &rewrites))
}

/// Typora's Increase/Decrease Heading Level on the caret line: increasing
/// goes paragraph → H6 → … → H1, decreasing goes H1 → … → H6 → paragraph.
pub(super) fn shift_heading_level(
    source: &str,
    selection: Range<usize>,
    increase: bool,
) -> Option<SourceEdit> {
    let caret = selection.start..selection.start;
    let line = selected_lines(source, &caret).into_iter().next()?;
    let text = &source[line.clone()];
    if text.trim().is_empty() || quote_prefix_len(text).is_some() || list_marker(text).is_some() {
        return None;
    }
    let hashes = text.bytes().take_while(|byte| *byte == b'#').count();
    let is_heading =
        (1..=6).contains(&hashes) && (text.len() == hashes || text[hashes..].starts_with(' '));
    let level = if is_heading { hashes } else { 0 };
    let next = match (increase, level) {
        (true, 0) => 6,
        (true, 1) => return None,
        (true, level) => level - 1,
        (false, 0) => return None,
        (false, 6) => 0,
        (false, level) => level + 1,
    };
    let old_len = if is_heading {
        hashes + usize::from(text.len() > hashes)
    } else {
        0
    };
    let prefix = if next == 0 {
        String::new()
    } else {
        format!("{} ", "#".repeat(next))
    };
    Some(rewrite_prefixes(
        source,
        &selection,
        &[(line, old_len, prefix)],
    ))
}

/// Checks every selected task item, or unchecks them all when they already
/// are checked.
pub(super) fn toggle_task_status(source: &str, selection: Range<usize>) -> Option<SourceEdit> {
    let tasks = selected_lines(source, &selection)
        .into_iter()
        .filter_map(|line| {
            let text = &source[line.clone()];
            let (len, _, task) = list_marker(text)?;
            if !task {
                return None;
            }
            let box_start = text[..len].rfind('[')?;
            Some((line, box_start, text[box_start..].starts_with("[ ]")))
        })
        .collect::<Vec<_>>();
    if tasks.is_empty() {
        return None;
    }
    let check = tasks.iter().any(|(_, _, open)| *open);
    let mut out = source.to_string();
    for (line, box_start, _) in &tasks {
        let at = line.start + box_start + 1;
        out.replace_range(at..at + 1, if check { "x" } else { " " });
    }
    Some(SourceEdit {
        source: out,
        selection,
    })
}

/// Inserts `snippet` as its own block after the block holding the caret (or
/// in place of an empty line). `caret` is the caret offset within `snippet`.
pub(super) fn insert_block(
    source: &str,
    selection: Range<usize>,
    snippet: &str,
    caret: usize,
) -> SourceEdit {
    let at = selection.end.min(source.len());
    let line_start = source[..at].rfind('\n').map_or(0, |index| index + 1);
    let line_end = source[at..]
        .find('\n')
        .map_or(source.len(), |index| at + index);
    let (split_before, split_after) = if source[line_start..line_end].trim().is_empty() {
        (line_start, line_end)
    } else {
        // The block ends at the next blank line.
        let mut end = line_end;
        while end < source.len() {
            let next_end = source[end + 1..]
                .find('\n')
                .map_or(source.len(), |index| end + 1 + index);
            if source[end + 1..next_end].trim().is_empty() {
                break;
            }
            end = next_end;
        }
        (end, end)
    };
    let before = source[..split_before].trim_end_matches('\n');
    let after = source[split_after..].trim_start_matches('\n');
    let mut out = String::with_capacity(source.len() + snippet.len() + 4);
    out.push_str(before);
    if !before.is_empty() {
        out.push_str("\n\n");
    }
    let snippet_start = out.len();
    out.push_str(snippet);
    if after.is_empty() {
        out.push('\n');
    } else {
        out.push_str("\n\n");
        out.push_str(after);
    }
    let caret = snippet_start + caret.min(snippet.len());
    SourceEdit {
        source: out,
        selection: caret..caret,
    }
}

/// Code or math block: wraps the selected lines in `fence` lines, or inserts
/// an empty block with the caret inside when nothing is selected.
pub(super) fn insert_fenced_block(
    source: &str,
    selection: Range<usize>,
    fence: &str,
) -> SourceEdit {
    if selection.is_empty() {
        let snippet = format!("{fence}\n\n{fence}");
        return insert_block(source, selection, &snippet, fence.len() + 1);
    }
    let lines = selected_lines(source, &selection);
    let (Some(first), Some(last)) = (lines.first(), lines.last()) else {
        return insert_block(
            source,
            selection,
            &format!("{fence}\n\n{fence}"),
            fence.len() + 1,
        );
    };
    let body = &source[first.start..last.end];
    let mut out = String::with_capacity(source.len() + fence.len() * 2 + 2);
    out.push_str(&source[..first.start]);
    out.push_str(fence);
    out.push('\n');
    let body_start = out.len();
    out.push_str(body);
    let body_end = out.len();
    out.push('\n');
    out.push_str(fence);
    out.push_str(&source[last.end..]);
    SourceEdit {
        source: out,
        selection: body_start..body_end,
    }
}

/// Wraps the selection in `before`/`after`, keeping the wrapped text
/// selected (or the caret between them for an empty selection).
pub(super) fn wrap_inline(
    source: &str,
    selection: Range<usize>,
    before: &str,
    after: &str,
) -> SourceEdit {
    let start = selection.start.min(source.len());
    let end = selection.end.min(source.len()).max(start);
    let mut out = String::with_capacity(source.len() + before.len() + after.len());
    out.push_str(&source[..start]);
    out.push_str(before);
    out.push_str(&source[start..end]);
    out.push_str(after);
    out.push_str(&source[end..]);
    SourceEdit {
        source: out,
        selection: start + before.len()..end + before.len(),
    }
}

/// Ctrl+K: `[selection](url)`. Without a URL the caret goes between the
/// parentheses; without a selection it goes between the brackets.
pub(super) fn insert_link(source: &str, selection: Range<usize>, url: Option<&str>) -> SourceEdit {
    let start = selection.start.min(source.len());
    let end = selection.end.min(source.len()).max(start);
    let text = &source[start..end];
    let url = url.unwrap_or_default();
    let link = format!("[{text}]({url})");
    let mut out = String::with_capacity(source.len() + link.len());
    out.push_str(&source[..start]);
    out.push_str(&link);
    out.push_str(&source[end..]);
    let caret = if text.is_empty() {
        start + 1
    } else if url.is_empty() {
        start + text.len() + 3
    } else {
        start + link.len()
    };
    SourceEdit {
        source: out,
        selection: caret..caret,
    }
}

/// Whether `text` looks like a URL worth using as a link target.
pub(super) fn looks_like_url(text: &str) -> bool {
    let text = text.trim();
    !text.contains(char::is_whitespace)
        && ["http://", "https://", "mailto:", "ftp://", "file://"]
            .iter()
            .any(|scheme| {
                text.len() > scheme.len() && text.to_ascii_lowercase().starts_with(scheme)
            })
}

/// Inserts the next free numeric footnote reference at the caret and its
/// definition at the end of the document, with the caret in the definition.
pub(super) fn insert_footnote(source: &str, selection: Range<usize>) -> SourceEdit {
    let mut next = 1;
    let mut rest = source;
    while let Some(index) = rest.find("[^") {
        rest = &rest[index + 2..];
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0
            && rest[digits..].starts_with(']')
            && let Ok(number) = rest[..digits].parse::<usize>()
        {
            next = next.max(number + 1);
        }
    }
    let reference = format!("[^{next}]");
    let at = selection.end.min(source.len());
    let mut out = String::with_capacity(source.len() + reference.len() * 2 + 4);
    out.push_str(&source[..at]);
    out.push_str(&reference);
    let tail = &source[at..];
    out.push_str(tail.trim_end_matches('\n'));
    out.push_str("\n\n");
    out.push_str(&reference);
    out.push_str(": ");
    let caret = out.len();
    out.push('\n');
    SourceEdit {
        source: out,
        selection: caret..caret,
    }
}

/// Adds an empty YAML front matter block at the start, unless one exists.
pub(super) fn insert_front_matter(source: &str) -> Option<SourceEdit> {
    if source.starts_with("---\n") || source.starts_with("---\r\n") {
        return None;
    }
    let body = source.trim_start_matches('\n');
    let mut out = String::from("---\n\n---\n");
    if !body.is_empty() {
        out.push('\n');
        out.push_str(body);
    }
    Some(SourceEdit {
        source: out,
        selection: 4..4,
    })
}

/// Removes inline Markdown markers from the selected source text: emphasis,
/// strike, highlight, code, super/subscript, `<u>` and link syntax.
pub(super) fn clear_inline_markup(source: &str, selection: Range<usize>) -> Option<SourceEdit> {
    let start = selection.start.min(source.len());
    let end = selection.end.min(source.len()).max(start);
    if start == end {
        return None;
    }
    let text = &source[start..end];
    let mut cleaned = String::with_capacity(text.len());
    let mut chars = text.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        match ch {
            '*' | '_' | '~' | '=' | '`' | '^' => {}
            '<' => {
                let tag = [
                    "<u>", "</u>", "<mark>", "</mark>", "<sup>", "</sup>", "<sub>", "</sub>",
                ]
                .iter()
                .find(|tag| text[index..].starts_with(**tag));
                if let Some(tag) = tag {
                    for _ in 1..tag.len() {
                        chars.next();
                    }
                } else {
                    cleaned.push(ch);
                }
            }
            '[' => {}
            ']' => {
                // Drop a following `(target)` of an inline link.
                if text[index + 1..].starts_with('(')
                    && let Some(close) = text[index + 1..].find(')')
                {
                    let skip_to = index + 1 + close;
                    while chars.peek().is_some_and(|(next, _)| *next <= skip_to) {
                        chars.next();
                    }
                }
            }
            _ => cleaned.push(ch),
        }
    }
    if cleaned == text {
        return None;
    }
    let mut out = String::with_capacity(source.len());
    out.push_str(&source[..start]);
    out.push_str(&cleaned);
    out.push_str(&source[end..]);
    Some(SourceEdit {
        selection: start..start + cleaned.len(),
        source: out,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit_at(source: &str, caret: usize) -> Range<usize> {
        caret.min(source.len())..caret.min(source.len())
    }

    #[test]
    fn quote_toggles_on_and_off_over_selected_lines() {
        let source = "intro\n\none\ntwo\n\nend";
        let selection = 7..12;
        let edit = toggle_line_style(source, selection, LineStyle::Quote).unwrap();
        assert_eq!(edit.source, "intro\n\n> one\n> two\n\nend");
        assert_eq!(&edit.source[edit.selection.clone()], "one\n> t");

        let back = toggle_line_style(&edit.source, edit.selection, LineStyle::Quote).unwrap();
        assert_eq!(back.source, source);
    }

    #[test]
    fn lists_replace_each_other_and_number_ordered_items() {
        let source = "- a\n- b\n- c";
        let all = 0..source.len();
        let ordered = toggle_line_style(source, all.clone(), LineStyle::OrderedList).unwrap();
        assert_eq!(ordered.source, "1. a\n2. b\n3. c");
        let tasks = toggle_line_style(
            &ordered.source,
            0..ordered.source.len(),
            LineStyle::TaskList,
        )
        .unwrap();
        assert_eq!(tasks.source, "- [ ] a\n- [ ] b\n- [ ] c");
        let plain =
            toggle_line_style(&tasks.source, 0..tasks.source.len(), LineStyle::TaskList).unwrap();
        assert_eq!(plain.source, "a\nb\nc");
        let bullets = toggle_line_style("  x", edit_at("  x", 3), LineStyle::BulletList).unwrap();
        assert_eq!(bullets.source, "  - x");
        assert_eq!(bullets.selection, 5..5, "caret stays after the text");
    }

    #[test]
    fn heading_level_steps_like_typora() {
        let up = shift_heading_level("Title", 0..0, true).unwrap();
        assert_eq!(up.source, "###### Title");
        let up = shift_heading_level("## Title", 4..4, true).unwrap();
        assert_eq!(up.source, "# Title");
        assert_eq!(up.selection, 3..3);
        assert!(shift_heading_level("# Title", 0..0, true).is_none());
        let down = shift_heading_level("###### Title", 0..0, false).unwrap();
        assert_eq!(down.source, "Title");
        assert!(shift_heading_level("Title", 0..0, false).is_none());
        assert!(shift_heading_level("- item", 0..0, true).is_none());
        let second = shift_heading_level("a\n\n# b", 5..5, false).unwrap();
        assert_eq!(second.source, "a\n\n## b");
    }

    #[test]
    fn task_status_checks_all_then_unchecks() {
        let source = "- [ ] a\n- [x] b";
        let checked = toggle_task_status(source, 0..source.len()).unwrap();
        assert_eq!(checked.source, "- [x] a\n- [x] b");
        let open = toggle_task_status(&checked.source, 0..source.len()).unwrap();
        assert_eq!(open.source, "- [ ] a\n- [ ] b");
        assert!(toggle_task_status("- a", 0..3).is_none());
    }

    #[test]
    fn blocks_are_inserted_after_the_current_paragraph() {
        let source = "first line\nsecond line\n\nnext";
        let edit = insert_block(source, edit_at(source, 2), "---", 3);
        assert_eq!(edit.source, "first line\nsecond line\n\n---\n\nnext");
        // An empty line is replaced instead.
        let source = "a\n\n\n\nb";
        let edit = insert_block(source, edit_at(source, 3), "[TOC]", 5);
        assert_eq!(edit.source, "a\n\n[TOC]\n\nb");
        let edit = insert_block("", 0..0, "$$\n\n$$", 3);
        assert_eq!(edit.source, "$$\n\n$$\n");
        assert_eq!(edit.selection, 3..3);
    }

    #[test]
    fn fenced_blocks_wrap_selected_lines() {
        let source = "x\nlet a = 1;\nlet b = 2;\ny";
        let edit = insert_fenced_block(source, 2..23, "```");
        assert_eq!(edit.source, "x\n```\nlet a = 1;\nlet b = 2;\n```\ny");
        assert_eq!(&edit.source[edit.selection], "let a = 1;\nlet b = 2;");
        let empty = insert_fenced_block("text", 4..4, "```");
        assert_eq!(empty.source, "text\n\n```\n\n```\n");
        assert_eq!(empty.selection, 10..10);
    }

    #[test]
    fn inline_wraps_links_and_footnotes() {
        let edit = wrap_inline("a word here", 2..6, "==", "==");
        assert_eq!(edit.source, "a ==word== here");
        assert_eq!(&edit.source[edit.selection], "word");

        let link = insert_link("see docs", 4..8, None);
        assert_eq!(link.source, "see [docs]()");
        assert_eq!(link.selection, 11..11);
        let link = insert_link("see docs", 4..8, Some("https://x.y"));
        assert_eq!(link.source, "see [docs](https://x.y)");
        let empty = insert_link("", 0..0, None);
        assert_eq!((empty.source.as_str(), empty.selection), ("[]()", 1..1));
        assert!(looks_like_url("https://example.com/a"));
        assert!(!looks_like_url("not a url"));

        let note = insert_footnote("Text[^1] more\n\n[^1]: one\n", 4..4);
        assert_eq!(note.source, "Text[^2][^1] more\n\n[^1]: one\n\n[^2]: \n");
        assert_eq!(
            &note.source[note.selection.start - 6..note.selection.start],
            "[^2]: "
        );
    }

    #[test]
    fn front_matter_only_once_and_clear_markup() {
        let edit = insert_front_matter("# Doc").unwrap();
        assert_eq!(edit.source, "---\n\n---\n\n# Doc");
        assert_eq!(edit.selection, 4..4);
        assert!(insert_front_matter(&edit.source).is_none());

        let source = "a **bold** and [link](http://x) ==hi== <u>u</u> end";
        let edit = clear_inline_markup(source, 0..source.len()).unwrap();
        assert_eq!(edit.source, "a bold and link hi u end");
        assert!(clear_inline_markup("plain", 0..5).is_none());
    }
}
