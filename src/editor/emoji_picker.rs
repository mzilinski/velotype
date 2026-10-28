//! Edit > Emoji & Symbols: a searchable emoji grid that inserts the chosen
//! emoji at the caret of the block that was focused when it opened.

use gpui::prelude::FluentBuilder;
use gpui::*;

use super::Editor;
use crate::components::single_line_input::{InputChanged, SingleLineInput, SingleLineOverflow};
use crate::components::{Block, OpenEmojiPicker};
use crate::i18n::I18nManager;
use crate::theme::Theme;

/// Emoji per grid row.
const COLUMNS: usize = 10;
/// Grid cells rendered at once.
const MAX_SHOWN: usize = 600;
const CELL: f32 = 36.0;

pub(super) struct EmojiPickerState {
    input: Entity<SingleLineInput>,
    /// The block (and its selection) the emoji goes into.
    target: Option<Entity<Block>>,
    pub(super) items: Vec<&'static emojis::Emoji>,
    pub(super) selected: usize,
    _subscription: Subscription,
}

/// Emoji whose name or shortcodes contain every word of `query`
/// (case-insensitive), in Unicode order; all of them for an empty query.
pub(super) fn search_emoji(query: &str) -> Vec<&'static emojis::Emoji> {
    let words = query
        .split_whitespace()
        .map(|word| word.trim_matches(':').to_lowercase())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    emojis::iter()
        .filter(|emoji| {
            words.iter().all(|word| {
                emoji.name().to_lowercase().contains(word.as_str())
                    || emoji.shortcodes().any(|code| code.contains(word.as_str()))
            })
        })
        .take(MAX_SHOWN)
        .collect()
}

impl Editor {
    pub(super) fn on_open_emoji_picker(
        &mut self,
        _: &OpenEmojiPicker,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.emoji_picker.is_some() {
            self.close_emoji_picker(window, cx);
            return;
        }
        if self.is_read_only() {
            return;
        }
        let target = self
            .document
            .focused_block_entity_id(window, cx)
            .or(self.active_entity_id)
            .and_then(|id| {
                self.document.block_entity_by_id(id).or_else(|| {
                    self.table_cells
                        .get(&id)
                        .map(|binding| binding.cell.clone())
                })
            });
        let placeholder = cx
            .global::<I18nManager>()
            .strings()
            .emoji_search_placeholder
            .clone();
        let input = cx.new(|cx| {
            SingleLineInput::new("", placeholder, cx).with_overflow(SingleLineOverflow::Clip)
        });
        let subscription = cx.subscribe(&input, |this, input, _: &InputChanged, cx| {
            let query = input.read(cx).value().to_string();
            if let Some(picker) = this.emoji_picker.as_mut() {
                picker.items = search_emoji(&query);
                picker.selected = 0;
                cx.notify();
            }
        });
        window.focus(&input.focus_handle(cx));
        self.emoji_picker = Some(EmojiPickerState {
            input,
            target,
            items: search_emoji(""),
            selected: 0,
            _subscription: subscription,
        });
        cx.notify();
    }

