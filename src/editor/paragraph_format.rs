//! Paragraph and Format menu commands (Typora layout and shortcuts).
//!
//! Commands that restructure blocks (quotes, lists, inserted blocks) or wrap
//! text in Markdown syntax run as source edits from [`super::markdown_commands`]
//! and land as one undo step. Inline styles with a native representation
//! (strike, highlight, super/subscript) toggle on the focused block instead.

use std::ops::Range;
use std::time::Instant;

use gpui::*;

use super::context_menu::{TableInsertDialogState, TableInsertTarget};
use super::markdown_commands::{
    LineStyle, SourceEdit, clear_inline_markup, insert_block, insert_fenced_block, insert_footnote,
    insert_front_matter, insert_link, looks_like_url, shift_heading_level, toggle_line_style,
    toggle_task_status, wrap_inline,
};
use super::{Editor, HistoryEntry, UndoCaptureKind, UndoSelectionSnapshot};
use crate::components::{
    ClearFormat, CommentSelection, DecreaseHeadingLevel, IncreaseHeadingLevel, InlineMathSelection,
    InsertCallout, InsertCodeBlock, InsertFootnote, InsertFrontMatter, InsertHorizontalRule,
    InsertLink, InsertLinkReference, InsertMathBlock, InsertTable, InsertTableOfContents,
    ToggleBulletList, ToggleOrderedList, ToggleQuote, ToggleTaskList, ToggleTaskStatus,
};

/// GitHub alert types offered by Paragraph → Alert.
pub(crate) const CALLOUT_KINDS: &[&str] = &["note", "tip", "important", "warning", "caution"];

impl Editor {
    /// Replaces the document with `edit` as a single undo step, the same way
    /// undo/redo rebuild the blocks from source.
    pub(super) fn apply_source_edit(&mut self, edit: SourceEdit, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        let current = self.capture_history_entry(UndoCaptureKind::NonCoalescible, cx);
        if current.source_text == edit.source {
            return;
        }
        let entry = HistoryEntry {
            source_text: edit.source,
            selection: UndoSelectionSnapshot {
                range: edit.selection,
                reversed: false,
            },
            timestamp: Instant::now(),
            kind: UndoCaptureKind::NonCoalescible,
        };
        self.pending_undo_capture = None;
        self.history_restore_in_progress = true;
        self.clear_cross_block_selection(cx);
        self.restore_history_entry(&entry, cx);
        self.history_restore_in_progress = false;
        self.redo_history.clear();
        self.undo_history.push(current);
        if self.undo_history.len() > Self::HISTORY_LIMIT {
            let overflow = self.undo_history.len() - Self::HISTORY_LIMIT;
            self.undo_history.drain(0..overflow);
        }
        self.mark_dirty(cx);
        self.sync_table_axis_visuals(cx);
        self.dismiss_contextual_overlays(cx);
        cx.notify();
    }

    /// Runs a source command against the current source and selection.
    fn run_source_command(
        &mut self,
        cx: &mut Context<Self>,
        command: impl FnOnce(&str, Range<usize>) -> Option<SourceEdit>,
    ) {
        if self.read_only {
            return;
        }
        let source = self.current_document_source(cx);
        let selection = self.capture_source_selection_snapshot(cx).range;
        if let Some(edit) = command(&source, selection) {
            self.apply_source_edit(edit, cx);
        }
    }

    fn insert_block_snippet(&mut self, snippet: &str, caret: usize, cx: &mut Context<Self>) {
        let snippet = snippet.to_string();
        self.run_source_command(cx, |source, selection| {
            Some(insert_block(source, selection, &snippet, caret))
        });
    }

    pub(crate) fn on_increase_heading_level(
        &mut self,
        _: &IncreaseHeadingLevel,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_source_command(cx, |source, selection| {
            shift_heading_level(source, selection, true)
        });
    }

    pub(crate) fn on_decrease_heading_level(
        &mut self,
        _: &DecreaseHeadingLevel,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_source_command(cx, |source, selection| {
            shift_heading_level(source, selection, false)
        });
    }

    pub(crate) fn on_toggle_quote(
        &mut self,
        _: &ToggleQuote,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_source_command(cx, |source, selection| {
            toggle_line_style(source, selection, LineStyle::Quote)
        });
    }

    pub(crate) fn on_toggle_ordered_list(
        &mut self,
        _: &ToggleOrderedList,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_source_command(cx, |source, selection| {
            toggle_line_style(source, selection, LineStyle::OrderedList)
        });
    }

    pub(crate) fn on_toggle_bullet_list(
        &mut self,
        _: &ToggleBulletList,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_source_command(cx, |source, selection| {
            toggle_line_style(source, selection, LineStyle::BulletList)
        });
    }

    pub(crate) fn on_toggle_task_list(
        &mut self,
        _: &ToggleTaskList,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_source_command(cx, |source, selection| {
            toggle_line_style(source, selection, LineStyle::TaskList)
        });
    }

    pub(crate) fn on_toggle_task_status(
        &mut self,
        _: &ToggleTaskStatus,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_source_command(cx, toggle_task_status);
    }

