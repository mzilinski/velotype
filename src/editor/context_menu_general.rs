//! The editor's context menu (right click in the document): clipboard
//! commands, the Paragraph and Format menus as submenus, Insert and a web
//! search for the selection. Entries dispatch the same actions as the main
//! menus, so they behave identically.

use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::*;

use super::Editor;
use super::context_menu::{ContextMenuState, TableInsertTarget};
use crate::i18n::{I18nManager, I18nStrings};
use crate::theme::Theme;

/// What a context menu entry does.
pub(super) enum EntryKind {
    Action(Box<dyn Action>),
    Submenu(Vec<ContextEntry>),
    /// Opens the table size dialog.
    InsertTable,
    /// Searches the web for the selection.
    SearchWeb(String),
    Separator,
}

pub(super) struct ContextEntry {
    pub(super) label: SharedString,
    pub(super) kind: EntryKind,
}

impl ContextEntry {
    fn action(label: impl Into<SharedString>, action: impl Action) -> Self {
        Self {
            label: label.into(),
            kind: EntryKind::Action(Box::new(action)),
        }
    }

    fn separator() -> Self {
        Self {
            label: SharedString::default(),
            kind: EntryKind::Separator,
        }
    }

    fn submenu(label: impl Into<SharedString>, entries: Vec<ContextEntry>) -> Self {
        Self {
            label: label.into(),
            kind: EntryKind::Submenu(entries),
        }
    }
}

/// The entries of a main menu, for use as a context submenu.
fn from_menu_items(items: Vec<MenuItem>) -> Vec<ContextEntry> {
    items
        .into_iter()
        .filter_map(|item| match item {
            MenuItem::Separator => Some(ContextEntry::separator()),
            MenuItem::Action { name, action, .. } => Some(ContextEntry {
                label: name,
                kind: EntryKind::Action(action),
            }),
            MenuItem::Submenu(menu) => Some(ContextEntry::submenu(
                menu.name,
                from_menu_items(menu.items),
            )),
            MenuItem::SystemMenu(_) => None,
        })
        .collect()
}

/// The context menu for the document. `selection` is the selected text, if
/// any (for the web search).
pub(super) fn context_menu_entries(
    strings: &I18nStrings,
    selection: Option<&str>,
    allow_table: bool,
) -> Vec<ContextEntry> {
    use crate::components::{
        Copy, CopyAsHtml, CopyAsPlainText, Cut, InsertImage, OpenEmojiPicker, Paste,
        PasteAsPlainText,
    };
    let mut entries = vec![
        ContextEntry::action(strings.preferences_shortcut_cut.clone(), Cut),
        ContextEntry::action(strings.preferences_shortcut_copy.clone(), Copy),
        ContextEntry::action(strings.preferences_shortcut_paste.clone(), Paste),
        ContextEntry::separator(),
        ContextEntry::action(strings.menu_copy_as_plain_text.clone(), CopyAsPlainText),
        ContextEntry::action(strings.menu_copy_as_html.clone(), CopyAsHtml),
        ContextEntry::action(strings.menu_paste_as_plain_text.clone(), PasteAsPlainText),
        ContextEntry::separator(),
        ContextEntry::submenu(
            strings.menu_paragraph.clone(),
            from_menu_items(crate::app_menu::paragraph_menu(strings).items),
        ),
        ContextEntry::submenu(
            strings.menu_format.clone(),
            from_menu_items(crate::app_menu::format_menu(strings).items),
        ),
        ContextEntry::submenu(
            strings.context_menu_insert.clone(),
            allow_table
                .then(|| ContextEntry {
                    label: strings.context_menu_table.clone().into(),
                    kind: EntryKind::InsertTable,
                })
                .into_iter()
                .chain([
                    ContextEntry::action(strings.menu_insert_image.clone(), InsertImage),
                    ContextEntry::action(strings.menu_emoji_picker.clone(), OpenEmojiPicker),
                ])
                .collect(),
        ),
    ];
    if let Some(query) = selection.map(str::trim).filter(|query| !query.is_empty()) {
        entries.push(ContextEntry::separator());
        let shown = query.chars().take(24).collect::<String>();
        let shown = if shown.len() < query.len() {
            format!("{shown}…")
        } else {
            shown
        };
        entries.push(ContextEntry {
            label: strings
                .context_menu_search_web
                .replace("{text}", &shown)
                .into(),
            kind: EntryKind::SearchWeb(query.to_string()),
        });
    }
    entries
}

