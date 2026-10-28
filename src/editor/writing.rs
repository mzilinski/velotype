//! Writing aids: read-only mode, focus mode and typewriter scrolling.

use gpui::*;

use super::Editor;
use crate::config::EditorSettings;

/// Opacity of blocks outside the edited one while focus mode is on.
pub(super) const FOCUS_MODE_DIMMED_OPACITY: f32 = 0.3;

impl Editor {
    pub(crate) fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Toggles read-only mode for this window. Blocks learn the flag through
    /// their runtime context, which rejects every text edit; editor-level
    /// structural edits are swallowed by [`Self::swallow_edit_when_read_only`].
    pub(crate) fn toggle_read_only(&mut self, cx: &mut Context<Self>) {
        self.read_only = !self.read_only;
        self.dismiss_contextual_overlays(cx);
        self.rebuild_image_runtimes(cx);
        cx.notify();
    }

    /// Capture-phase guard registered for every editing action: in read-only
    /// mode the action never reaches the focused block.
    pub(super) fn swallow_edit_when_read_only<A: Action>(
        &mut self,
        _: &A,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only {
            cx.stop_propagation();
        }
    }

    /// Visible index of the block that should stay fully opaque in focus mode
    /// (table cells resolve to their table).
    pub(super) fn focus_mode_visible_index(&self, window: &Window, cx: &App) -> Option<usize> {
        if !EditorSettings::focus_mode(cx) || self.view_mode != super::ViewMode::Rendered {
            return None;
        }
        let id = self.focused_edit_target_entity_id(window, cx)?;
        self.document.visible_index_for_entity_id(id).or_else(|| {
            self.table_cell_binding(id).and_then(|binding| {
                self.document
                    .visible_index_for_entity_id(binding.table_block.entity_id())
            })
        })
    }

    /// Typewriter mode: scroll so the caret sits at the vertical center of the
    /// viewport. Returns `false` when there is no laid-out caret yet.
    pub(super) fn center_caret_for_typewriter(&mut self, window: &Window, cx: &App) -> bool {
        let Some(block) = self.focused_edit_target(window, cx) else {
            return false;
        };
        let Some(caret) = block.read(cx).active_range_or_cursor_bounds() else {
            return false;
        };
        let viewport = self.scroll_handle.bounds();
        let target_center = viewport.top() + viewport.size.height / 2.0;
        let caret_center = caret.top() + caret.size.height / 2.0;
        let delta = target_center - caret_center;
        if delta.abs() < px(1.0) {
            return true;
        }
        let mut offset = self.scroll_handle.offset();
        let max_offset_y = self.scroll_handle.max_offset().y.max(px(0.0));
        offset.y = (offset.y + delta).min(px(0.0)).max(-max_offset_y);
        self.scroll_handle.set_offset(offset);
        true
    }
}

impl Editor {
    /// Re-centers the caret in typewriter mode whenever it moved or the text
    /// changed since the last centering. Returns `true` when it took over this
    /// frame's scrolling (the caller then schedules one settle frame).
    pub(super) fn apply_typewriter_scroll(&mut self, window: &Window, cx: &App) -> bool {
        if !EditorSettings::typewriter_mode(cx) {
            self.typewriter_last_caret = None;
            return false;
        }
        let Some(block) = self.focused_edit_target(window, cx) else {
            return false;
        };
        let key = {
            let block_ref = block.read(cx);
            (
                block.entity_id(),
                block_ref.selected_range.clone(),
                block_ref.display_text().len(),
            )
        };
        if self.typewriter_last_caret.as_ref() == Some(&key) {
            return false;
        }
        // Only remember the caret once it could be centered, so a block that
        // has not been laid out yet is retried on the next frame.
        if self.center_caret_for_typewriter(window, cx) {
            self.typewriter_last_caret = Some(key);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use gpui::{AppContext as _, TestAppContext};

    use crate::editor::Editor;

    #[gpui::test]
    async fn read_only_reaches_every_block_and_table_cell(cx: &mut TestAppContext) {
        let markdown = "# Title\n\ntext\n\n| A | B |\n| --- | --- |\n| 1 | 2 |".to_string();
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

        editor.update(cx, |editor, cx| {
            editor.toggle_read_only(cx);
            assert!(editor.is_read_only());
            for visible in editor.document.visible_blocks() {
                let block = visible.entity.read(cx);
                assert!(block.read_only);
                if let Some(runtime) = block.table_runtime.as_ref() {
                    assert!(runtime.header.iter().all(|cell| cell.read(cx).read_only));
                    assert!(
                        runtime
                            .rows
                            .iter()
                            .flatten()
                            .all(|cell| cell.read(cx).read_only)
                    );
                }
            }
            editor.toggle_read_only(cx);
            assert!(
                editor
                    .document
                    .visible_blocks()
                    .iter()
                    .all(|visible| !visible.entity.read(cx).read_only)
            );
        });
    }
}
