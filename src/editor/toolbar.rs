//! View > Toolbar: a row of buttons above the document for the most used
//! commands. Buttons dispatch the same actions as the menus, through the
//! focused block, so they act on the caret or selection.

use gpui::prelude::FluentBuilder as _;
use gpui::*;

use super::Editor;
use crate::i18n::I18nStrings;
use crate::theme::Theme;

/// Hover text of a toolbar button.
struct ToolbarTooltip {
    text: SharedString,
    background: Hsla,
    border: Hsla,
    color: Hsla,
}

impl Render for ToolbarTooltip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(8.0))
            .py(px(4.0))
            .rounded(px(4.0))
            .bg(self.background)
            .border(px(1.0))
            .border_color(self.border)
            .text_size(px(12.0))
            .text_color(self.color)
            .child(self.text.clone())
    }
}

/// A button (symbol, name for the tooltip, action) or a gap between groups.
enum ToolbarItem {
    Button(&'static str, String, Box<dyn Action>),
    Gap,
}

fn toolbar_items(strings: &I18nStrings) -> Vec<ToolbarItem> {
    use crate::components::*;
    use ToolbarItem::{Button, Gap};
    let heading = |level: u8| {
        strings
            .preferences_shortcut_set_heading
            .replace("{level}", &level.to_string())
    };
    vec![
        Button(
            "↶",
            strings.preferences_shortcut_undo.clone(),
            Box::new(Undo),
        ),
        Button(
            "↷",
            strings.preferences_shortcut_redo.clone(),
            Box::new(Redo),
        ),
        Gap,
        Button("H1", heading(1), Box::new(SetHeading1)),
        Button("H2", heading(2), Box::new(SetHeading2)),
        Button("H3", heading(3), Box::new(SetHeading3)),
        Button(
            "¶",
            strings.preferences_shortcut_set_paragraph.clone(),
            Box::new(SetParagraph),
        ),
        Gap,
        Button(
            "B",
            strings.preferences_shortcut_bold_selection.clone(),
            Box::new(BoldSelection),
        ),
        Button(
            "I",
            strings.preferences_shortcut_italic_selection.clone(),
            Box::new(ItalicSelection),
        ),
        Button(
            "U",
            strings.preferences_shortcut_underline_selection.clone(),
            Box::new(UnderlineSelection),
        ),
        Button(
            "S",
            strings.menu_strikethrough.clone(),
            Box::new(StrikethroughSelection),
        ),
        Button(
            "`",
            strings.preferences_shortcut_code_selection.clone(),
            Box::new(CodeSelection),
        ),
        Button(
            "≡",
            strings.menu_highlight.clone(),
            Box::new(HighlightSelection),
        ),
        Button("🔗", strings.menu_link.clone(), Box::new(InsertLink)),
        Gap,
        Button(
            "•",
            strings.menu_bullet_list.clone(),
            Box::new(ToggleBulletList),
        ),
        Button(
            "1.",
            strings.menu_ordered_list.clone(),
            Box::new(ToggleOrderedList),
        ),
        Button(
            "☑",
            strings.menu_task_list.clone(),
            Box::new(ToggleTaskList),
        ),
        Button("❝", strings.menu_quote.clone(), Box::new(ToggleQuote)),
        Gap,
        Button(
            "{}",
            strings.menu_code_block.clone(),
            Box::new(InsertCodeBlock),
        ),
        Button(
            "∑",
            strings.menu_math_block.clone(),
            Box::new(InsertMathBlock),
        ),
        Button(
            "▦",
            strings.menu_insert_table.clone(),
            Box::new(InsertTable),
        ),
        Button(
            "▣",
            strings.menu_insert_image.clone(),
            Box::new(InsertImage),
        ),
        Button(
            "―",
            strings.menu_horizontal_rule.clone(),
            Box::new(InsertHorizontalRule),
        ),
        Gap,
        Button(
            "</>",
            strings.menu_source_mode.clone(),
            Box::new(ToggleViewMode),
        ),
    ]
}

impl Editor {
    pub(super) fn render_toolbar(
        &self,
        theme: &Theme,
        strings: &I18nStrings,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !crate::config::EditorSettings::writing(cx).toolbar {
            return None;
        }
        let c = &theme.colors;
        let d = &theme.dimensions;
        let (tooltip_bg, tooltip_border, tooltip_text) =
            (c.dialog_surface, c.dialog_border, c.dialog_body);
        let items =
            toolbar_items(strings)
                .into_iter()
                .enumerate()
                .map(|(index, item)| match item {
                    ToolbarItem::Gap => div()
                        .w(px(1.0))
                        .h(px(18.0))
                        .mx(px(4.0))
                        .bg(c.dialog_border)
                        .into_any_element(),
                    ToolbarItem::Button(symbol, name, action) => {
                        let shortcut = window.keystroke_text_for(action.as_ref());
                        let tip: SharedString = if shortcut.is_empty() || shortcut == action.name()
                        {
                            name.into()
                        } else {
                            format!("{name}  {shortcut}").into()
                        };
                        let bold = symbol == "B";
                        let italic = symbol == "I";
                        div()
                            .id(ElementId::named_usize("toolbar-button", index))
                            .debug_selector(|| format!("toolbar-{symbol}"))
                            .min_w(px(28.0))
                            .h(px(26.0))
                            .px(px(6.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(5.0))
                            .cursor_pointer()
                            .text_size(px(d.menu_text_size))
                            .text_color(c.dialog_body)
                            .hover(|button| button.bg(c.dialog_secondary_button_hover))
                            .when(bold, |button| button.font_weight(FontWeight::BOLD))
                            .when(italic, |button| button.italic())
                            .when(symbol == "U", |button| button.underline())
                            .when(symbol == "S", |button| button.line_through())
                            .child(symbol)
                            .tooltip(move |_window, cx| {
                                let text = tip.clone();
                                cx.new(|_| ToolbarTooltip {
                                    text,
                                    background: tooltip_bg,
                                    border: tooltip_border,
                                    color: tooltip_text,
                                })
                                .into()
                            })
                            .on_click(move |_event, window, cx| {
                                window.dispatch_action(action.boxed_clone(), cx);
                            })
                            .into_any_element()
                    }
                });
        Some(
            div()
                .id("editor-toolbar")
                .w_full()
                .flex_shrink_0()
                .px(px(8.0))
                .py(px(3.0))
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(2.0))
                .bg(c.editor_background)
                .border_b(px(d.dialog_border_width))
                .border_color(c.dialog_border)
                .children(items)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{ToolbarItem, toolbar_items};

    #[test]
    fn toolbar_has_grouped_buttons() {
        let strings = crate::i18n::I18nManager::default().strings().clone();
        let items = toolbar_items(&strings);
        let buttons = items
            .iter()
            .filter(|item| matches!(item, ToolbarItem::Button(..)))
            .count();
        assert!(buttons >= 20);
        assert!(!matches!(items.first(), Some(ToolbarItem::Gap)));
        assert!(!matches!(items.last(), Some(ToolbarItem::Gap)));
    }
}

#[cfg(test)]
mod editor_tests {
    use gpui::TestAppContext;

    use crate::editor::Editor;

    #[gpui::test]
    async fn toolbar_shows_only_when_enabled(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let (editor, cx) =
            cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "text".to_string(), None));
        for enabled in [false, true] {
            cx.update(|window, cx| {
                crate::config::EditorSettings::set_writing_for_test(
                    cx,
                    crate::config::WritingPreferences {
                        toolbar: enabled,
                        ..Default::default()
                    },
                );
                let theme = cx.global::<crate::theme::ThemeManager>().current_arc();
                let strings = cx.global::<crate::i18n::I18nManager>().strings().clone();
                editor.update(cx, |editor, cx| {
                    assert_eq!(
                        editor
                            .render_toolbar(&theme, &strings, window, cx)
                            .is_some(),
                        enabled
                    );
                });
            });
        }
    }

    #[gpui::test]
    async fn clicking_a_button_formats_the_selection(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
            crate::config::EditorSettings::set_writing_for_test(
                cx,
                crate::config::WritingPreferences {
                    toolbar: true,
                    ..Default::default()
                },
            );
        });
        let (editor, cx) = cx.add_window_view(|_window, cx| {
            Editor::from_markdown(cx, "alpha beta".to_string(), None)
        });
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                let block = editor.document.first_root().unwrap().clone();
                window.focus(&block.read(cx).focus_handle);
                block.update(cx, |block, cx| {
                    block.move_to(0, cx);
                    block.select_to(5, cx);
                });
            })
        });
        cx.run_until_parked();
        let bounds = cx.debug_bounds("toolbar-B").expect("bold button rendered");
        cx.simulate_click(bounds.center(), gpui::Modifiers::default());
        cx.run_until_parked();
        editor.update(cx, |editor, cx| {
            assert_eq!(editor.current_document_source(cx), "**alpha** beta");
        });
    }
}
