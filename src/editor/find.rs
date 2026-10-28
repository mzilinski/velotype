//! In-document find & replace bar.
//!
//! Matches are computed against each block's clean visible text (the text
//! without the inline-marker projection shown while a block is focused), so
//! the stored ranges stay valid while focus moves between blocks. Blocks paint
//! their own highlights from those ranges; the editor keeps keyboard focus in
//! the find field while stepping through matches.

use std::ops::Range;

use gpui::prelude::FluentBuilder as _;
use gpui::*;

use super::Editor;
use crate::components::single_line_input::{
    InputChanged, InputSubmitted, SingleLineInput, SingleLineOverflow,
};
use crate::components::{
    Block, FindAndReplace, FindInDocument, FindNext, FindPrevious, UndoCaptureKind,
};
use crate::i18n::I18nManager;
use crate::theme::Theme;

#[derive(Clone)]
pub(super) struct FindMatch {
    pub(super) entity: Entity<Block>,
    /// Byte range in the block's clean visible text.
    pub(super) range: Range<usize>,
}

/// One block's highlight ranges plus the index of the current match among them.
type BlockHighlights = (Entity<Block>, Vec<Range<usize>>, Option<usize>);

pub(super) struct FindState {
    pub(super) query: Entity<SingleLineInput>,
    pub(super) replacement: Entity<SingleLineInput>,
    pub(super) show_replace: bool,
    pub(super) case_sensitive: bool,
    pub(super) matches: Vec<FindMatch>,
    pub(super) current: Option<usize>,
    /// Blocks currently carrying highlights, so they can be cleared.
    highlighted: Vec<Entity<Block>>,
    _subscriptions: Vec<Subscription>,
}

/// Returns the non-overlapping byte ranges of `query` in `text`. Without
/// `case_sensitive`, characters are compared by their full lowercase mapping.
pub(super) fn find_ranges(text: &str, query: &str, case_sensitive: bool) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }
    if case_sensitive {
        return text
            .match_indices(query)
            .map(|(start, found)| start..start + found.len())
            .collect();
    }

    let query = query
        .chars()
        .flat_map(char::to_lowercase)
        .collect::<Vec<_>>();
    let mut ranges = Vec::new();
    let mut start = 0;
    while start < text.len() {
        if let Some(end) = lowercase_match_end(text, start, &query) {
            ranges.push(start..end);
            start = end;
        } else {
            start += text[start..].chars().next().map_or(1, char::len_utf8);
        }
    }
    ranges
}

fn lowercase_match_end(text: &str, start: usize, query: &[char]) -> Option<usize> {
    let mut matched = 0;
    for (offset, ch) in text[start..].char_indices() {
        for lower in ch.to_lowercase() {
            if query.get(matched) != Some(&lower) {
                return None;
            }
            matched += 1;
        }
        if matched == query.len() {
            return Some(start + offset + ch.len_utf8());
        }
    }
    None
}

impl Editor {
    pub(super) fn on_find_in_document(
        &mut self,
        _: &FindInDocument,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_find_bar(false, window, cx);
    }

    pub(super) fn on_find_and_replace(
        &mut self,
        _: &FindAndReplace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_find_bar(true, window, cx);
    }

    pub(super) fn on_find_next(
        &mut self,
        _: &FindNext,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.find.is_none() {
            self.open_find_bar(false, window, cx);
        }
        self.step_find_match(true, cx);
    }

    pub(super) fn on_find_previous(
        &mut self,
        _: &FindPrevious,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.find.is_none() {
            self.open_find_bar(false, window, cx);
        }
        self.step_find_match(false, cx);
    }

    fn open_find_bar(&mut self, show_replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        let seed = self.find_seed_from_selection(window, cx);
        if self.find.is_none() {
            self.find = Some(self.new_find_state(cx));
        }
        let Some(find) = self.find.as_mut() else {
            return;
        };
        find.show_replace |= show_replace;
        let query = find.query.clone();
        if let Some(seed) = seed {
            query.update(cx, |input, cx| input.set_value(seed, cx));
        }
        query.update(cx, |input, cx| input.select_all(cx));
        window.focus(&query.focus_handle(cx), cx);
        self.refresh_find_matches(cx);
        cx.notify();
    }

