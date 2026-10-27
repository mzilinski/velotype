//! YAML front matter detection shared by the importer and HTML export.
//!
//! Front matter is only recognized at the very start of a document: an
//! opening `---` line, at least one non-blank body line, and a closing `---`
//! or `...` line. Everything in between is kept verbatim.

/// Returns how many leading lines (both fences included) form a front matter
/// block, or `None` when the document does not start with one.
pub(crate) fn front_matter_line_count<S: AsRef<str>>(lines: &[S]) -> Option<usize> {
    if lines.first()?.as_ref().trim_end() != "---" {
        return None;
    }

    // A blank line (or another fence) right after the opener means the
    // document starts with a thematic break, not metadata.
    let first_body = lines.get(1)?.as_ref().trim_end();
    if first_body.trim().is_empty() || is_front_matter_close(first_body) {
        return None;
    }

    lines
        .iter()
        .enumerate()
        .skip(2)
        .find(|(_, line)| is_front_matter_close(line.as_ref().trim_end()))
        .map(|(index, _)| index + 1)
}

/// Returns `markdown` without its leading front matter block and the newline
/// that terminates the closing fence.
pub(crate) fn strip_front_matter(markdown: &str) -> &str {
    let lines = markdown.split('\n').collect::<Vec<_>>();
    let Some(count) = front_matter_line_count(&lines) else {
        return markdown;
    };

    let front_matter_len = lines[..count].iter().map(|line| line.len()).sum::<usize>() + count;
    markdown.get(front_matter_len..).unwrap_or("")
}

fn is_front_matter_close(line: &str) -> bool {
    line == "---" || line == "..."
}

/// The value of one front matter property, as far as it can be shown as a
/// form field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PropertyValue {
    /// `key:` without a value.
    Empty,
    /// A scalar, unquoted; block scalars (`|`, `>`) are joined.
    Text(String),
    /// A `[a, b]` flow list or a `- item` block list.
    List(Vec<String>),
    /// Nested YAML (maps, lists of maps), shown as source.
    Nested(String),
}

/// One top-level `key: value` of the front matter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Property {
    pub(crate) key: String,
    pub(crate) value: PropertyValue,
}

/// The top-level properties of a front matter block (fences included), or
/// `None` when it is not plain `key: value` YAML and is better shown as
/// source.
pub(crate) fn parse_properties(source: &str) -> Option<Vec<Property>> {
    let mut lines = source.lines().collect::<Vec<_>>();
    if lines.first().is_some_and(|line| line.trim_end() == "---") {
        lines.remove(0);
    }
    if lines
        .last()
        .is_some_and(|line| is_front_matter_close(line.trim_end()))
    {
        lines.pop();
    }

    let mut properties = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        index += 1;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if line.starts_with([' ', '\t', '-']) {
            return None;
        }
        let (key, value) = trimmed.split_once(':')?;
        let key = unquote(key.trim());
        if key.is_empty() {
            return None;
        }
        // The indented lines that belong to this key.
        let start = index;
        while index < lines.len()
            && (lines[index].trim().is_empty() || lines[index].starts_with([' ', '\t', '-']))
        {
            index += 1;
        }
        let body = &lines[start..index];
        let value = value.trim();
        let value = if value.is_empty() {
            block_value(body)
        } else if matches!(value.chars().next(), Some('|' | '>')) {
            let separator = if value.starts_with('|') { "\n" } else { " " };
            let text = body
                .iter()
                .map(|line| line.trim())
                .collect::<Vec<_>>()
                .join(separator);
            PropertyValue::Text(text.trim().to_string())
        } else if !body.iter().all(|line| line.trim().is_empty()) {
            return None;
        } else if let Some(items) = value
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
        {
            PropertyValue::List(
                items
                    .split(',')
                    .map(|item| unquote(item.trim()))
                    .filter(|item| !item.is_empty())
                    .collect(),
            )
        } else {
            PropertyValue::Text(unquote(value))
        };
        properties.push(Property { key, value });
    }
    Some(properties)
}

