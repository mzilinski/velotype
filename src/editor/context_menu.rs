//! Rendered-mode context menus and native table insertion dialog.

use gpui::*;

use super::{Editor, TableAxisSelection, ViewMode};
use crate::components::{DismissTransientUi, TableAxisKind, TableColumnAlignment, TableData};
use crate::i18n::I18nManager;
use crate::theme::Theme;

/// Target block position for inserting a native table.
#[derive(Clone, Copy)]
pub(super) enum TableInsertTarget {
    /// Insert the table immediately after the referenced block.
    After(EntityId),
    /// Append the table to the end of the current root list.
    Append,
}

/// Rendered-mode context menu currently open in the editor.
pub(super) enum ContextMenuState {
    /// The document context menu (see `context_menu_general.rs`).
    General {
        position: Point<Pixels>,
        /// Where Insert > Table puts the table.
        target: TableInsertTarget,
        /// The block that was right-clicked.
        block: Option<EntityId>,
        entries: std::rc::Rc<Vec<super::context_menu_general::ContextEntry>>,
        /// Indices of the open submenus, from the top level down.
        open_path: Vec<usize>,
    },
    /// Spelling suggestions for the misspelled word under the pointer.
    Spelling(super::spelling::SpellingMenuState),
    /// Table row or column context menu for an existing native table.
    TableAxis {
        position: Point<Pixels>,
        selection: TableAxisSelection,
    },
    /// Zoom levels for a rendered standalone image block.
    Image {
        position: Point<Pixels>,
        entity_id: EntityId,
    },
}

/// State for the table insertion dialog opened from the context menu.
pub(super) struct TableInsertDialogState {
    pub target: TableInsertTarget,
    pub body_rows: usize,
    pub columns: usize,
}

impl Editor {
    pub(super) fn root_ancestor_entity_id(&self, entity_id: EntityId) -> EntityId {
        let mut current = entity_id;
        while let Some(location) = self.document.find_block_location(current) {
            let Some(parent) = location.parent else {
                break;
            };
            current = parent.entity_id();
        }
        current
    }

    pub(super) fn open_table_axis_context_menu(
        &mut self,
        position: Point<Pixels>,
        selection: TableAxisSelection,
        cx: &mut Context<Self>,
    ) {
        if self.view_mode != ViewMode::Rendered || self.read_only {
            return;
        }

        self.close_menu_bar(cx);
        self.context_menu_submenu_close_task = None;
        self.context_menu = Some(ContextMenuState::TableAxis {
            position,
            selection,
        });
        cx.notify();
    }

    pub(super) fn close_table_insert_dialog(&mut self, cx: &mut Context<Self>) {
        if self.table_insert_dialog.take().is_some() {
            cx.notify();
        }
    }

    fn close_context_menu(&mut self, cx: &mut Context<Self>) {
        let had_menu = self.context_menu.take().is_some();
        let had_submenu_close = self.context_menu_submenu_close_task.take().is_some();
        if had_menu || had_submenu_close {
            cx.notify();
        }
    }

    pub(super) fn dismiss_contextual_overlays(&mut self, cx: &mut Context<Self>) {
        let had_menu = self.context_menu.take().is_some();
        let had_dialog = self.table_insert_dialog.take().is_some();
        let had_submenu_close = self.context_menu_submenu_close_task.take().is_some();
        if had_menu || had_dialog || had_submenu_close {
            cx.notify();
        }
    }