    fn new_find_state(&mut self, cx: &mut Context<Self>) -> FindState {
        let strings = cx.global::<I18nManager>().strings().clone();
        let query = cx.new(|cx| {
            SingleLineInput::new("", strings.find_placeholder.clone(), cx)
                .with_overflow(SingleLineOverflow::Clip)
        });
        let replacement = cx.new(|cx| {
            SingleLineInput::new("", strings.find_replace_placeholder.clone(), cx)
                .with_overflow(SingleLineOverflow::Clip)
        });
        let subscriptions = vec![
            cx.subscribe(&query, |this, _, _: &InputChanged, cx| {
                if let Some(find) = this.find.as_mut() {
                    find.current = None;
                }
                this.refresh_find_matches(cx);
                this.step_find_match(true, cx);
            }),
            cx.subscribe(&query, |this, _, event: &InputSubmitted, cx| {
                this.step_find_match(!event.shift, cx);
            }),
            cx.subscribe(&replacement, |this, _, event: &InputSubmitted, cx| {
                if event.shift {
                    this.replace_all_find_matches(cx);
                } else {
                    this.replace_current_find_match(cx);
                }
            }),
        ];
        FindState {
            query,
            replacement,
            show_replace: false,
            case_sensitive: false,
            matches: Vec::new(),
            current: None,
            highlighted: Vec::new(),
            _subscriptions: subscriptions,
        }
    }

    /// Uses a single-line selection in the focused block as the initial query.
    fn find_seed_from_selection(&self, window: &Window, cx: &App) -> Option<String> {
        let block = self.focused_edit_target(window, cx)?;
        let block = block.read(cx);
        if block.selected_range.is_empty() {
            return None;
        }
        let range = block.current_to_clean_range(block.selected_range.clone());
        let text = block.clean_text().get(range)?;
        (!text.contains('\n')).then(|| text.to_string())
    }

    /// Closes the bar and clears highlights. When focus was in the bar, it
    /// returns to the document with the current match selected.
    pub(super) fn close_find_bar(&mut self, window: &Window, cx: &mut Context<Self>) -> bool {
        let Some(find) = self.find.take() else {
            return false;
        };
        for entity in &find.highlighted {
            entity.update(cx, |block, cx| {
                block.set_search_highlights(Vec::new(), None, cx)
            });
        }
        let focus_was_in_bar = find.query.focus_handle(cx).is_focused(window)
            || find.replacement.focus_handle(cx).is_focused(window);
        let target = find
            .current
            .and_then(|index| find.matches.get(index))
            .cloned();
        if !focus_was_in_bar {
            // Keep the user's caret where it is when the bar is closed from
            // the document (e.g. Escape while typing).
        } else if let Some(found) = target {
            let range = found
                .entity
                .read(cx)
                .clean_to_current_range(found.range.clone());
            self.focus_block_range(&found.entity, range, cx);
        } else if let Some(active) = self.active_entity_id {
            self.focus_block(active);
        }
        cx.notify();
        true
    }

    /// Text-bearing blocks in reading order; tables contribute their cells.
    fn searchable_blocks(&self, cx: &App) -> Vec<Entity<Block>> {
        let mut blocks = Vec::new();
        for visible in self.document.visible_blocks() {
            let block = visible.entity.read(cx);
            if let Some(runtime) = block.table_runtime.as_ref() {
                blocks.extend(runtime.header.iter().cloned());
                for row in &runtime.rows {
                    blocks.extend(row.iter().cloned());
                }
            } else {
                blocks.push(visible.entity.clone());
            }
        }
        blocks
    }

