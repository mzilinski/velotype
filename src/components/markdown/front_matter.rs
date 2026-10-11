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

#[cfg(test)]
mod tests {
    use super::{front_matter_line_count, strip_front_matter};

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
}