    /// Opens the rows/columns dialog of the table insert menu for the
    /// focused block.
    pub(crate) fn on_insert_table(
        &mut self,
        _: &InsertTable,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only || self.view_mode != super::ViewMode::Rendered {
            return;
        }
        let target = self
            .active_entity_id
            .map(|id| TableInsertTarget::After(self.root_ancestor_entity_id(id)))
            .unwrap_or(TableInsertTarget::Append);
        self.dismiss_contextual_overlays(cx);
        self.table_insert_dialog = Some(TableInsertDialogState {
            target,
            body_rows: 2,
            columns: 2,
        });
        cx.notify();
    }

    pub(crate) fn on_insert_code_block(
        &mut self,
        _: &InsertCodeBlock,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_source_command(cx, |source, selection| {
            Some(insert_fenced_block(source, selection, "```"))
        });
    }

    pub(crate) fn on_insert_math_block(
        &mut self,
        _: &InsertMathBlock,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_source_command(cx, |source, selection| {
            Some(insert_fenced_block(source, selection, "$$"))
        });
    }

    pub(crate) fn on_insert_horizontal_rule(
        &mut self,
        _: &InsertHorizontalRule,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.insert_block_snippet("---", 3, cx);
    }

    pub(crate) fn on_insert_table_of_contents(
        &mut self,
        _: &InsertTableOfContents,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.insert_block_snippet("[TOC]", 5, cx);
    }

    pub(crate) fn on_insert_link_reference(
        &mut self,
        _: &InsertLinkReference,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The caret lands in the label: `[|]: `.
        self.insert_block_snippet("[]: ", 1, cx);
    }

    pub(crate) fn on_insert_footnote(
        &mut self,
        _: &InsertFootnote,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_source_command(cx, |source, selection| {
            Some(insert_footnote(source, selection))
        });
    }

    pub(crate) fn on_insert_front_matter(
        &mut self,
        _: &InsertFrontMatter,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_source_command(cx, |source, _| insert_front_matter(source));
    }

    pub(crate) fn on_insert_callout(
        &mut self,
        action: &InsertCallout,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let kind = action.kind.to_ascii_uppercase();
        if !CALLOUT_KINDS.contains(&action.kind.to_ascii_lowercase().as_str()) {
            return;
        }
        let snippet = format!("> [!{kind}]\n> ");
        let caret = snippet.len();
        self.insert_block_snippet(&snippet, caret, cx);
    }

    pub(crate) fn on_inline_math_selection(
        &mut self,
        _: &InlineMathSelection,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_source_command(cx, |source, selection| {
            Some(wrap_inline(source, selection, "$", "$"))
        });
    }

    pub(crate) fn on_comment_selection(
        &mut self,
        _: &CommentSelection,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_source_command(cx, |source, selection| {
            Some(wrap_inline(source, selection, "<!-- ", " -->"))
        });
    }

    /// Ctrl+K: links the selection, using a URL from the clipboard when it
    /// holds one.
    pub(crate) fn on_insert_link(
        &mut self,
        _: &InsertLink,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let url = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .map(|text| text.trim().to_string())
            .filter(|text| looks_like_url(text));
        self.run_source_command(cx, |source, selection| {
            Some(insert_link(source, selection, url.as_deref()))
        });
    }

    pub(crate) fn on_clear_format(
        &mut self,
        _: &ClearFormat,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_source_command(cx, clear_inline_markup);
    }
}

#[cfg(test)]
mod tests {
    use gpui::{AppContext as _, TestAppContext};

    use crate::editor::Editor;

    use super::LineStyle;

    fn editor_with(cx: &mut TestAppContext, markdown: &str) -> gpui::Entity<Editor> {
        let markdown = markdown.to_string();
        cx.new(|cx| Editor::from_markdown(cx, markdown, None))
    }

    #[gpui::test]
    async fn source_commands_rebuild_blocks_and_undo_in_one_step(cx: &mut TestAppContext) {
        let editor = editor_with(cx, "first\n\nsecond");
        editor.update(cx, |editor, cx| {
            let source = editor.current_document_source(cx);
            let edit = super::toggle_line_style(&source, 0..0, LineStyle::Quote).unwrap();
            editor.apply_source_edit(edit, cx);
            assert_eq!(editor.current_document_source(cx), "> first\n\nsecond");
            assert!(editor.document_dirty);

            let source = editor.current_document_source(cx);
            let edit = super::insert_block(&source, 0..0, "---", 3);
            editor.apply_source_edit(edit, cx);
            assert_eq!(
                editor.current_document_source(cx),
                "> first\n\n---\n\nsecond"
            );

            editor.undo_document(cx);
            assert_eq!(editor.current_document_source(cx), "> first\n\nsecond");
            editor.undo_document(cx);
            assert_eq!(editor.current_document_source(cx), "first\n\nsecond");
            editor.redo_document(cx);
            assert_eq!(editor.current_document_source(cx), "> first\n\nsecond");
        });
    }

    #[gpui::test]
    async fn read_only_documents_ignore_source_commands(cx: &mut TestAppContext) {
        let editor = editor_with(cx, "text");
        editor.update(cx, |editor, cx| {
            editor.read_only = true;
            let edit = super::insert_block("text", 0..0, "---", 3);
            editor.apply_source_edit(edit, cx);
            assert_eq!(editor.current_document_source(cx), "text");
        });
    }
}