    pub(super) fn refresh_find_matches(&mut self, cx: &mut Context<Self>) {
        let Some(find) = self.find.as_ref() else {
            return;
        };
        let query = find.query.read(cx).value().to_string();
        let case_sensitive = find.case_sensitive;
        let previous = find
            .current
            .and_then(|index| find.matches.get(index))
            .map(|found| (found.entity.entity_id(), found.range.start));

        let mut matches = Vec::new();
        for entity in self.searchable_blocks(cx) {
            let ranges = find_ranges(entity.read(cx).clean_text(), &query, case_sensitive);
            matches.extend(ranges.into_iter().map(|range| FindMatch {
                entity: entity.clone(),
                range,
            }));
        }
        let current = previous.and_then(|(entity_id, start)| {
            matches.iter().position(|found| {
                found.entity.entity_id() == entity_id && found.range.start >= start
            })
        });

        if let Some(find) = self.find.as_mut() {
            find.matches = matches;
            find.current = current;
        }
        self.sync_find_highlights(cx);
    }

    fn sync_find_highlights(&mut self, cx: &mut Context<Self>) {
        let Some(find) = self.find.as_mut() else {
            return;
        };
        let mut per_block: Vec<BlockHighlights> = Vec::new();
        for (index, found) in find.matches.iter().enumerate() {
            let is_current = find.current == Some(index);
            match per_block.last_mut() {
                Some((entity, ranges, active))
                    if entity.entity_id() == found.entity.entity_id() =>
                {
                    if is_current {
                        *active = Some(ranges.len());
                    }
                    ranges.push(found.range.clone());
                }
                _ => per_block.push((
                    found.entity.clone(),
                    vec![found.range.clone()],
                    is_current.then_some(0),
                )),
            }
        }
        let stale = std::mem::take(&mut find.highlighted);
        find.highlighted = per_block
            .iter()
            .map(|(entity, _, _)| entity.clone())
            .collect();

        for entity in stale {
            if !per_block
                .iter()
                .any(|(current, _, _)| current.entity_id() == entity.entity_id())
            {
                entity.update(cx, |block, cx| {
                    block.set_search_highlights(Vec::new(), None, cx)
                });
            }
        }
        for (entity, ranges, active) in per_block {
            entity.update(cx, |block, cx| {
                block.set_search_highlights(ranges, active, cx)
            });
        }
        cx.notify();
    }

    /// Moves to the next (or previous) match. Without a current match, starts
    /// from the active block so the search continues where the user was.
    fn step_find_match(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(find) = self.find.as_ref() else {
            return;
        };
        let count = find.matches.len();
        if count == 0 {
            return;
        }
        let next = match find.current {
            Some(index) if forward => (index + 1) % count,
            Some(index) => (index + count - 1) % count,
            None => self.first_match_from_active_block(forward, cx),
        };
        self.select_find_match(next, cx);
    }

    fn first_match_from_active_block(&self, forward: bool, cx: &App) -> usize {
        let Some(find) = self.find.as_ref() else {
            return 0;
        };
        let order = self
            .searchable_blocks(cx)
            .iter()
            .map(|entity| entity.entity_id())
            .collect::<Vec<_>>();
        let position = |entity_id| order.iter().position(|id| *id == entity_id);
        let Some(active_position) = self.active_entity_id.and_then(position) else {
            return if forward { 0 } else { find.matches.len() - 1 };
        };
        let cursor = self
            .active_entity_id
            .and_then(|id| self.focusable_entity_by_id(id))
            .map(|block| {
                let block = block.read(cx);
                block.current_to_clean_offset(block.cursor_offset())
            })
            .unwrap_or(0);
        let key = |found: &FindMatch| {
            (
                position(found.entity.entity_id()).unwrap_or(usize::MAX),
                found.range.start,
            )
        };
        if forward {
            find.matches
                .iter()
                .position(|found| key(found) >= (active_position, cursor))
                .unwrap_or(0)
        } else {
            find.matches
                .iter()
                .rposition(|found| key(found) < (active_position, cursor))
                .unwrap_or(find.matches.len() - 1)
        }
    }