/// The entries at `path` (the open submenus), starting at the top level.
fn entries_at<'a>(entries: &'a [ContextEntry], path: &[usize]) -> Option<&'a [ContextEntry]> {
    let mut current = entries;
    for index in path {
        match &current.get(*index)?.kind {
            EntryKind::Submenu(children) => current = children,
            _ => return None,
        }
    }
    Some(current)
}

/// The URL for a web search.
pub(crate) fn web_search_url(query: &str) -> String {
    let encoded = url::form_urlencoded::byte_serialize(query.as_bytes()).collect::<String>();
    format!("https://www.google.com/search?q={encoded}")
}

impl Editor {
    pub(super) fn open_general_context_menu(
        &mut self,
        position: Point<Pixels>,
        target: TableInsertTarget,
        block: Option<EntityId>,
        cx: &mut Context<Self>,
    ) {
        if self.view_mode != super::ViewMode::Rendered {
            return;
        }
        self.close_menu_bar(cx);
        self.context_menu_submenu_close_task = None;
        let strings = cx.global::<I18nManager>().strings().clone();
        let selection = self.selected_markdown_text(cx);
        // Tables go only where they make sense (not into code, math, ...).
        let allow_table = !self.read_only
            && block.is_none_or(|block| {
                self.table_cell_binding(block).is_none()
                    && self
                        .focusable_entity_by_id(block)
                        .is_none_or(|entity| entity.read(cx).kind().allows_context_table_insert())
            });
        self.context_menu = Some(ContextMenuState::General {
            position,
            target,
            block,
            entries: std::rc::Rc::new(context_menu_entries(
                &strings,
                selection.as_deref(),
                allow_table,
            )),
            open_path: Vec::new(),
        });
        cx.notify();
    }