/// The value of a key whose content follows on indented lines.
fn block_value(body: &[&str]) -> PropertyValue {
    let content = body
        .iter()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    if content.is_empty() {
        return PropertyValue::Empty;
    }
    let items = content
        .iter()
        .map(|line| {
            let item = line.trim_start().strip_prefix('-')?;
            (item.is_empty() || item.starts_with(' ')).then(|| item.trim())
        })
        .collect::<Option<Vec<_>>>();
    match items {
        Some(items) if items.iter().all(|item| !looks_like_map(item)) => {
            PropertyValue::List(items.into_iter().map(unquote).collect())
        }
        _ => {
            let indent = content
                .iter()
                .map(|line| line.len() - line.trim_start().len())
                .min()
                .unwrap_or(0);
            PropertyValue::Nested(
                content
                    .iter()
                    .map(|line| line.get(indent..).unwrap_or(line.trim_start()))
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
        }
    }
}

/// Whether a list item is itself a `key: value` map.
fn looks_like_map(item: &str) -> bool {
    !item.starts_with(['"', '\''])
        && item
            .split_once(':')
            .is_some_and(|(_, rest)| rest.is_empty() || rest.starts_with(' '))
}

/// Drops matching surrounding quotes.
fn unquote(value: &str) -> String {
    for quote in ['"', '\''] {
        if let Some(inner) = value
            .strip_prefix(quote)
            .and_then(|value| value.strip_suffix(quote))
        {
            return inner.to_string();
        }
    }
    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        Property, PropertyValue, front_matter_line_count, parse_properties, strip_front_matter,
    };

    fn lines(markdown: &str) -> Vec<&str> {
        markdown.split('\n').collect()
    }

    #[test]
    fn detects_leading_yaml_front_matter() {
        assert_eq!(
            front_matter_line_count(&lines("---\ntitle: Doc\ntags: [a]\n---\n# Body")),
            Some(4)
        );
        assert_eq!(
            front_matter_line_count(&lines("---\ntitle: Doc\n...\n")),
            Some(3)
        );
    }

    #[test]
    fn rejects_separator_shaped_starts() {
        for markdown in [
            "---\n\ntext\n---",
            "---\n---\ntext",
            "text\n---\na: b\n---",
            "---\nunclosed: true",
            " ---\na: b\n---",
        ] {
            assert_eq!(
                front_matter_line_count(&lines(markdown)),
                None,
                "{markdown}"
            );
        }
    }

    #[test]
    fn strips_front_matter_for_export() {
        assert_eq!(strip_front_matter("---\na: b\n---\n# Body"), "# Body");
        assert_eq!(strip_front_matter("---\na: b\n---"), "");
        assert_eq!(strip_front_matter("# Body\n---"), "# Body\n---");
    }

    fn property(key: &str, value: PropertyValue) -> Property {
        Property {
            key: key.to_string(),
            value,
        }
    }

    #[test]
    fn parses_top_level_properties() {
        let source = "---\ntitle: \"My: Doc\"\ndate: 2026-10-27\ntags: [a, 'b c']\naliases:\n  - One\n  - \"Two\"\ndraft:\n# comment\nsummary: >\n  first\n  second\nauthor:\n  name: Anna\n  mail: a@b.c\n---";
        assert_eq!(
            parse_properties(source),
            Some(vec![
                property("title", PropertyValue::Text("My: Doc".into())),
                property("date", PropertyValue::Text("2026-10-27".into())),
                property("tags", PropertyValue::List(vec!["a".into(), "b c".into()])),
                property(
                    "aliases",
                    PropertyValue::List(vec!["One".into(), "Two".into()])
                ),
                property("draft", PropertyValue::Empty),
                property("summary", PropertyValue::Text("first second".into())),
                property(
                    "author",
                    PropertyValue::Nested("name: Anna\nmail: a@b.c".into())
                ),
            ])
        );
    }

    #[test]
    fn unusual_yaml_is_left_as_source() {
        assert_eq!(parse_properties("---\n- a\n- b\n---"), None);
        assert_eq!(parse_properties("---\njust text\n---"), None);
        assert_eq!(parse_properties("---\nkey: value\n  continued\n---"), None);
        assert_eq!(
            parse_properties("---\nitems:\n  - name: a\n---"),
            Some(vec![property(
                "items",
                PropertyValue::Nested("- name: a".into())
            )])
        );
    }
}