    fn select_find_match(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(find) = self.find.as_mut() else {
            return;
        };
        let Some(found) = find.matches.get(index).cloned() else {
            return;
        };
        find.current = Some(index);
        found.entity.update(cx, |block, cx| {
            block.selected_range = block.clean_to_current_range(found.range.clone());
            block.selection_reversed = false;
            cx.notify();
        });
        self.active_entity_id = Some(found.entity.entity_id());
        self.sync_find_highlights(cx);
        self.request_active_block_scroll_into_view(cx);
    }

    /// The block to keep mounted and scroll to while the find field has focus.
    pub(super) fn find_scroll_target(&self) -> Option<Entity<Block>> {
        let find = self.find.as_ref()?;
        find.matches
            .get(find.current?)
            .map(|found| found.entity.clone())
    }

    fn replace_current_find_match(&mut self, cx: &mut Context<Self>) {
        let Some(find) = self.find.as_ref() else {
            return;
        };
        let Some(index) = find.current else {
            self.step_find_match(true, cx);
            return;
        };
        let Some(found) = find.matches.get(index).cloned() else {
            return;
        };
        let replacement = find.replacement.read(cx).value().to_string();
        self.replace_find_ranges(&found.entity, vec![found.range.clone()], &replacement, cx);
        self.refresh_find_matches(cx);
        // The replaced text no longer matches, so the next match now sits at
        // the same index (wrapping at the end).
        let count = self.find.as_ref().map_or(0, |find| find.matches.len());
        if count > 0 {
            self.select_find_match(index % count, cx);
        }
    }

    fn replace_all_find_matches(&mut self, cx: &mut Context<Self>) {
        let Some(find) = self.find.as_ref() else {
            return;
        };
        if find.matches.is_empty() {
            return;
        }
        let replacement = find.replacement.read(cx).value().to_string();
        let mut grouped: Vec<(Entity<Block>, Vec<Range<usize>>)> = Vec::new();
        for found in &find.matches {
            match grouped.last_mut() {
                Some((entity, ranges)) if entity.entity_id() == found.entity.entity_id() => {
                    ranges.push(found.range.clone());
                }
                _ => grouped.push((found.entity.clone(), vec![found.range.clone()])),
            }
        }
        // One undo entry covers every block touched by the replacement.
        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        for (entity, ranges) in grouped {
            self.replace_find_ranges(&entity, ranges, &replacement, cx);
        }
        if let Some(find) = self.find.as_mut() {
            find.current = None;
        }
        self.refresh_find_matches(cx);
    }

    /// Replaces `ranges` (clean offsets, ascending) in one block, back to
    /// front so earlier offsets stay valid.
    fn replace_find_ranges(
        &mut self,
        entity: &Entity<Block>,
        ranges: Vec<Range<usize>>,
        replacement: &str,
        cx: &mut Context<Self>,
    ) {
        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        entity.update(cx, |block, cx| {
            for range in ranges.into_iter().rev() {
                let current = block.clean_to_current_range(range);
                block.replace_text_in_visible_range(current, replacement, None, false, cx);
            }
        });
    }

    fn toggle_find_case_sensitive(&mut self, cx: &mut Context<Self>) {
        if let Some(find) = self.find.as_mut() {
            find.case_sensitive = !find.case_sensitive;
            find.current = None;
        }
        self.refresh_find_matches(cx);
    }

    fn toggle_find_replace_row(&mut self, cx: &mut Context<Self>) {
        if let Some(find) = self.find.as_mut() {
            find.show_replace = !find.show_replace;
        }
        cx.notify();
    }