    /// Hovering an entry at `depth`: open its submenu, or (after a short
    /// delay, so the pointer can cross to an open submenu) close deeper ones.
    fn on_context_entry_hover(&mut self, depth: usize, index: usize, cx: &mut Context<Self>) {
        let Some(ContextMenuState::General {
            entries, open_path, ..
        }) = self.context_menu.as_mut()
        else {
            return;
        };
        let is_submenu = entries_at(entries, &open_path[..depth.min(open_path.len())])
            .and_then(|level| level.get(index))
            .is_some_and(|entry| matches!(entry.kind, EntryKind::Submenu(_)));
        let mut next = open_path[..depth.min(open_path.len())].to_vec();
        if is_submenu {
            next.push(index);
        }
        if *open_path == next {
            self.context_menu_submenu_close_task = None;
            return;
        }
        // Opening (or staying inside the open branch) is immediate.
        if next.len() > open_path.len() || next.starts_with(open_path) || is_submenu {
            *open_path = next;
            self.context_menu_submenu_close_task = None;
            cx.notify();
            return;
        }
        self.context_menu_submenu_close_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(150))
                .await;
            let _ = this.update(cx, |editor, cx| {
                editor.context_menu_submenu_close_task = None;
                if let Some(ContextMenuState::General { open_path, .. }) =
                    editor.context_menu.as_mut()
                {
                    *open_path = next;
                    cx.notify();
                }
            });
        }));
    }

    /// Pointer entered a submenu panel at `depth`: keep its branch open.
    fn on_context_panel_hover(&mut self, depth: usize, hovered: bool) {
        if !hovered || depth == 0 {
            return;
        }
        if let Some(ContextMenuState::General { open_path, .. }) = self.context_menu.as_ref()
            && open_path.len() >= depth
        {
            self.context_menu_submenu_close_task = None;
        }
    }

    fn on_context_entry_click(
        &mut self,
        depth: usize,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ContextMenuState::General {
            entries,
            open_path,
            block,
            target,
            ..
        }) = self.context_menu.as_ref()
        else {
            return;
        };
        let Some(entry) = entries_at(entries, &open_path[..depth.min(open_path.len())])
            .and_then(|level| level.get(index))
        else {
            return;
        };
        let block = *block;
        let target = *target;
        match &entry.kind {
            EntryKind::Separator | EntryKind::Submenu(_) => {}
            EntryKind::SearchWeb(query) => {
                let url = web_search_url(query);
                self.close_context_menu_now(cx);
                cx.open_url(&url);
            }
            EntryKind::InsertTable => {
                self.close_context_menu_now(cx);
                self.table_insert_dialog = Some(super::context_menu::TableInsertDialogState {
                    target,
                    body_rows: 2,
                    columns: 2,
                });
                cx.notify();
            }
            EntryKind::Action(action) => {
                let action = action.boxed_clone();
                self.close_context_menu_now(cx);
                // Act on the clicked block (the focused one keeps its selection).
                let focused = self.document.focused_block_entity_id(window, cx);
                if let Some(block) = block
                    && focused != Some(block)
                    && let Some(entity) = self.focusable_entity_by_id(block)
                {
                    window.focus(&entity.read(cx).focus_handle);
                    self.active_entity_id = Some(block);
                }
                window.defer(cx, move |window, cx| window.dispatch_action(action, cx));
            }
        }
    }

    fn close_context_menu_now(&mut self, cx: &mut Context<Self>) {
        self.context_menu = None;
        self.context_menu_submenu_close_task = None;
        cx.notify();
    }

    /// The panels of the general context menu (top level and open submenus).
    pub(super) fn render_general_context_menu(
        &self,
        theme: &Theme,
        viewport: Size<Pixels>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let Some(ContextMenuState::General {
            position,
            entries,
            open_path,
            ..
        }) = self.context_menu.as_ref()
        else {
            return None;
        };
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let width = px(d.context_menu_panel_width.max(220.0));
        let separator_height = d.menu_separator_height + d.menu_separator_margin_y * 2.0;
        let panel_height = |level: &[ContextEntry]| {
            let items = level
                .iter()
                .map(|entry| match entry.kind {
                    EntryKind::Separator => separator_height,
                    _ => d.menu_item_height,
                })
                .sum::<f32>();
            items
                + d.menu_panel_gap * level.len().saturating_sub(1) as f32
                + d.menu_panel_padding * 2.0
        };
        let item_top = |level: &[ContextEntry], index: usize| {
            d.menu_panel_padding
                + level[..index]
                    .iter()
                    .map(|entry| match entry.kind {
                        EntryKind::Separator => separator_height,
                        _ => d.menu_item_height,
                    })
                    .sum::<f32>()
                + d.menu_panel_gap * index as f32
        };

        let mut panels = Vec::new();
        let mut x = position.x;
        let mut y = position.y;
        let mut level: &[ContextEntry] = entries;
        for depth in 0..=open_path.len() {
            // Keep the panel inside the window.
            let height = px(panel_height(level));
            let left = if x + width > viewport.width {
                (viewport.width - width).max(px(0.0))
            } else {
                x
            };
            let top = if y + height > viewport.height {
                (viewport.height - height).max(px(0.0))
            } else {
                y
            };
            let open_index = open_path.get(depth).copied();
            let rows = level.iter().enumerate().map(|(index, entry)| {
                if matches!(entry.kind, EntryKind::Separator) {
                    return div()
                        .mx(px(d.menu_separator_margin_x))
                        .my(px(d.menu_separator_margin_y))
                        .h(px(d.menu_separator_height))
                        .bg(c.dialog_border)
                        .into_any_element();
                }
                let hint = match &entry.kind {
                    EntryKind::Action(action) => {
                        let text = window.keystroke_text_for(action.as_ref());
                        (!text.is_empty() && text != action.name()).then_some(text)
                    }
                    EntryKind::Submenu(_) => Some("›".to_string()),
                    _ => None,
                };
                let open = open_index == Some(index);
                div()
                    .id(ElementId::Name(
                        format!("context-entry-{depth}-{index}").into(),
                    ))
                    .h(px(d.menu_item_height))
                    .px(px(d.menu_item_padding_x))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(16.0))
                    .rounded(px(d.menu_item_radius))
                    .when(open, |row| row.bg(c.dialog_secondary_button_hover))
                    .hover(|row| row.bg(c.dialog_secondary_button_hover))
                    .cursor_pointer()
                    .text_size(px(d.menu_text_size))
                    .font_weight(t.dialog_body_weight.to_font_weight())
                    .text_color(c.dialog_secondary_button_text)
                    .child(
                        div()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(entry.label.clone()),
                    )
                    .children(hint.map(|hint| {
                        div()
                            .flex_shrink_0()
                            .text_color(c.text_placeholder)
                            .child(hint)
                    }))
                    .on_hover(cx.listener(move |this, hovered: &bool, _window, cx| {
                        if *hovered {
                            this.on_context_entry_hover(depth, index, cx);
                        }
                    }))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.on_context_entry_click(depth, index, window, cx);
                    }))
                    .into_any_element()
            });
            panels.push(
                div()
                    .id(ElementId::Name(format!("context-panel-{depth}").into()))
                    .absolute()
                    .left(left)
                    .top(top)
                    .w(width)
                    .p(px(d.menu_panel_padding))
                    .flex()
                    .flex_col()
                    .gap(px(d.menu_panel_gap))
                    .occlude()
                    .bg(c.dialog_surface)
                    .border(px(d.dialog_border_width))
                    .border_color(c.dialog_border)
                    .rounded(px(d.menu_panel_radius))
                    .shadow_lg()
                    .on_mouse_down(MouseButton::Left, |_event, _window, cx| {
                        cx.stop_propagation()
                    })
                    .on_hover(cx.listener(move |this, hovered: &bool, _window, _cx| {
                        this.on_context_panel_hover(depth, *hovered)
                    }))
                    .children(rows)
                    .into_any_element(),
            );
            let Some(open_index) = open_index else {
                break;
            };
            let Some(EntryKind::Submenu(children)) = level.get(open_index).map(|entry| &entry.kind)
            else {
                break;
            };
            x = left + width + px(d.context_menu_submenu_gap);
            y = top + px(item_top(level, open_index) - d.menu_panel_padding);
            level = children;
        }

        Some(
            div()
                .id("editor-context-menu-overlay")
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
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(Self::on_dismiss_context_menu_overlay),
                )
                .children(panels)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{EntryKind, context_menu_entries, web_search_url};

    #[test]
    fn menu_mirrors_paragraph_and_format_menus() {
        let strings = crate::i18n::I18nManager::default().strings().clone();
        let entries = context_menu_entries(&strings, None, true);
        let labels = entries
            .iter()
            .map(|entry| entry.label.to_string())
            .collect::<Vec<_>>();
        assert!(labels.contains(&strings.menu_paragraph));
        assert!(labels.contains(&strings.menu_format));
        assert!(labels.contains(&strings.context_menu_insert));
        // No search entry without a selection.
        assert!(
            !entries
                .iter()
                .any(|entry| matches!(entry.kind, EntryKind::SearchWeb(_)))
        );
        let paragraph = entries
            .iter()
            .find(|entry| entry.label.as_ref() == strings.menu_paragraph)
            .unwrap();
        let EntryKind::Submenu(items) = &paragraph.kind else {
            panic!("paragraph submenu");
        };
        assert!(items.len() > 10);

        let with_selection = context_menu_entries(&strings, Some("rust gpui"), false);
        assert!(matches!(
            with_selection.last().map(|entry| &entry.kind),
            Some(EntryKind::SearchWeb(query)) if query == "rust gpui"
        ));
    }

    #[test]
    fn web_search_encodes_the_query() {
        assert_eq!(
            web_search_url("a&b c"),
            "https://www.google.com/search?q=a%26b+c"
        );
    }
}

