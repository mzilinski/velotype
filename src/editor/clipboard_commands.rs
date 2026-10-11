//! Edit menu clipboard variants: Copy as Plain Text and Copy as HTML.
//! (Paste as Plain Text lives on the block, next to the regular paste.)

use gpui::*;

use super::Editor;
use crate::clipboard::{markdown_to_html, markdown_to_plain_text, write_html};
use crate::components::{CopyAsHtml, CopyAsPlainText};

impl Editor {
    pub(crate) fn on_copy_as_plain_text(
        &mut self,
        _: &CopyAsPlainText,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(markdown) = self.selected_markdown_text(cx) {
            cx.write_to_clipboard(ClipboardItem::new_string(markdown_to_plain_text(&markdown)));
        }
    }

    /// Puts the selection on the clipboard as HTML: rich text for apps that
    /// read HTML, the HTML source for plain-text editors.
    pub(crate) fn on_copy_as_html(
        &mut self,
        _: &CopyAsHtml,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(markdown) = self.selected_markdown_text(cx) else {
            return;
        };
        let html = markdown_to_html(&markdown);
        if write_html(&html, &html).is_err() {
            cx.write_to_clipboard(ClipboardItem::new_string(html));
        }
    }
}