    pub(super) fn render_find_bar(
        &self,
        theme: &Theme,
        top: Pixels,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let find = self.find.as_ref()?;
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let strings = cx.global::<I18nManager>().strings().clone();

        let status = if find.query.read(cx).value().is_empty() {
            String::new()
        } else if find.matches.is_empty() {
            strings.find_no_results.clone()
        } else {
            strings
                .find_match_count
                .replace(
                    "{current}",
                    &find
                        .current
                        .map_or("?".to_string(), |index| (index + 1).to_string()),
                )
                .replace("{total}", &find.matches.len().to_string())
        };

        let button = |id: &'static str, label: SharedString, active: bool| {
            div()
                .id(id)
                .h(px(28.0))
                .min_w(px(28.0))
                .px(px(8.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(d.menu_item_radius))
                .text_size(px(d.menu_text_size))
                .font_weight(t.dialog_button_weight.to_font_weight())
                .text_color(c.dialog_secondary_button_text)
                .when(active, |this| this.bg(c.dialog_secondary_button_hover))
                .hover(|this| this.bg(c.dialog_secondary_button_hover))
                .cursor_pointer()
                .child(label)
        };

        let find_row = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(
                button(
                    "find-toggle-replace",
                    if find.show_replace { "▾" } else { "▸" }.into(),
                    false,
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_find_replace_row(cx))),
            )
            .child(div().w(px(220.0)).child(find.query.clone()))
            .child(
                button("find-match-case", "Aa".into(), find.case_sensitive)
                    .tooltip({
                        let label = strings.find_match_case.clone();
                        move |_window, cx| cx.new(|_| FindTooltip(label.clone().into())).into()
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_find_case_sensitive(cx))),
            )
            .child(
                div()
                    .min_w(px(72.0))
                    .text_size(px(d.menu_text_size))
                    .text_color(c.text_placeholder)
                    .child(status),
            )
            .child(
                button("find-previous", "↑".into(), false)
                    .on_click(cx.listener(|this, _, _, cx| this.step_find_match(false, cx))),
            )
            .child(
                button("find-next", "↓".into(), false)
                    .on_click(cx.listener(|this, _, _, cx| this.step_find_match(true, cx))),
            )
            .child(
                button("find-close", "✕".into(), false).on_click(cx.listener(
                    |this, _, window, cx| {
                        this.close_find_bar(window, cx);
                    },
                )),
            );

        let replace_row = find.show_replace.then(|| {
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .pl(px(34.0))
                .child(div().w(px(220.0)).child(find.replacement.clone()))
                .child(
                    button(
                        "find-replace-one",
                        strings.find_replace_one.clone().into(),
                        false,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.replace_current_find_match(cx))),
                )
                .child(
                    button(
                        "find-replace-all",
                        strings.find_replace_all.clone().into(),
                        false,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.replace_all_find_matches(cx))),
                )
        });

        Some(
            div()
                .id("find-bar")
                .absolute()
                .top(top + px(8.0))
                .right(px(24.0))
                .p(px(6.0))
                .flex()
                .flex_col()
                .gap(px(6.0))
                .bg(c.dialog_surface)
                .border(px(d.dialog_border_width))
                .border_color(c.dialog_border)
                .rounded(px(d.menu_panel_radius))
                .shadow_lg()
                .occlude()
                .text_size(px(t.dialog_body_size))
                .child(find_row)
                .children(replace_row)
                .into_any_element(),
        )
    }
}

pub(super) struct FindTooltip(pub(super) SharedString);

