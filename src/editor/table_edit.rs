//! Native table runtime installation and table-editing operations.

use std::ops::RangeInclusive;

use super::*;

impl Editor {
    /// Creates a table block backed by the supplied serializable table data.
    pub(crate) fn new_table_block(cx: &mut Context<Self>, table: TableData) -> Entity<Block> {
        Self::new_block(cx, BlockRecord::table(table))
    }

    /// Creates editable header/body cell entities and binds them to a table block.
    pub(super) fn install_table_runtime_for_block(
        &mut self,
        table_block: &Entity<Block>,
        table: &TableData,
        cx: &mut Context<Self>,
    ) {
        let header = table
            .header
            .iter()
            .cloned()
            .enumerate()
            .map(|(column, title)| {
                let alignment = table
                    .alignments
                    .get(column)
                    .copied()
                    .unwrap_or(TableColumnAlignment::Default);
                let position = TableCellPosition { row: 0, column };
                let cell = Self::new_table_cell_block(cx, title, position, alignment);
                self.table_cells.insert(
                    cell.entity_id(),
                    TableCellBinding {
                        table_block: table_block.clone(),
                        cell: cell.clone(),
                        position,
                    },
                );
                cell
            })
            .collect::<Vec<_>>();

        let rows = table
            .rows
            .iter()
            .cloned()
            .enumerate()
            .map(|(body_row_index, row)| {
                row.into_iter()
                    .enumerate()
                    .map(|(column, title)| {
                        let alignment = table
                            .alignments
                            .get(column)
                            .copied()
                            .unwrap_or(TableColumnAlignment::Default);
                        let position = TableCellPosition {
                            row: body_row_index + 1,
                            column,
                        };
                        let cell = Self::new_table_cell_block(cx, title, position, alignment);
                        self.table_cells.insert(
                            cell.entity_id(),
                            TableCellBinding {
                                table_block: table_block.clone(),
                                cell: cell.clone(),
                                position,
                            },
                        );
                        cell
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        table_block.update(cx, {
            let runtime = TableRuntime { header, rows };
            move |block, _cx| block.set_table_runtime(runtime.clone())
        });
    }

    /// Recreates table cell runtimes and their lookup bindings for visible tables.
    pub(super) fn rebuild_table_runtimes(&mut self, cx: &mut Context<Self>) {
        self.table_cells.clear();
        self.table_axis_preview = None;
        let visible = self.document.visible_blocks().to_vec();
        for block in &visible {
            block
                .entity
                .update(cx, |block, _cx| block.clear_table_runtime());
        }
        for visible in visible {
            let Some(table) = visible.entity.read(cx).record.table.clone() else {
                continue;
            };
            if visible.entity.read(cx).kind() == BlockKind::Table {
                self.install_table_runtime_for_block(&visible.entity, &table, cx);
            }
        }
        self.rebuild_image_runtimes(cx);
        self.sync_table_axis_visuals(cx);
    }

    /// Copies live cell titles into the table's serializable record.
    pub(super) fn sync_table_record_from_runtime(
        &mut self,
        table_block: &Entity<Block>,
        cx: &mut Context<Self>,
    ) {
        let Some(runtime) = table_block.read(cx).table_runtime.clone() else {
            return;
        };
        let alignments = table_block
            .read(cx)
            .record
            .table
            .as_ref()
            .map(|table| table.alignments.clone())
            .unwrap_or_default();
        let header = runtime
            .header
            .iter()
            .map(|cell| cell.read(cx).record.title.clone())
            .collect::<Vec<_>>();
        let rows = runtime
            .rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| cell.read(cx).record.title.clone())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        table_block.update(cx, move |block, _cx| {
            block.record.table = Some(TableData {
                header,
                rows,
                alignments,
            });
        });
    }

    /// Adds a column using the last alignment and focuses its header cell.
    pub(super) fn append_table_column(
        &mut self,
        table_block: &Entity<Block>,
        cx: &mut Context<Self>,
    ) {
        self.sync_table_record_from_runtime(table_block, cx);

        let Some(mut table) = table_block.read(cx).record.table.clone() else {
            return;
        };
        let started_local_capture = if self.pending_undo_capture.is_none() {
            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            true
        } else {
            false
        };
        let alignment = table
            .alignments
            .last()
            .copied()
            .unwrap_or(TableColumnAlignment::Default);
        table.append_column(alignment);

        table_block.update(cx, move |block, _cx| {
            block.record.table = Some(table.clone());
        });
        self.rebuild_table_runtimes(cx);
        if let Some(cell) = table_block
            .read(cx)
            .table_runtime
            .as_ref()
            .and_then(|runtime| runtime.header.last())
        {
            self.focus_block(cell.entity_id());
        }
        self.mark_dirty(cx);
        self.request_active_block_scroll_into_view(cx);
        if started_local_capture {
            self.finalize_pending_undo_capture(cx);
        }
        cx.notify();
    }

    /// Adds a body row and focuses its first cell.
    pub(super) fn append_table_row(&mut self, table_block: &Entity<Block>, cx: &mut Context<Self>) {
        self.sync_table_record_from_runtime(table_block, cx);

        let Some(mut table) = table_block.read(cx).record.table.clone() else {
            return;
        };
        let started_local_capture = if self.pending_undo_capture.is_none() {
            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            true
        } else {
            false
        };
        table.append_row();

        table_block.update(cx, move |block, _cx| {
            block.record.table = Some(table.clone());
        });
        self.rebuild_table_runtimes(cx);
        if let Some(cell) = table_block
            .read(cx)
            .table_runtime
            .as_ref()
            .and_then(|runtime| runtime.rows.last())
            .and_then(|row| row.first())
        {
            self.focus_block(cell.entity_id());
        }
        self.mark_dirty(cx);
        self.request_active_block_scroll_into_view(cx);
        if started_local_capture {
            self.finalize_pending_undo_capture(cx);
        }
        cx.notify();
    }

    /// Inserts an empty body row before `body_index` and focuses its first cell.
    pub(super) fn insert_table_row(
        &mut self,
        table_block: &Entity<Block>,
        body_index: usize,
        cx: &mut Context<Self>,
    ) {
        self.sync_table_record_from_runtime(table_block, cx);
        let Some(mut table) = table_block.read(cx).record.table.clone() else {
            return;
        };
        let started_local_capture = if self.pending_undo_capture.is_none() {
            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            true
        } else {
            false
        };
        let body_index = body_index.min(table.rows.len());
        table.insert_row(body_index);
        table_block.update(cx, move |block, _cx| {
            block.record.table = Some(table.clone());
        });
        self.rebuild_table_runtimes(cx);
        self.set_table_axis_selection(None, cx);
        self.focus_table_cell_position(
            table_block,
            TableCellPosition {
                row: body_index + 1,
                column: 0,
            },
            cx,
        );
        self.mark_dirty(cx);
        self.request_active_block_scroll_into_view(cx);
        if started_local_capture {
            self.finalize_pending_undo_capture(cx);
        }
        cx.notify();
    }

    /// Inserts an empty column before `col_index` and focuses its header cell.
    pub(super) fn insert_table_column(
        &mut self,
        table_block: &Entity<Block>,
        col_index: usize,
        cx: &mut Context<Self>,
    ) {
        self.sync_table_record_from_runtime(table_block, cx);
        let Some(mut table) = table_block.read(cx).record.table.clone() else {
            return;
        };
        let started_local_capture = if self.pending_undo_capture.is_none() {
            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            true
        } else {
            false
        };
        let col_index = col_index.min(table.column_count());
        table.insert_column(col_index, TableColumnAlignment::Default);
        table_block.update(cx, move |block, _cx| {
            block.record.table = Some(table.clone());
        });
        self.rebuild_table_runtimes(cx);
        self.set_table_axis_selection(None, cx);
        self.focus_table_cell_position(
            table_block,
            TableCellPosition {
                row: 0,
                column: col_index,
            },
            cx,
        );
        self.mark_dirty(cx);
        self.request_active_block_scroll_into_view(cx);
        if started_local_capture {
            self.finalize_pending_undo_capture(cx);
        }
        cx.notify();
    }

    /// Updates the hovered row/column preview without clearing a newer hover.
    pub(super) fn preview_table_axis(
        &mut self,
        table_block_id: EntityId,
        kind: TableAxisKind,
        index: usize,
        hovered: bool,
        cx: &mut Context<Self>,
    ) {
        let marker = TableAxisSelection::single(table_block_id, kind, index);
        if hovered {
            self.table_axis_preview_hovers = if self.table_axis_preview == Some(marker) {
                self.table_axis_preview_hovers + 1
            } else {
                1
            };
            self.set_table_axis_preview(Some(marker), cx);
        } else if self.table_axis_preview == Some(marker) {
            // Only clear on a leave that still owns the preview. Adjacent
            // handles share one preview slot, and a leave can arrive after
            // the next handle's enter; clearing unconditionally would erase
            // the highlight the pointer just moved onto. A column's top-edge
            // strip and the band it reveals preview the same axis: showing
            // the band moves the table down so the pointer lands on it, and
            // the strip's leave must not hide the band again (that flickered).
            self.table_axis_preview_hovers = self.table_axis_preview_hovers.saturating_sub(1);
            if self.table_axis_preview_hovers == 0 {
                self.set_table_axis_preview(None, cx);
            }
        }
    }

    /// Selects a visual row or column and previews the same axis.
    pub(super) fn select_table_axis(
        &mut self,
        table_block_id: EntityId,
        kind: TableAxisKind,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        let selection = TableAxisSelection::single(table_block_id, kind, index);
        self.set_table_axis_preview(Some(selection), cx);
        self.set_table_axis_selection(Some(selection), cx);
    }

    /// Grows the selection from its anchor to `index` while the pointer is
    /// dragged over the handles, or on Shift-click. A Shift-click without a
    /// selection of this table and kind selects `index`; a drag without one
    /// does nothing (it started on a cell, not on a handle).
    pub(super) fn extend_table_axis(
        &mut self,
        table_block_id: EntityId,
        kind: TableAxisKind,
        index: usize,
        dragging: bool,
        cx: &mut Context<Self>,
    ) {
        let current = self.table_axis_selection.filter(|selection| {
            selection.table_block_id == table_block_id && selection.kind == kind
        });
        if current.is_none() && dragging {
            return;
        }
        let anchor = current.map(|selection| selection.anchor).unwrap_or(index);
        let selection = TableAxisSelection::spanning(table_block_id, kind, anchor, index);
        self.set_table_axis_preview(
            Some(TableAxisSelection::single(table_block_id, kind, index)),
            cx,
        );
        self.set_table_axis_selection(Some(selection), cx);
    }

    /// Selects an axis and opens its context menu at a window position.
    pub(super) fn open_table_axis_menu(
        &mut self,
        table_block_id: EntityId,
        kind: TableAxisKind,
        index: usize,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        // A right-click inside a selected range keeps the range, so its
        // commands apply to all selected rows or columns.
        let keeps_range = self.table_axis_selection.is_some_and(|selection| {
            selection.table_block_id == table_block_id
                && selection.kind == kind
                && selection.contains(index)
        });
        if !keeps_range {
            self.select_table_axis(table_block_id, kind, index, cx);
        }
        if let Some(selection) = self.table_axis_selection {
            self.open_table_axis_context_menu(position, selection, cx);
        }
    }

    /// Sets the alignment of `columns` and keeps them selected, focusing the
    /// first one's header cell.
    pub(super) fn set_table_column_alignment(
        &mut self,
        table_block: &Entity<Block>,
        columns: RangeInclusive<usize>,
        alignment: TableColumnAlignment,
        cx: &mut Context<Self>,
    ) {
        self.sync_table_record_from_runtime(table_block, cx);
        let Some(mut table) = table_block.read(cx).record.table.clone() else {
            return;
        };
        let started_local_capture = if self.pending_undo_capture.is_none() {
            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            true
        } else {
            false
        };
        let (first, last) = (*columns.start(), *columns.end());
        for column in columns {
            table.set_column_alignment(column, alignment);
        }
        table_block.update(cx, move |block, _cx| {
            block.record.table = Some(table.clone());
        });
        self.rebuild_table_runtimes(cx);
        let selection = TableAxisSelection::spanning(
            table_block.entity_id(),
            TableAxisKind::Column,
            first,
            last,
        );
        self.set_table_axis_selection(Some(selection), cx);
        self.focus_table_cell_position(
            table_block,
            TableCellPosition {
                row: 0,
                column: first,
            },
            cx,
        );
        self.mark_dirty(cx);
        self.request_active_block_scroll_into_view(cx);
        if started_local_capture {
            self.finalize_pending_undo_capture(cx);
        }
        cx.notify();
    }

    /// Moves the visual rows `rows` (row zero is the header) one step up
    /// (`delta < 0`) or down, keeping them selected.
    pub(super) fn move_table_row(
        &mut self,
        table_block: &Entity<Block>,
        rows: RangeInclusive<usize>,
        delta: i32,
        cx: &mut Context<Self>,
    ) {
        self.sync_table_record_from_runtime(table_block, cx);
        let Some(mut table) = table_block.read(cx).record.table.clone() else {
            return;
        };
        let (first, last) = (*rows.start(), *rows.end());
        // Visual rows are the header (0) plus every body row, so the last valid
        // index is `rows.len()`.
        let moves_up = delta < 0;
        if (moves_up && first == 0) || (!moves_up && last >= table.rows.len()) {
            return;
        }
        let started_local_capture = if self.pending_undo_capture.is_none() {
            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            true
        } else {
            false
        };
        // Moving the block by one is the neighbour walking across it.
        if moves_up {
            for row in first..=last {
                table.swap_visual_rows(row - 1, row);
            }
        } else {
            for row in (first..=last).rev() {
                table.swap_visual_rows(row, row + 1);
            }
        }
        let (first, last) = if moves_up {
            (first - 1, last - 1)
        } else {
            (first + 1, last + 1)
        };
        table_block.update(cx, move |block, _cx| {
            block.record.table = Some(table.clone());
        });
        self.rebuild_table_runtimes(cx);
        let selection =
            TableAxisSelection::spanning(table_block.entity_id(), TableAxisKind::Row, first, last);
        self.set_table_axis_selection(Some(selection), cx);
        self.focus_table_cell_position(
            table_block,
            TableCellPosition {
                row: first,
                column: 0,
            },
            cx,
        );
        self.mark_dirty(cx);
        self.request_active_block_scroll_into_view(cx);
        if started_local_capture {
            self.finalize_pending_undo_capture(cx);
        }
        cx.notify();
    }

    /// Moves `columns` one step left (`delta < 0`) or right, keeping them
    /// selected and following the first one's header cell.
    pub(super) fn move_table_column(
        &mut self,
        table_block: &Entity<Block>,
        columns: RangeInclusive<usize>,
        delta: i32,
        cx: &mut Context<Self>,
    ) {
        self.sync_table_record_from_runtime(table_block, cx);
        let Some(mut table) = table_block.read(cx).record.table.clone() else {
            return;
        };
        let (first, last) = (*columns.start(), *columns.end());
        let moves_left = delta < 0;
        if (moves_left && first == 0) || (!moves_left && last + 1 >= table.column_count()) {
            return;
        }
        let started_local_capture = if self.pending_undo_capture.is_none() {
            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            true
        } else {
            false
        };
        if moves_left {
            for column in first..=last {
                table.swap_columns(column - 1, column);
            }
        } else {
            for column in (first..=last).rev() {
                table.swap_columns(column, column + 1);
            }
        }
        let (first, last) = if moves_left {
            (first - 1, last - 1)
        } else {
            (first + 1, last + 1)
        };
        table_block.update(cx, move |block, _cx| {
            block.record.table = Some(table.clone());
        });
        self.rebuild_table_runtimes(cx);
        let selection = TableAxisSelection::spanning(
            table_block.entity_id(),
            TableAxisKind::Column,
            first,
            last,
        );
        self.set_table_axis_selection(Some(selection), cx);
        self.focus_table_cell_position(
            table_block,
            TableCellPosition {
                row: 0,
                column: first,
            },
            cx,
        );
        self.mark_dirty(cx);
        self.request_active_block_scroll_into_view(cx);
        if started_local_capture {
            self.finalize_pending_undo_capture(cx);
        }
        cx.notify();
    }

    /// Removes a zero-based body row and focuses the nearest surviving visual row.
    pub(super) fn delete_table_row(
        &mut self,
        table_block: &Entity<Block>,
        row_index: usize,
        cx: &mut Context<Self>,
    ) {
        self.sync_table_record_from_runtime(table_block, cx);
        let Some(mut table) = table_block.read(cx).record.table.clone() else {
            return;
        };
        if row_index >= table.rows.len() {
            return;
        }
        let started_local_capture = if self.pending_undo_capture.is_none() {
            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            true
        } else {
            false
        };
        table.remove_body_row(row_index);
        let remaining_body_rows = table.rows.len();
        table_block.update(cx, move |block, _cx| {
            block.record.table = Some(table.clone());
        });
        self.rebuild_table_runtimes(cx);
        // Row selections are addressed by visual index, where the first body row
        // is `1` (the header is `0`). With no body rows left, fall back to the
        // header so focus lands on a cell that still exists.
        let focus_visual_row = if remaining_body_rows == 0 {
            0
        } else {
            row_index.min(remaining_body_rows - 1) + 1
        };
        if remaining_body_rows == 0 {
            self.clear_table_axis_selection(cx);
        } else {
            self.set_table_axis_selection(
                Some(TableAxisSelection::single(
                    table_block.entity_id(),
                    TableAxisKind::Row,
                    focus_visual_row,
                )),
                cx,
            );
        }
        self.focus_table_cell_position(
            table_block,
            TableCellPosition {
                row: focus_visual_row,
                column: 0,
            },
            cx,
        );
        self.mark_dirty(cx);
        self.request_active_block_scroll_into_view(cx);
        if started_local_capture {
            self.finalize_pending_undo_capture(cx);
        }
        cx.notify();
    }

    /// Promotes the first body row to header; does nothing without a body row.
    pub(super) fn delete_table_header_row(
        &mut self,
        table_block: &Entity<Block>,
        cx: &mut Context<Self>,
    ) {
        self.sync_table_record_from_runtime(table_block, cx);
        let Some(mut table) = table_block.read(cx).record.table.clone() else {
            return;
        };
        // The first body row is promoted into the header, so there must be at
        // least one body row to delete the header.
        if table.rows.is_empty() {
            return;
        }
        let started_local_capture = if self.pending_undo_capture.is_none() {
            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            true
        } else {
            false
        };
        table.remove_header_row();
        table_block.update(cx, move |block, _cx| {
            block.record.table = Some(table.clone());
        });
        self.rebuild_table_runtimes(cx);
        self.clear_table_axis_selection(cx);
        self.focus_table_cell_position(table_block, TableCellPosition { row: 0, column: 0 }, cx);
        self.mark_dirty(cx);
        self.request_active_block_scroll_into_view(cx);
        if started_local_capture {
            self.finalize_pending_undo_capture(cx);
        }
        cx.notify();
    }

    /// Removes a column only when at least one other column remains.
    pub(super) fn delete_table_column(
        &mut self,
        table_block: &Entity<Block>,
        column: usize,
        cx: &mut Context<Self>,
    ) {
        self.sync_table_record_from_runtime(table_block, cx);
        let Some(mut table) = table_block.read(cx).record.table.clone() else {
            return;
        };
        if table.column_count() <= 1 || column >= table.column_count() {
            return;
        }
        let started_local_capture = if self.pending_undo_capture.is_none() {
            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            true
        } else {
            false
        };
        table.remove_column(column);
        let focus_column = column.min(table.column_count().saturating_sub(1));
        table_block.update(cx, move |block, _cx| {
            block.record.table = Some(table.clone());
        });
        self.rebuild_table_runtimes(cx);
        let selection = TableAxisSelection::single(
            table_block.entity_id(),
            TableAxisKind::Column,
            focus_column,
        );
        self.set_table_axis_selection(Some(selection), cx);
        self.focus_table_cell_position(
            table_block,
            TableCellPosition {
                row: 0,
                column: focus_column,
            },
            cx,
        );
        self.mark_dirty(cx);
        self.request_active_block_scroll_into_view(cx);
        if started_local_capture {
            self.finalize_pending_undo_capture(cx);
        }
        cx.notify();
    }

    /// Deletes the columns `columns`, or the whole table when they are all
    /// of its columns, and selects the column that takes their place.
    pub(super) fn delete_table_columns(
        &mut self,
        table_block: &Entity<Block>,
        columns: RangeInclusive<usize>,
        cx: &mut Context<Self>,
    ) {
        let column_count = table_block
            .read(cx)
            .record
            .table
            .as_ref()
            .map_or(0, |table| table.column_count());
        let (first, last) = (*columns.start(), *columns.end());
        if last >= column_count {
            return;
        }
        if first == 0 && last + 1 == column_count {
            self.remove_table_block(table_block, cx);
            return;
        }
        let started_local_capture = if self.pending_undo_capture.is_none() {
            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            true
        } else {
            false
        };
        for column in (first..=last).rev() {
            self.delete_table_column(table_block, column, cx);
        }
        if started_local_capture {
            self.finalize_pending_undo_capture(cx);
        }
    }

    /// Deletes the visual rows `rows` (row zero is the header, whose place the
    /// first remaining body row takes), or the whole table when they are all
    /// of its rows.
    pub(super) fn delete_table_rows(
        &mut self,
        table_block: &Entity<Block>,
        rows: RangeInclusive<usize>,
        cx: &mut Context<Self>,
    ) {
        let body_rows = table_block
            .read(cx)
            .record
            .table
            .as_ref()
            .map_or(0, |table| table.rows.len());
        let (first, last) = (*rows.start(), *rows.end());
        if last > body_rows {
            return;
        }
        if first == 0 && last == body_rows {
            self.remove_table_block(table_block, cx);
            return;
        }
        let started_local_capture = if self.pending_undo_capture.is_none() {
            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            true
        } else {
            false
        };
        // Body rows from the bottom up keep the remaining indices valid; the
        // header goes last, once a body row is left to replace it.
        for row in (first.max(1)..=last).rev() {
            self.delete_table_row(table_block, row - 1, cx);
        }
        if first == 0 {
            self.delete_table_header_row(table_block, cx);
        }
        if started_local_capture {
            self.finalize_pending_undo_capture(cx);
        }
    }

    /// Removes the table block entirely, leaving an empty paragraph in its place
    /// so the caret has somewhere to land. Used when deleting the last remaining
    /// row or column, which empties the table.
    pub(super) fn remove_table_block(
        &mut self,
        table_block: &Entity<Block>,
        cx: &mut Context<Self>,
    ) {
        let Some(location) = self.document.find_block_location(table_block.entity_id()) else {
            return;
        };
        let started_local_capture = if self.pending_undo_capture.is_none() {
            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            true
        } else {
            false
        };
        // Insert the replacement paragraph after the table first, then remove the
        // table, so the document is never momentarily empty.
        let paragraph = Self::new_block(cx, BlockRecord::paragraph(String::new()));
        self.document.insert_blocks_at(
            location.parent.clone(),
            location.index + 1,
            vec![paragraph.clone()],
            cx,
        );
        let table_id = table_block.entity_id();
        self.document.with_structure_mutation(cx, |document, cx| {
            let _ = document.remove_block_by_id_raw(table_id, cx);
        });
        self.rebuild_table_runtimes(cx);
        self.clear_table_axis_selection(cx);
        self.focus_block(paragraph.entity_id());
        self.mark_dirty(cx);
        self.request_active_block_scroll_into_view(cx);
        if started_local_capture {
            self.finalize_pending_undo_capture(cx);
        }
        cx.notify();
    }

    /// Converts an axis selection to the marker stored on a table block.
    pub(super) fn table_axis_marker(selection: TableAxisSelection) -> TableAxisMarker {
        TableAxisMarker {
            kind: selection.kind,
            index: selection.index,
            end: selection.end,
        }
    }

    /// Clears the hovered axis and updates table visuals if it was present.
    pub(super) fn clear_table_axis_preview(&mut self, cx: &mut Context<Self>) {
        if self.table_axis_preview.take().is_some() {
            self.sync_table_axis_visuals(cx);
        }
    }

    /// Clears the selected axis and updates table visuals if it was present.
    pub(super) fn clear_table_axis_selection(&mut self, cx: &mut Context<Self>) {
        if self.table_axis_selection.take().is_some() {
            self.sync_table_axis_visuals(cx);
        }
    }

    /// Changes the hovered axis and refreshes table visuals when needed.
    pub(super) fn set_table_axis_preview(
        &mut self,
        preview: Option<TableAxisSelection>,
        cx: &mut Context<Self>,
    ) {
        if self.table_axis_preview != preview {
            self.table_axis_preview = preview;
            self.sync_table_axis_visuals(cx);
        }
    }

    /// Changes the selected axis and refreshes table visuals when needed.
    pub(super) fn set_table_axis_selection(
        &mut self,
        selection: Option<TableAxisSelection>,
        cx: &mut Context<Self>,
    ) {
        if self.table_axis_selection != selection {
            self.table_axis_selection = selection;
            self.sync_table_axis_visuals(cx);
        }
    }

    /// Checks that an axis index still addresses a live row or column.
    pub(super) fn table_axis_selection_valid(
        &self,
        selection: TableAxisSelection,
        cx: &App,
    ) -> bool {
        let Some(table_block) = self.table_block_by_id(selection.table_block_id, cx) else {
            return false;
        };
        let Some(runtime) = table_block.read(cx).table_runtime.as_ref() else {
            return false;
        };
        match selection.kind {
            TableAxisKind::Column => selection.end < runtime.header.len(),
            // Visual row index: `0` is the header, `1..=rows.len()` the body.
            TableAxisKind::Row => selection.end <= runtime.rows.len(),
        }
    }

    /// Drops preview/selection markers invalidated by table changes.
    pub(super) fn normalize_table_axis_state(&mut self, cx: &mut Context<Self>) {
        if let Some(selection) = self.table_axis_selection
            && !self.table_axis_selection_valid(selection, cx)
        {
            self.table_axis_selection = None;
        }
        if let Some(preview) = self.table_axis_preview
            && !self.table_axis_selection_valid(preview, cx)
        {
            self.table_axis_preview = None;
        }
    }

    /// Propagates axis preview and selection to visible tables and cell runtimes.
    pub(super) fn sync_table_axis_visuals(&mut self, cx: &mut Context<Self>) {
        self.normalize_table_axis_state(cx);

        let visible_tables = self
            .document
            .flatten_visible_blocks()
            .into_iter()
            .filter(|visible| visible.entity.read(cx).kind() == BlockKind::Table)
            .map(|visible| visible.entity)
            .collect::<Vec<_>>();

        for table_block in &visible_tables {
            let block_id = table_block.entity_id();
            let preview_marker = self
                .table_axis_preview
                .filter(|selection| selection.table_block_id == block_id)
                .map(Self::table_axis_marker);
            let selected_marker = self
                .table_axis_selection
                .filter(|selection| selection.table_block_id == block_id)
                .map(Self::table_axis_marker);

            table_block.update(cx, move |block, cx| {
                block.set_table_axis_visual_state(preview_marker, selected_marker);
                cx.notify();
            });

            let Some(runtime) = table_block.read(cx).table_runtime.clone() else {
                continue;
            };

            let selected = self
                .table_axis_selection
                .filter(|selection| selection.table_block_id == block_id);
            let preview = self
                .table_axis_preview
                .filter(|selection| selection.table_block_id == block_id);

            // `row` is the visual row index: `0` is the header and body rows
            // follow at `1..`, matching how row selections are addressed.
            let mut apply_highlight = |cell: &Entity<Block>, row: usize, column: usize| {
                let highlight = if selected.is_some_and(|selection| match selection.kind {
                    TableAxisKind::Column => selection.contains(column),
                    TableAxisKind::Row => selection.contains(row),
                }) {
                    TableAxisHighlight::Selected
                } else if preview.is_some_and(|selection| match selection.kind {
                    TableAxisKind::Column => selection.contains(column),
                    TableAxisKind::Row => selection.contains(row),
                }) {
                    TableAxisHighlight::Preview
                } else {
                    TableAxisHighlight::None
                };

                cell.update(cx, move |block, cx| {
                    block.set_table_axis_highlight(highlight);
                    cx.notify();
                });
            };

            for (column, cell) in runtime.header.iter().enumerate() {
                apply_highlight(cell, 0, column);
            }
            for (body_row_index, row) in runtime.rows.iter().enumerate() {
                for (column, cell) in row.iter().enumerate() {
                    apply_highlight(cell, body_row_index + 1, column);
                }
            }
        }
    }
}