    pub(super) fn on_editor_context_menu_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.view_mode != ViewMode::Rendered {
            return;
        }
        cx.stop_propagation();
        self.open_general_context_menu(event.position, TableInsertTarget::Append, None, cx);
    }

    pub(super) fn on_block_context_menu_mouse_down(
        &mut self,
        entity_id: EntityId,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.view_mode != ViewMode::Rendered {
            return;
        }
        cx.stop_propagation();
        // A misspelled word under the pointer takes precedence over inserting.
        if self.open_spelling_menu_at(entity_id, event.position, cx) {
            return;
        }
        if !self.read_only && self.zoomable_image(entity_id, cx).is_some() {
            self.close_menu_bar(cx);
            self.context_menu_submenu_close_task = None;
            self.context_menu = Some(ContextMenuState::Image {
                position: event.position,
                entity_id,
            });
            cx.notify();
            return;
        }
        let target = TableInsertTarget::After(self.root_ancestor_entity_id(entity_id));
        self.open_general_context_menu(event.position, target, Some(entity_id), cx);
    }

    pub(super) fn on_dismiss_context_menu_overlay(
        &mut self,
        _event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dismiss_contextual_overlays(cx);
    }

    pub(super) fn on_dismiss_transient_ui(
        &mut self,
        _: &DismissTransientUi,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.close_palette(cx) {
            return;
        }
        let had_overlay = self.context_menu.is_some() || self.table_insert_dialog.is_some();
        self.dismiss_contextual_overlays(cx);
        if !had_overlay {
            self.close_find_bar(window, cx);
        }
    }

    pub(super) fn on_table_rows_decrement(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(dialog) = self.table_insert_dialog.as_mut() {
            dialog.body_rows = dialog.body_rows.saturating_sub(1).max(1);
            cx.notify();
        }
    }

    pub(super) fn on_table_rows_increment(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(dialog) = self.table_insert_dialog.as_mut() {
            dialog.body_rows += 1;
            cx.notify();
        }
    }

    pub(super) fn on_table_columns_decrement(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(dialog) = self.table_insert_dialog.as_mut() {
            dialog.columns = dialog.columns.saturating_sub(1).max(1);
            cx.notify();
        }
    }

    pub(super) fn on_table_columns_increment(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(dialog) = self.table_insert_dialog.as_mut() {
            dialog.columns += 1;
            cx.notify();
        }
    }

    pub(super) fn on_cancel_table_insert_dialog(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_table_insert_dialog(cx);
    }

    pub(super) fn on_confirm_table_insert_dialog(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(dialog) = self.table_insert_dialog.take() else {
            return;
        };

        let table = TableData::new_empty(dialog.body_rows, dialog.columns);
        let new_block = Self::new_table_block(cx, table);

        match dialog.target {
            TableInsertTarget::After(entity_id) => {
                if let Some(location) = self.document.find_block_location(entity_id) {
                    self.document.insert_blocks_at(
                        location.parent,
                        location.index + 1,
                        vec![new_block.clone()],
                        cx,
                    );
                } else {
                    self.document.insert_blocks_at(
                        None,
                        self.document.root_count(),
                        vec![new_block.clone()],
                        cx,
                    );
                }
            }
            TableInsertTarget::Append => {
                self.document.insert_blocks_at(
                    None,
                    self.document.root_count(),
                    vec![new_block.clone()],
                    cx,
                );
            }
        }

        // A table inserted as the last block in its container leaves no line
        // below it, so in rendered mode the caret cannot move past the table.
        // Add a trailing empty paragraph to land on when nothing follows it.
        self.ensure_trailing_paragraph_after_structural(&new_block, cx);

        self.rebuild_table_runtimes(cx);
        if let Some(first_cell) = new_block
            .read(cx)
            .table_runtime
            .as_ref()
            .and_then(|runtime| runtime.header.first())
        {
            self.focus_block(first_cell.entity_id());
        }
        self.mark_dirty(cx);
        self.request_active_block_scroll_into_view(cx);
        cx.notify();
    }

    fn active_axis_menu_selection(&self) -> Option<TableAxisSelection> {
        match self.context_menu.as_ref() {
            Some(ContextMenuState::TableAxis { selection, .. }) => Some(*selection),
            _ => None,
        }
    }

    fn on_apply_column_alignment(
        &mut self,
        alignment: TableColumnAlignment,
        cx: &mut Context<Self>,
    ) {
        let Some(selection) = self.active_axis_menu_selection() else {
            return;
        };
        if selection.kind != TableAxisKind::Column {
            return;
        }
        let Some(table_block) = self.table_block_by_id(selection.table_block_id, cx) else {
            return;
        };
        self.close_context_menu(cx);
        self.set_table_column_alignment(&table_block, selection.index, alignment, cx);
    }

    pub(super) fn on_align_table_column_left(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Left is the default, so emit the unmarked `---` form rather than an
        // explicit `:---`; an explicit colon is only kept when the source had
        // one. This keeps the menu's output unchanged from before.
        self.on_apply_column_alignment(TableColumnAlignment::Default, cx);
    }

    pub(super) fn on_align_table_column_center(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.on_apply_column_alignment(TableColumnAlignment::Center, cx);
    }

    pub(super) fn on_align_table_column_right(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.on_apply_column_alignment(TableColumnAlignment::Right, cx);
    }

    /// Resolves the table targeted by the open axis menu when it matches `kind`.
    fn axis_menu_table(
        &self,
        kind: TableAxisKind,
        cx: &App,
    ) -> Option<(TableAxisSelection, Entity<crate::components::Block>)> {
        let selection = self.active_axis_menu_selection()?;
        if selection.kind != kind {
            return None;
        }
        let table_block = self.table_block_by_id(selection.table_block_id, cx)?;
        Some((selection, table_block))
    }

    pub(super) fn on_insert_table_row_above(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Markdown tables have exactly one header row, so nothing can be
        // inserted above it (visual index 0).
        let Some((selection, table_block)) = self.axis_menu_table(TableAxisKind::Row, cx) else {
            return;
        };
        if selection.index == 0 {
            return;
        }
        self.close_context_menu(cx);
        self.insert_table_row(&table_block, selection.index - 1, cx);
    }

    pub(super) fn on_insert_table_row_below(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((selection, table_block)) = self.axis_menu_table(TableAxisKind::Row, cx) else {
            return;
        };
        self.close_context_menu(cx);
        // Visual row `n` is body row `n - 1`, so "below" is body index `n`.
        self.insert_table_row(&table_block, selection.index, cx);
    }

    pub(super) fn on_insert_table_column_left(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((selection, table_block)) = self.axis_menu_table(TableAxisKind::Column, cx) else {
            return;
        };
        self.close_context_menu(cx);
        self.insert_table_column(&table_block, selection.index, cx);
    }

    pub(super) fn on_insert_table_column_right(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((selection, table_block)) = self.axis_menu_table(TableAxisKind::Column, cx) else {
            return;
        };
        self.close_context_menu(cx);
        self.insert_table_column(&table_block, selection.index + 1, cx);
    }

    pub(super) fn on_move_table_row_up(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(selection) = self.active_axis_menu_selection() else {
            return;
        };
        if selection.kind != TableAxisKind::Row || selection.index == 0 {
            return;
        }
        let Some(table_block) = self.table_block_by_id(selection.table_block_id, cx) else {
            return;
        };
        self.close_context_menu(cx);
        self.move_table_row(&table_block, selection.index, -1, cx);
    }

    pub(super) fn on_move_table_row_down(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(selection) = self.active_axis_menu_selection() else {
            return;
        };
        if selection.kind != TableAxisKind::Row {
            return;
        }
        let Some(table_block) = self.table_block_by_id(selection.table_block_id, cx) else {
            return;
        };
        let can_move = table_block
            .read(cx)
            .record
            .table
            .as_ref()
            .map(|table| selection.index < table.rows.len())
            .unwrap_or(false);
        if !can_move {
            return;
        }
        self.close_context_menu(cx);
        self.move_table_row(&table_block, selection.index, 1, cx);
    }

    pub(super) fn on_move_table_column_left(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(selection) = self.active_axis_menu_selection() else {
            return;
        };
        if selection.kind != TableAxisKind::Column || selection.index == 0 {
            return;
        }
        let Some(table_block) = self.table_block_by_id(selection.table_block_id, cx) else {
            return;
        };
        self.close_context_menu(cx);
        self.move_table_column(&table_block, selection.index, -1, cx);
    }

    pub(super) fn on_move_table_column_right(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(selection) = self.active_axis_menu_selection() else {
            return;
        };
        if selection.kind != TableAxisKind::Column {
            return;
        }
        let Some(table_block) = self.table_block_by_id(selection.table_block_id, cx) else {
            return;
        };
        let can_move = table_block
            .read(cx)
            .record
            .table
            .as_ref()
            .map(|table| selection.index + 1 < table.column_count())
            .unwrap_or(false);
        if !can_move {
            return;
        }
        self.close_context_menu(cx);
        self.move_table_column(&table_block, selection.index, 1, cx);
    }

    pub(super) fn on_delete_table_row(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(selection) = self.active_axis_menu_selection() else {
            return;
        };
        if selection.kind != TableAxisKind::Row {
            return;
        }
        let Some(table_block) = self.table_block_by_id(selection.table_block_id, cx) else {
            return;
        };
        let row_count = table_block
            .read(cx)
            .record
            .table
            .as_ref()
            .map(|table| table.rows.len());
        self.close_context_menu(cx);
        // Visual index 0 is the header: deleting it promotes the first body row,
        // unless there is no body row left, in which case it was the table's last
        // row and the whole table is removed.
        if selection.index == 0 {
            if row_count == Some(0) {
                self.remove_table_block(&table_block, cx);
            } else {
                self.delete_table_header_row(&table_block, cx);
            }
        } else {
            self.delete_table_row(&table_block, selection.index - 1, cx);
        }
    }

    pub(super) fn on_toggle_table_headers(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let next = !crate::config::EditorSettings::show_table_headers(cx);
        crate::config::EditorSettings::set_show_table_headers(cx, next);
        self.close_context_menu(cx);
        // The preference is read while rendering table cells; re-render the
        // editor (and with it every table) to reflect the new styling.
        cx.notify();
    }

    pub(super) fn on_delete_table_column(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(selection) = self.active_axis_menu_selection() else {
            return;
        };
        if selection.kind != TableAxisKind::Column {
            return;
        }
        let Some(table_block) = self.table_block_by_id(selection.table_block_id, cx) else {
            return;
        };
        let column_count = table_block
            .read(cx)
            .record
            .table
            .as_ref()
            .map(|table| table.column_count());
        self.close_context_menu(cx);
        // Removing the only column empties the table, so drop the whole block.
        if column_count == Some(1) {
            self.remove_table_block(&table_block, cx);
        } else {
            self.delete_table_column(&table_block, selection.index, cx);
        }
    }

    fn render_axis_menu_item(
        theme: &Theme,
        id: &'static str,
        label: String,
        enabled: bool,
        danger: bool,
        on_click: fn(&mut Editor, &ClickEvent, &mut Window, &mut Context<Editor>),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        if enabled {
            div()
                .id(id)
                .h(px(d.menu_item_height))
                .px(px(d.menu_item_padding_x))
                .flex()
                .items_center()
                .rounded(px(d.menu_item_radius))
                .bg(c.dialog_surface)
                .text_size(px(d.menu_text_size))
                .font_weight(t.dialog_body_weight.to_font_weight())
                .text_color(if danger {
                    c.dialog_danger_button_bg
                } else {
                    c.dialog_secondary_button_text
                })
                .child(label)
                .hover(|this| this.bg(c.dialog_secondary_button_hover))
                .cursor_pointer()
                .on_click(cx.listener(on_click))
                .into_any_element()
        } else {
            div()
                .id(id)
                .h(px(d.menu_item_height))
                .px(px(d.menu_item_padding_x))
                .flex()
                .items_center()
                .rounded(px(d.menu_item_radius))
                .bg(c.dialog_surface)
                .text_size(px(d.menu_text_size))
                .font_weight(t.dialog_body_weight.to_font_weight())
                .text_color(if danger {
                    c.dialog_danger_button_bg
                } else {
                    c.dialog_muted
                })
                .child(label)
                .into_any_element()
        }
    }

    pub(super) fn render_context_menu_overlay(
        &self,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let menu = self.context_menu.as_ref()?;
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let s = cx.global::<I18nManager>().strings().clone();

        if let ContextMenuState::Spelling(spelling) = menu {
            return Some(self.render_spelling_menu(spelling, theme, cx));
        }
        match menu {
            ContextMenuState::General { .. } => {
                self.render_general_context_menu(theme, window.viewport_size(), window, cx)
            }
            ContextMenuState::Spelling(_) => None,
            ContextMenuState::Image {
                position,
                entity_id,
            } => {
                let entity_id = *entity_id;
                let items = super::images::IMAGE_ZOOM_LEVELS.map(|percent| {
                    div()
                        .id(("image-zoom-menu-item", percent as usize))
                        .h(px(d.menu_item_height))
                        .px(px(d.menu_item_padding_x))
                        .flex()
                        .items_center()
                        .rounded(px(d.menu_item_radius))
                        .bg(c.dialog_surface)
                        .text_size(px(d.menu_text_size))
                        .font_weight(t.dialog_body_weight.to_font_weight())
                        .text_color(c.dialog_secondary_button_text)
                        .child(
                            s.context_menu_image_zoom
                                .replace("{percent}", &percent.to_string()),
                        )
                        .hover(|this| this.bg(c.dialog_secondary_button_hover))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            this.close_context_menu(cx);
                            this.apply_image_zoom(entity_id, percent, cx);
                        }))
                });
                Some(
                    div()
                        .id("image-context-menu-overlay")
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .occlude()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(Self::on_dismiss_context_menu_overlay),
                        )
                        .child(
                            div()
                                .id("image-context-menu-panel")
                                .absolute()
                                .left(position.x)
                                .top(position.y)
                                .w(px(d.context_menu_panel_width))
                                .p(px(d.menu_panel_padding))
                                .flex()
                                .flex_col()
                                .gap(px(d.menu_panel_gap))
                                .bg(c.dialog_surface)
                                .border(px(d.dialog_border_width))
                                .border_color(c.dialog_border)
                                .rounded(px(d.menu_panel_radius))
                                .shadow_lg()
                                .on_mouse_down(MouseButton::Left, |_event, _window, cx| {
                                    cx.stop_propagation()
                                })
                                .children(items),
                        )
                        .into_any_element(),
                )
            }
            ContextMenuState::TableAxis {
                position,
                selection,
            } => {
                let Some(table_block) = self.table_block_by_id(selection.table_block_id, cx) else {
                    return None;
                };
                let table = table_block.read(cx).record.table.clone()?;
                let items = match selection.kind {
                    TableAxisKind::Column => vec![
                        Self::render_axis_menu_item(
                            theme,
                            "table-axis-align-column-left",
                            s.table_axis_align_column_left.clone(),
                            true,
                            false,
                            Self::on_align_table_column_left,
                            cx,
                        ),
                        Self::render_axis_menu_item(
                            theme,
                            "table-axis-align-column-center",
                            s.table_axis_align_column_center.clone(),
                            true,
                            false,
                            Self::on_align_table_column_center,
                            cx,
                        ),
                        Self::render_axis_menu_item(
                            theme,
                            "table-axis-align-column-right",
                            s.table_axis_align_column_right.clone(),
                            true,
                            false,
                            Self::on_align_table_column_right,
                            cx,
                        ),
                        div()
                            .mx(px(d.menu_separator_margin_x))
                            .my(px(d.menu_separator_margin_y))
                            .h(px(d.menu_separator_height))
                            .bg(c.dialog_border)
                            .into_any_element(),
                        Self::render_axis_menu_item(
                            theme,
                            "table-axis-insert-column-left",
                            s.table_axis_insert_column_left.clone(),
                            true,
                            false,
                            Self::on_insert_table_column_left,
                            cx,
                        ),
                        Self::render_axis_menu_item(
                            theme,
                            "table-axis-insert-column-right",
                            s.table_axis_insert_column_right.clone(),
                            true,
                            false,
                            Self::on_insert_table_column_right,
                            cx,
                        ),
                        Self::render_axis_menu_item(
                            theme,
                            "table-axis-move-column-left",
                            s.table_axis_move_column_left.clone(),
                            selection.index > 0,
                            false,
                            Self::on_move_table_column_left,
                            cx,
                        ),
                        Self::render_axis_menu_item(
                            theme,
                            "table-axis-move-column-right",
                            s.table_axis_move_column_right.clone(),
                            selection.index + 1 < table.column_count(),
                            false,
                            Self::on_move_table_column_right,
                            cx,
                        ),
                        div()
                            .mx(px(d.menu_separator_margin_x))
                            .my(px(d.menu_separator_margin_y))
                            .h(px(d.menu_separator_height))
                            .bg(c.dialog_border)
                            .into_any_element(),
                        Self::render_axis_menu_item(
                            theme,
                            "table-axis-delete-column",
                            s.table_axis_delete_column.clone(),
                            // Always enabled: deleting the last column removes the
                            // whole table.
                            true,
                            true,
                            Self::on_delete_table_column,
                            cx,
                        ),
                    ],
                    TableAxisKind::Row => {
                        let mut items: Vec<AnyElement> = Vec::new();
                        // The header row (visual index 0) shares the normal row
                        // menu, with its Header Row styling toggle added on top.
                        if selection.index == 0 {
                            let headers_shown =
                                crate::config::EditorSettings::show_table_headers(cx);
                            items.push(
                                div()
                                    .id("table-header-toggle")
                                    .h(px(d.menu_item_height))
                                    .px(px(d.menu_item_padding_x))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(d.menu_item_padding_x))
                                    .rounded(px(d.menu_item_radius))
                                    .bg(c.dialog_surface)
                                    .text_size(px(d.menu_text_size))
                                    .font_weight(t.dialog_body_weight.to_font_weight())
                                    .text_color(c.dialog_secondary_button_text)
                                    .child(s.table_header_row.clone())
                                    .child(if headers_shown { "✓" } else { "" })
                                    .hover(|this| this.bg(c.dialog_secondary_button_hover))
                                    .cursor_pointer()
                                    .on_click(cx.listener(Self::on_toggle_table_headers))
                                    .into_any_element(),
                            );
                            items.push(
                                div()
                                    .mx(px(d.menu_separator_margin_x))
                                    .my(px(d.menu_separator_margin_y))
                                    .h(px(d.menu_separator_height))
                                    .bg(c.dialog_border)
                                    .into_any_element(),
                            );
                        }
                        items.push(Self::render_axis_menu_item(
                            theme,
                            "table-axis-insert-row-above",
                            s.table_axis_insert_row_above.clone(),
                            selection.index > 0,
                            false,
                            Self::on_insert_table_row_above,
                            cx,
                        ));
                        items.push(Self::render_axis_menu_item(
                            theme,
                            "table-axis-insert-row-below",
                            s.table_axis_insert_row_below.clone(),
                            true,
                            false,
                            Self::on_insert_table_row_below,
                            cx,
                        ));
                        items.push(Self::render_axis_menu_item(
                            theme,
                            "table-axis-move-row-up",
                            s.table_axis_move_row_up.clone(),
                            selection.index > 0,
                            false,
                            Self::on_move_table_row_up,
                            cx,
                        ));
                        items.push(Self::render_axis_menu_item(
                            theme,
                            "table-axis-move-row-down",
                            s.table_axis_move_row_down.clone(),
                            selection.index < table.rows.len(),
                            false,
                            Self::on_move_table_row_down,
                            cx,
                        ));
                        items.push(
                            div()
                                .mx(px(d.menu_separator_margin_x))
                                .my(px(d.menu_separator_margin_y))
                                .h(px(d.menu_separator_height))
                                .bg(c.dialog_border)
                                .into_any_element(),
                        );
                        // Always enabled: deleting the header promotes the first
                        // body row, and deleting the last remaining row removes
                        // the whole table.
                        items.push(Self::render_axis_menu_item(
                            theme,
                            "table-axis-delete-row",
                            s.table_axis_delete_row.clone(),
                            true,
                            true,
                            Self::on_delete_table_row,
                            cx,
                        ));
                        items
                    }
                };

                Some(
                    div()
                        .id("table-axis-context-menu-overlay")
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .occlude()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(Self::on_dismiss_context_menu_overlay),
                        )
                        .child(
                            div()
                                .id("table-axis-context-menu-panel")
                                .absolute()
                                .left(position.x)
                                .top(position.y)
                                .w(px(d.context_menu_axis_panel_width))
                                .p(px(d.menu_panel_padding))
                                .flex()
                                .flex_col()
                                .gap(px(d.menu_panel_gap))
                                .bg(c.dialog_surface)
                                .border(px(d.dialog_border_width))
                                .border_color(c.dialog_border)
                                .rounded(px(d.menu_panel_radius))
                                .shadow_lg()
                                .on_mouse_down(MouseButton::Left, |_event, _window, cx| {
                                    cx.stop_propagation()
                                })
                                .children(items),
                        )
                        .into_any_element(),
                )
            }
        }
    }

    pub(super) fn render_table_insert_dialog_overlay(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let dialog = self.table_insert_dialog.as_ref()?;
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let s = cx.global::<I18nManager>().strings().clone();

        let stepper =
            |id_prefix: &'static str,
             label: String,
             value: usize,
             on_dec: fn(&mut Editor, &ClickEvent, &mut Window, &mut Context<Editor>),
             on_inc: fn(&mut Editor, &ClickEvent, &mut Window, &mut Context<Editor>)| {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(d.table_insert_stepper_gap))
                    .child(
                        div()
                            .text_size(px(t.dialog_body_size))
                            .font_weight(t.dialog_button_weight.to_font_weight())
                            .text_color(c.dialog_body)
                            .child(label),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(d.table_insert_stepper_gap))
                            .child(
                                div()
                                    .id((id_prefix, 0usize))
                                    .size(px(d.table_insert_stepper_button_size))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(d.table_insert_stepper_radius))
                                    .border(px(d.dialog_border_width))
                                    .border_color(c.dialog_border)
                                    .bg(c.dialog_secondary_button_bg)
                                    .hover(|this| this.bg(c.dialog_secondary_button_hover))
                                    .cursor_pointer()
                                    .text_color(c.dialog_secondary_button_text)
                                    .on_click(cx.listener(on_dec))
                                    .child("-"),
                            )
                            .child(
                                div()
                                    .min_w(px(d.table_insert_stepper_value_min_width))
                                    .h(px(d.table_insert_stepper_button_size))
                                    .px(px(d.table_insert_stepper_value_padding_x))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(d.table_insert_stepper_radius))
                                    .border(px(d.dialog_border_width))
                                    .border_color(c.dialog_border)
                                    .bg(c.dialog_surface)
                                    .text_size(px(t.dialog_body_size))
                                    .text_color(c.dialog_title)
                                    .child(value.to_string()),
                            )
                            .child(
                                div()
                                    .id((id_prefix, 1usize))
                                    .size(px(d.table_insert_stepper_button_size))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(d.table_insert_stepper_radius))
                                    .border(px(d.dialog_border_width))
                                    .border_color(c.dialog_border)
                                    .bg(c.dialog_secondary_button_bg)
                                    .hover(|this| this.bg(c.dialog_secondary_button_hover))
                                    .cursor_pointer()
                                    .text_color(c.dialog_secondary_button_text)
                                    .on_click(cx.listener(on_inc))
                                    .child("+"),
                            ),
                    )
            };

        Some(
            div()
                .id("table-insert-dialog-overlay")
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom_0()
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .bg(c.dialog_backdrop)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(Self::on_dismiss_context_menu_overlay),
                )
                .child(
                    div()
                        .w_full()
                        .px(px(d.editor_padding))
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .id("table-insert-dialog")
                                .w(px(d.dialog_width.min(d.table_insert_dialog_width)))
                                .max_w(relative(1.0))
                                .p(px(d.dialog_padding))
                                .flex()
                                .flex_col()
                                .gap(px(d.dialog_gap))
                                .bg(c.dialog_surface)
                                .border(px(d.dialog_border_width))
                                .border_color(c.dialog_border)
                                .rounded(px(d.dialog_radius))
                                .shadow_lg()
                                .on_mouse_down(MouseButton::Left, |_event, _window, cx| {
                                    cx.stop_propagation()
                                })
                                .child(
                                    div()
                                        .text_size(px(t.dialog_title_size))
                                        .font_weight(t.dialog_title_weight.to_font_weight())
                                        .text_color(c.dialog_title)
                                        .child(s.table_insert_title.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(t.dialog_body_size))
                                        .font_weight(t.dialog_body_weight.to_font_weight())
                                        .text_color(c.dialog_body)
                                        .child(s.table_insert_description.clone()),
                                )
                                .child(stepper(
                                    "table-body-rows",
                                    s.table_insert_body_rows.clone(),
                                    dialog.body_rows,
                                    Self::on_table_rows_decrement,
                                    Self::on_table_rows_increment,
                                ))
                                .child(stepper(
                                    "table-columns",
                                    s.table_insert_columns.clone(),
                                    dialog.columns,
                                    Self::on_table_columns_decrement,
                                    Self::on_table_columns_increment,
                                ))
                                .child(
                                    div()
                                        .flex()
                                        .justify_end()
                                        .gap(px(d.dialog_button_gap))
                                        .child(
                                            div()
                                                .id("cancel-table-insert-dialog")
                                                .h(px(d.dialog_button_height))
                                                .px(px(d.dialog_button_padding_x))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .rounded(px((d.dialog_radius - 4.0).max(0.0)))
                                                .border(px(d.dialog_border_width))
                                                .border_color(c.dialog_border)
                                                .bg(c.dialog_secondary_button_bg)
                                                .hover(|this| {
                                                    this.bg(c.dialog_secondary_button_hover)
                                                })
                                                .cursor_pointer()
                                                .text_size(px(t.dialog_button_size))
                                                .font_weight(
                                                    t.dialog_button_weight.to_font_weight(),
                                                )
                                                .text_color(c.dialog_secondary_button_text)
                                                .on_click(
                                                    cx.listener(
                                                        Self::on_cancel_table_insert_dialog,
                                                    ),
                                                )
                                                .child(s.table_insert_cancel.clone()),
                                        )
                                        .child(
                                            div()
                                                .id("confirm-table-insert-dialog")
                                                .h(px(d.dialog_button_height))
                                                .px(px(d.dialog_button_padding_x))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .rounded(px((d.dialog_radius - 4.0).max(0.0)))
                                                .bg(c.dialog_primary_button_bg)
                                                .hover(|this| {
                                                    this.bg(c.dialog_primary_button_hover)
                                                })
                                                .cursor_pointer()
                                                .text_size(px(t.dialog_button_size))
                                                .font_weight(
                                                    t.dialog_button_weight.to_font_weight(),
                                                )
                                                .text_color(c.dialog_primary_button_text)
                                                .on_click(
                                                    cx.listener(
                                                        Self::on_confirm_table_insert_dialog,
                                                    ),
                                                )
                                                .child(s.table_insert_confirm.clone()),
                                        ),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{ContextMenuState, Editor, TableInsertTarget};
    use gpui::{AppContext, Point, TestAppContext, px};

    #[gpui::test]
    async fn context_menu_opens_with_paragraph_format_and_insert(cx: &mut TestAppContext) {
        cx.update(crate::i18n::I18nManager::init);
        let editor = cx.new(|cx| Editor::from_markdown(cx, "alpha".to_string(), None));
        editor.update(cx, |editor, cx| {
            editor.open_general_context_menu(
                Point {
                    x: px(24.0),
                    y: px(24.0),
                },
                TableInsertTarget::Append,
                None,
                cx,
            );
            let Some(ContextMenuState::General {
                entries, open_path, ..
            }) = editor.context_menu.as_ref()
            else {
                panic!("expected the document context menu");
            };
            assert!(open_path.is_empty());
            assert!(entries.len() >= 10);
        });
    }
}