impl Render for FindTooltip {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<crate::theme::ThemeManager>().current();
        div()
            .px(px(8.0))
            .py(px(4.0))
            .rounded(px(theme.dimensions.menu_item_radius))
            .bg(theme.colors.dialog_surface)
            .border(px(theme.dimensions.dialog_border_width))
            .border_color(theme.colors.dialog_border)
            .text_size(px(theme.dimensions.menu_text_size))
            .text_color(theme.colors.dialog_body)
            .child(self.0.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::find_ranges;

    #[test]
    fn finds_case_insensitive_matches_without_overlap() {
        assert_eq!(find_ranges("Aaa aA", "aa", false), vec![0..2, 4..6]);
        assert_eq!(find_ranges("Aaa aA", "aa", true), vec![1..3]);
        assert!(find_ranges("abc", "", false).is_empty());
    }

    #[test]
    fn case_insensitive_offsets_stay_on_original_text() {
        // `İ` lowercases to two chars, so offsets must come from the source.
        let text = "xİy Straße STRASSE";
        let ranges = find_ranges(text, "straße", false);
        assert_eq!(ranges.len(), 1);
        assert_eq!(&text[ranges[0].clone()], "Straße");
        assert_eq!(find_ranges(text, "i̇y", false), vec![1..4]);
    }

    #[test]
    fn finds_cjk_and_emoji() {
        let text = "*测试 abc 测试 🎉";
        let ranges = find_ranges(text, "测试", false);
        assert_eq!(ranges.len(), 2);
        assert!(ranges.iter().all(|r| &text[r.clone()] == "测试"));
        assert_eq!(&text[find_ranges(text, "🎉", true)[0].clone()], "🎉");
    }
}

#[cfg(test)]
mod editor_tests {
    use gpui::{Focusable as _, TestAppContext};

    use crate::components::{FindAndReplace, FindNext};
    use crate::editor::Editor;
    use crate::i18n::I18nManager;
    use crate::theme::ThemeManager;

    fn init(cx: &mut TestAppContext) {
        cx.update(|cx| {
            I18nManager::init(cx);
            ThemeManager::init(cx);
            crate::components::init(cx);
        });
    }

    #[gpui::test]
    async fn find_highlights_steps_and_replaces(cx: &mut TestAppContext) {
        init(cx);
        let markdown = "alpha beta\n\nBeta gamma **beta**".to_string();
        let (editor, cx) =
            cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown, None));

        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.on_find_and_replace(&FindAndReplace, window, cx);
                let find = editor.find.as_ref().expect("find bar open");
                assert!(find.show_replace);
                assert!(find.query.focus_handle(cx).is_focused(window));
                let query = find.query.clone();
                let replacement = find.replacement.clone();
                query.update(cx, |input, cx| input.set_value("beta", cx));
                replacement.update(cx, |input, cx| input.set_value("X", cx));
            });
        });
        cx.run_until_parked();

        editor.update(cx, |editor, cx| {
            let find = editor.find.as_ref().expect("find bar open");
            assert_eq!(find.matches.len(), 3);
            // Typing the query jumps to the first match.
            assert_eq!(find.current, Some(0));
            let first = find.matches[0].entity.read(cx);
            assert_eq!(first.search_highlights, vec![6..10]);
            assert_eq!(first.active_search_highlight, Some(0));
        });

        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| editor.on_find_next(&FindNext, window, cx));
        });
        editor.update(cx, |editor, _cx| {
            assert_eq!(editor.find.as_ref().unwrap().current, Some(1));
        });

        // Replace the current match only.
        editor.update(cx, |editor, cx| editor.replace_current_find_match(cx));
        cx.run_until_parked();
        editor.update(cx, |editor, cx| {
            assert_eq!(
                editor.document.markdown_text(cx),
                "alpha beta\n\nX gamma **beta**"
            );
            let find = editor.find.as_ref().unwrap();
            assert_eq!(find.matches.len(), 2);
            assert_eq!(find.current, Some(1));
        });

        // Replace the rest in one undoable step.
        editor.update(cx, |editor, cx| editor.replace_all_find_matches(cx));
        cx.run_until_parked();
        editor.update(cx, |editor, cx| {
            assert_eq!(
                editor.document.markdown_text(cx),
                "alpha X\n\nX gamma **X**"
            );
            assert!(editor.find.as_ref().unwrap().matches.is_empty());
            editor.undo_document(cx);
        });
        cx.run_until_parked();
        editor.update(cx, |editor, cx| {
            assert_eq!(
                editor.document.markdown_text(cx),
                "alpha beta\n\nX gamma **beta**"
            );
        });

        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                assert!(editor.close_find_bar(window, cx));
                assert!(editor.find.is_none());
                for visible in editor.document.visible_blocks() {
                    assert!(visible.entity.read(cx).search_highlights.is_empty());
                }
            });
        });
    }
}
