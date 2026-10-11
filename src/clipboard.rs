//! Rich clipboard formats GPUI does not expose: reading HTML copied from
//! browsers or office apps (pasted as Markdown) and writing HTML for
//! Copy as HTML. Plain text keeps going through GPUI's clipboard.

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd, html};

/// HTML on the system clipboard, if any. Tests never touch the real
/// clipboard.
pub(crate) fn read_html() -> Option<String> {
    if cfg!(test) {
        return None;
    }
    let html = arboard::Clipboard::new().ok()?.get().html().ok()?;
    (!html.trim().is_empty()).then_some(html)
}

/// Puts `html` on the clipboard as HTML, with `plain` as the text
/// alternative for apps that only read text.
pub(crate) fn write_html(html: &str, plain: &str) -> anyhow::Result<()> {
    if cfg!(test) {
        return Ok(());
    }
    arboard::Clipboard::new()?
        .set()
        .html(html.to_string(), Some(plain.to_string()))?;
    Ok(())
}

/// Markdown for pasted `html`, or `None` when it holds nothing worth
/// converting (no markup beyond what the plain text already says).
pub(crate) fn html_to_markdown(html: &str) -> Option<String> {
    let converter = htmd::HtmlToMarkdown::builder()
        .options(htmd::options::Options {
            hr_style: htmd::options::HrStyle::Dashes,
            bullet_list_marker: htmd::options::BulletListMarker::Dash,
            ul_bullet_spacing: 1,
            ol_number_spacing: 1,
            ..Default::default()
        })
        .skip_tags(vec!["script", "style", "head", "meta", "title", "noscript"])
        .build();
    let markdown = converter.convert(html).ok()?;
    let markdown = markdown.trim_matches('\n').to_string();
    (!markdown.trim().is_empty()).then_some(markdown)
}

/// Whether pasted HTML should replace the plain text: only when it carries
/// formatting, so plain text copied from a browser or a code editor (which
/// also offers HTML) stays byte-identical. Whitespace and the backslash
/// escapes the converter adds are not counted as formatting.
pub(crate) fn html_adds_formatting(markdown: &str, plain: Option<&str>) -> bool {
    let normalize = |text: &str, unescape: bool| {
        let mut out = String::with_capacity(text.len());
        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            if unescape && ch == '\\' && chars.peek().is_some_and(char::is_ascii_punctuation) {
                continue;
            }
            out.push(ch);
        }
        out.split_whitespace().collect::<Vec<_>>().join(" ")
    };
    plain.is_none_or(|plain| normalize(plain, false) != normalize(markdown, true))
}

fn markdown_options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
}

/// HTML for a Markdown selection (Copy as HTML).
pub(crate) fn markdown_to_html(markdown: &str) -> String {
    let mut out = String::new();
    html::push_html(&mut out, Parser::new_ext(markdown, markdown_options()));
    out.trim_end().to_string()
}

/// The visible text of a Markdown selection without any syntax (Copy as
/// Plain Text): blocks separated by blank lines, list items by line breaks.
pub(crate) fn markdown_to_plain_text(markdown: &str) -> String {
    let mut out = String::new();
    let mut pending_break = "";
    for event in Parser::new_ext(markdown, markdown_options()) {
        match event {
            Event::Text(text) | Event::Code(text) | Event::InlineMath(text) => {
                out.push_str(pending_break);
                pending_break = "";
                out.push_str(&text);
            }
            Event::DisplayMath(text) => {
                out.push_str(pending_break);
                pending_break = "";
                out.push_str(&text);
            }
            Event::SoftBreak | Event::HardBreak => out.push('\n'),
            Event::TaskListMarker(done) => {
                out.push_str(pending_break);
                pending_break = "";
                out.push_str(if done { "[x] " } else { "[ ] " });
            }
            Event::Start(Tag::Item)
            | Event::Start(Tag::TableRow)
            | Event::Start(Tag::TableHead) => {
                if !out.is_empty() && pending_break.is_empty() {
                    pending_break = "\n";
                }
            }
            Event::Start(Tag::TableCell) => {
                if !out.is_empty() && !out.ends_with('\n') && pending_break.is_empty() {
                    pending_break = "\t";
                }
            }
            Event::End(TagEnd::Paragraph)
            | Event::End(TagEnd::Heading(_))
            | Event::End(TagEnd::CodeBlock)
            | Event::End(TagEnd::BlockQuote(_))
            | Event::End(TagEnd::List(_))
            | Event::End(TagEnd::Table) => {
                if !out.is_empty() {
                    pending_break = "\n\n";
                }
            }
            _ => {}
        }
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_html_becomes_markdown() {
        let html = "<h2>Title</h2><p>Some <strong>bold</strong> and <a href=\"https://x.y\">link</a>.</p>\
                    <ul><li>one</li><li>two</li></ul><script>alert(1)</script>";
        let markdown = html_to_markdown(html).unwrap();
        assert_eq!(
            markdown,
            "## Title\n\nSome **bold** and [link](https://x.y).\n\n- one\n- two"
        );
        assert!(html_to_markdown("<p>  </p>").is_none());
    }

    #[test]
    fn html_only_wins_when_it_adds_formatting() {
        assert!(!html_adds_formatting("plain words", Some("plain  words")));
        assert!(html_adds_formatting("**bold** words", Some("bold words")));
        assert!(html_adds_formatting("text", None));
        // Code copied from an editor arrives as escaped HTML text.
        assert!(!html_adds_formatting(
            "let x = a\\*b; // \\_tmp\\_",
            Some("let x = a*b; // _tmp_")
        ));
    }

    #[test]
    fn markdown_copies_as_html_and_plain_text() {
        let markdown = "# Head\n\nSome **bold** `code`.\n\n- [x] done\n- open\n\n| a | b |\n|---|---|\n| 1 | 2 |";
        let html = markdown_to_html(markdown);
        assert!(
            html.starts_with("<h1>Head</h1>\n<p>Some <strong>bold</strong> <code>code</code>.</p>")
        );
        assert!(html.contains("<table>"));
        assert_eq!(
            markdown_to_plain_text(markdown),
            "Head\n\nSome bold code.\n\n[x] done\nopen\n\na\tb\n1\t2"
        );
    }
}