#[cfg(test)]
mod editor_tests {
    use gpui::{Point, TestAppContext, px};

    use super::EntryKind;
    use crate::editor::Editor;
    use crate::editor::context_menu::{ContextMenuState, TableInsertTarget};

    #[gpui::test]
    async fn entries_act_on_the_clicked_block(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let (editor, cx) = cx.add_window_view(|_window, cx| {
            Editor::from_markdown(cx, "alpha\n\nbeta".to_string(), None)
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                let second = editor.document.roots()[1].entity_id();
                editor.open_general_context_menu(
                    Point {
                        x: px(10.0),
                        y: px(10.0),
                    },
                    TableInsertTarget::After(second),
                    Some(second),
                    cx,
                );
                let strings = cx.global::<crate::i18n::I18nManager>().strings().clone();
                let Some(ContextMenuState::General { entries, .. }) = editor.context_menu.as_ref()
                else {
                    panic!("menu open");
                };
                let paragraph = entries
                    .iter()
                    .position(|entry| entry.label.as_ref() == strings.menu_paragraph)
                    .unwrap();
                let EntryKind::Submenu(items) = &entries[paragraph].kind else {
                    panic!("paragraph submenu");
                };
                // The first Paragraph entry is Heading 1.
                assert!(matches!(items[0].kind, EntryKind::Action(_)));
                editor.on_context_entry_hover(0, paragraph, cx);
                editor.on_context_entry_click(1, 0, window, cx);
                assert!(editor.context_menu.is_none());
            })
        });
        cx.run_until_parked();
        editor.update(cx, |editor, cx| {
            assert_eq!(editor.current_document_source(cx), "alpha\n\n# beta");
        });
    }
}