    /// Closes the picker and puts the focus back into the target block.
    pub(super) fn close_emoji_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(picker) = self.emoji_picker.take() else {
            return;
        };
        if let Some(target) = picker.target {
            window.focus(&target.read(cx).focus_handle);
        }
        cx.notify();
    }

    pub(super) fn insert_emoji(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(picker) = self.emoji_picker.as_ref() else {
            return;
        };
        let emoji = picker.items.get(index).map(|emoji| emoji.as_str());
        let target = picker.target.clone();
        self.close_emoji_picker(window, cx);
        if let (Some(emoji), Some(target)) = (emoji, target) {
            target.update(cx, |block, cx| {
                block.replace_text_in_range(None, emoji, window, cx);
            });
        }
    }

    fn move_emoji_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(picker) = self.emoji_picker.as_mut()
            && !picker.items.is_empty()
        {
            let last = picker.items.len() as isize - 1;
            picker.selected = (picker.selected as isize + delta).clamp(0, last) as usize;
            cx.notify();
        }
    }

    fn on_emoji_picker_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(picker) = self.emoji_picker.as_ref() else {
            return;
        };
        match event.keystroke.key.as_str() {
            "right" => self.move_emoji_selection(1, cx),
            "left" => self.move_emoji_selection(-1, cx),
            "down" => self.move_emoji_selection(COLUMNS as isize, cx),
            "up" => self.move_emoji_selection(-(COLUMNS as isize), cx),
            "enter" => {
                let selected = picker.selected;
                self.insert_emoji(selected, window, cx);
            }
            "escape" => self.close_emoji_picker(window, cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    pub(super) fn render_emoji_picker(
        &self,
        theme: &Theme,
        top: Pixels,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let picker = self.emoji_picker.as_ref()?;
        let c = &theme.colors;
        let d = &theme.dimensions;
        let strings = cx.global::<I18nManager>().strings().clone();
        let selected_name = picker
            .items
            .get(picker.selected)
            .map(|emoji| {
                let code = emoji
                    .shortcode()
                    .map(|code| format!("  :{code}:"))
                    .unwrap_or_default();
                format!("{} {}{code}", emoji.as_str(), emoji.name())
            })
            .unwrap_or_else(|| strings.emoji_no_results.clone());
        let cells = picker.items.iter().enumerate().map(|(index, emoji)| {
            let selected = index == picker.selected;
            div()
                .id(ElementId::named_usize("emoji", index))
                .size(px(CELL))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(d.menu_item_radius))
                .text_size(px(22.0))
                .when(selected, |cell| cell.bg(c.dialog_secondary_button_hover))
                .hover(|cell| cell.bg(c.dialog_secondary_button_hover))
                .cursor_pointer()
                .child(emoji.as_str())
                .on_click(
                    cx.listener(move |this, _, window, cx| this.insert_emoji(index, window, cx)),
                )
        });
        let width = CELL * COLUMNS as f32 + 16.0 + 12.0;

        Some(
            div()
                .id("emoji-picker-overlay")
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom_0()
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.close_emoji_picker(window, cx)),
                )
                .child(
                    div()
                        .id("emoji-picker")
                        .absolute()
                        .top(top + px(12.0))
                        .left(relative(0.5))
                        .ml(px(-width / 2.0))
                        .w(px(width))
                        .p(px(8.0))
                        .flex()
                        .flex_col()
                        .gap(px(6.0))
                        .bg(c.dialog_surface)
                        .border(px(d.dialog_border_width))
                        .border_color(c.dialog_border)
                        .rounded(px(d.menu_panel_radius))
                        .shadow_lg()
                        .capture_key_down(cx.listener(Self::on_emoji_picker_key_down))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(
                            div()
                                .px(px(8.0))
                                .py(px(4.0))
                                .border(px(1.0))
                                .border_color(c.dialog_border)
                                .rounded(px(d.menu_item_radius))
                                .text_size(px(d.menu_text_size))
                                .text_color(c.dialog_body)
                                .child(picker.input.clone()),
                        )
                        .child(
                            div()
                                .id("emoji-picker-grid")
                                .h(px(CELL * 8.0))
                                .overflow_y_scroll()
                                .flex()
                                .flex_wrap()
                                .content_start()
                                .children(cells),
                        )
                        .child(
                            div()
                                .px(px(4.0))
                                .text_size(px(d.menu_text_size))
                                .text_color(c.text_placeholder)
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(selected_name),
                        ),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::search_emoji;

    #[test]
    fn finds_emoji_by_name_and_shortcode() {
        let hearts = search_emoji("red heart");
        assert!(
            hearts
                .iter()
                .any(|emoji| emoji.as_str() == "\u{2764}\u{FE0F}")
        );
        let smile = search_emoji(":smile:");
        assert!(smile.iter().any(|emoji| emoji.shortcode() == Some("smile")));
        assert!(search_emoji("zzqqxx").is_empty());
        assert!(!search_emoji("").is_empty());
    }

    #[gpui::test]
    async fn picking_inserts_at_the_caret(cx: &mut gpui::TestAppContext) {
        use crate::components::OpenEmojiPicker;
        use crate::editor::Editor;
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let (editor, cx) =
            cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "Hello".to_string(), None));
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                let block = editor.document.first_root().unwrap().clone();
                block.update(cx, |block, cx| block.move_to(5, cx));
                editor.active_entity_id = Some(block.entity_id());
                editor.on_open_emoji_picker(&OpenEmojiPicker, window, cx);
                let picker = editor.emoji_picker.as_mut().expect("picker open");
                picker.items = search_emoji("rocket");
                editor.insert_emoji(0, window, cx);
                assert!(editor.emoji_picker.is_none());
            })
        });
        editor.update(cx, |editor, cx| {
            assert_eq!(editor.current_document_source(cx), "Hello\u{1F680}");
        });
    }
}
