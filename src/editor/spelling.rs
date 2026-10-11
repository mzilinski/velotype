//! Spelling context menu: suggestions, "Add to Dictionary" and "Ignore" for
//! the misspelled word under a right-click.

use std::ops::Range;

use gpui::*;

use super::Editor;
use super::context_menu::ContextMenuState;
use crate::components::{Block, UndoCaptureKind};
use crate::config::EditorSettings;
use crate::i18n::I18nManager;
use crate::spellcheck::SpellChecker;
use crate::theme::Theme;

/// Suggestions shown at most, like most editors' spelling menus.
const MAX_SUGGESTIONS: usize = 6;

/// Misspelled word targeted by an open spelling menu.
pub(super) struct SpellingMenuState {
    pub(super) position: Point<Pixels>,
    pub(super) block: Entity<Block>,
    /// Clean-text byte range of the word.
    pub(super) range: Range<usize>,
    pub(super) word: String,
    pub(super) suggestions: Vec<String>,
}

impl Editor {
    /// Opens the spelling menu when the right-click hit a misspelled word.
    /// Returns `false` (leaving other menus to handle the click) otherwise.
    pub(super) fn open_spelling_menu_at(
        &mut self,
        entity_id: EntityId,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> bool {
        if !EditorSettings::writing(cx).spellcheck || self.read_only {
            return false;
        }
        let Some(block) = self.focusable_entity_by_id(entity_id) else {
            return false;
        };
        let (clean_text, clean_offset) = {
            let block = block.read(cx);
            if block.uses_raw_text_editing() {
                return false;
            }
            let display_offset = block.index_for_mouse_position(position);
            (
                block.clean_text().to_string(),
                block.current_to_clean_offset(display_offset),
            )
        };
        let Some(range) = SpellChecker::misspelled(cx, &clean_text)
            .into_iter()
            .find(|range| range.start <= clean_offset && clean_offset <= range.end)
        else {
            return false;
        };
        let word = clean_text[range.clone()].to_string();
        let mut suggestions = SpellChecker::suggestions(cx, &word);
        suggestions.truncate(MAX_SUGGESTIONS);

        self.close_menu_bar(cx);
        self.context_menu = Some(ContextMenuState::Spelling(SpellingMenuState {
            position,
            block,
            range,
            word,
            suggestions,
        }));
        cx.notify();
        true
    }

    fn spelling_menu(&self) -> Option<&SpellingMenuState> {
        match self.context_menu.as_ref()? {
            ContextMenuState::Spelling(menu) => Some(menu),
            _ => None,
        }
    }

    fn apply_spelling_suggestion(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(menu) = self.spelling_menu() else {
            return;
        };
        let Some(replacement) = menu.suggestions.get(index).cloned() else {
            return;
        };
        let block = menu.block.clone();
        let range = menu.range.clone();
        self.dismiss_contextual_overlays(cx);
        block.update(cx, |block, cx| {
            // Skip if the word changed since the menu opened.
            if block.clean_text().get(range.clone()).is_none() {
                return;
            }
            block.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            let current = block.clean_to_current_range(range);
            block.replace_text_in_visible_range(current, &replacement, None, false, cx);
        });
    }

    fn accept_spelling_word(
        &mut self,
        permanently: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(word) = self.spelling_menu().map(|menu| menu.word.clone()) else {
            return;
        };
        self.dismiss_contextual_overlays(cx);
        SpellChecker::accept_word(cx, &word, permanently);
        window.refresh();
    }

    pub(super) fn render_spelling_menu(
        &self,
        menu: &SpellingMenuState,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let strings = cx.global::<I18nManager>().strings().clone();

        let item = |id: ElementId, label: SharedString, bold: bool| {
            div()
                .id(id)
                .h(px(d.menu_item_height))
                .px(px(d.menu_item_padding_x))
                .flex()
                .items_center()
                .rounded(px(d.menu_item_radius))
                .text_size(px(d.menu_text_size))
                .font_weight(if bold {
                    FontWeight::SEMIBOLD
                } else {
                    t.dialog_body_weight.to_font_weight()
                })
                .text_color(c.dialog_secondary_button_text)
                .hover(|this| this.bg(c.dialog_secondary_button_hover))
                .cursor_pointer()
                .child(label)
        };
        let separator = || {
            div()
                .mx(px(d.menu_separator_margin_x))
                .my(px(d.menu_separator_margin_y))
                .h(px(d.menu_separator_height))
                .bg(c.dialog_border)
                .into_any_element()
        };

        let mut items: Vec<AnyElement> = Vec::new();
        if menu.suggestions.is_empty() {
            items.push(
                div()
                    .h(px(d.menu_item_height))
                    .px(px(d.menu_item_padding_x))
                    .flex()
                    .items_center()
                    .text_size(px(d.menu_text_size))
                    .text_color(c.text_placeholder)
                    .child(strings.context_spelling_no_suggestions.clone())
                    .into_any_element(),
            );
        }
        for (index, suggestion) in menu.suggestions.iter().enumerate() {
            items.push(
                item(
                    ElementId::named_usize("spelling-suggestion", index),
                    suggestion.clone().into(),
                    true,
                )
                .on_click(
                    cx.listener(move |this, _, _, cx| this.apply_spelling_suggestion(index, cx)),
                )
                .into_any_element(),
            );
        }
        items.push(separator());
        items.push(
            item(
                "spelling-add".into(),
                strings.context_spelling_add.clone().into(),
                false,
            )
            .on_click(
                cx.listener(|this, _, window, cx| this.accept_spelling_word(true, window, cx)),
            )
            .into_any_element(),
        );
        items.push(
            item(
                "spelling-ignore".into(),
                strings.context_spelling_ignore.clone().into(),
                false,
            )
            .on_click(
                cx.listener(|this, _, window, cx| this.accept_spelling_word(false, window, cx)),
            )
            .into_any_element(),
        );

        div()
            .id("spelling-context-menu-overlay")
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
                    .id("spelling-context-menu-panel")
                    .absolute()
                    .left(menu.position.x)
                    .top(menu.position.y)
                    .min_w(px(d.context_menu_panel_width))
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
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use gpui::{AppContext as _, TestAppContext, point, px};

    use super::SpellingMenuState;
    use crate::editor::Editor;
    use crate::editor::context_menu::ContextMenuState;

    #[gpui::test]
    async fn applying_a_suggestion_replaces_the_word(cx: &mut TestAppContext) {
        let editor = cx.new(|cx| Editor::from_markdown(cx, "say **Thsi** now".to_string(), None));
        editor.update(cx, |editor, cx| {
            let block = editor.document.first_root().expect("root").clone();
            let start = block.read(cx).clean_text().find("Thsi").unwrap();
            editor.context_menu = Some(ContextMenuState::Spelling(SpellingMenuState {
                position: point(px(0.0), px(0.0)),
                block,
                range: start..start + 4,
                word: "Thsi".into(),
                suggestions: vec!["This".into(), "Thai".into()],
            }));
            editor.apply_spelling_suggestion(0, cx);
            assert!(editor.context_menu.is_none());
        });
        cx.run_until_parked();
        editor.update(cx, |editor, cx| {
            assert_eq!(editor.document.markdown_text(cx), "say **This** now");
        });
    }
}
