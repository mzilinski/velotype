//! Tags of a document, the way Obsidian reads them: `#tag` in the text and
//! `tags:` (or `tag:`) in the YAML front matter. Tags may nest with `/`
//! (`#project/velotype`) and compare case-insensitively.

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

/// A tag found in a document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FoundTag {
    /// The tag without `#`, as written.
    pub(crate) name: String,
    /// Zero-based source line.
    pub(crate) line: usize,
}

/// Comparison key of a tag.
pub(crate) fn tag_key(name: &str) -> String {
    name.to_lowercase()
}

/// Whether the tag `key` is `parent` or nested below it.
pub(crate) fn tag_within(key: &str, parent: &str) -> bool {
    key == parent
        || key
            .strip_prefix(parent)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn is_tag_char(ch: char) -> bool {
    ch.is_alphanumeric() || matches!(ch, '_' | '-' | '/')
}

/// A valid tag name: tag characters only, not only digits, no empty level.
fn valid_tag(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(is_tag_char)
        && !name.chars().all(|ch| ch.is_ascii_digit())
        && !name.split('/').any(str::is_empty)
}

/// The `#tags` in a run of plain text: a `#` at the start or after
/// whitespace or an opening bracket, then tag characters.
fn inline_tags(text: &str) -> Vec<(usize, String)> {
    let mut tags = Vec::new();
    let mut previous: Option<char> = None;
    let mut chars = text.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        if ch == '#'
            && previous.is_none_or(|before| before.is_whitespace() || "([{".contains(before))
        {
            let rest = &text[index + 1..];
            let end = rest.find(|ch: char| !is_tag_char(ch)).unwrap_or(rest.len());
            let name = rest[..end].trim_end_matches('/');
            if valid_tag(name) {
                tags.push((index, name.to_string()));
            }
            // Skip the tag so `#a#b` does not yield `b`.
            while chars.peek().is_some_and(|(next, _)| *next <= index + end) {
                chars.next();
            }
            previous = rest[..end].chars().last().or(Some('#'));
            continue;
        }
        previous = Some(ch);
    }
    tags
}

/// The tags listed under `tags:`/`tag:` in YAML front matter, with the line
/// (relative to the block) of each.
fn front_matter_tags(yaml: &str) -> Vec<(usize, String)> {
    let mut tags = Vec::new();
    let mut in_list = false;
    for (line_index, line) in yaml.lines().enumerate() {
        let trimmed = line.trim();
        if in_list {
            if let Some(item) = trimmed.strip_prefix("- ").or(trimmed.strip_prefix('-')) {
                push_tag_values(item, line_index, &mut tags);
                continue;
            }
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            in_list = false;
        }
        if line.starts_with([' ', '\t']) {
            continue;
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        if !matches!(key.trim().to_lowercase().as_str(), "tags" | "tag") {
            continue;
        }
        let value = value.trim();
        if value.is_empty() {
            in_list = true;
        } else {
            push_tag_values(value, line_index, &mut tags);
        }
    }
    tags
}

/// Splits `a, b`, `[a, b]` or `a b` into tags, dropping quotes and `#`.
fn push_tag_values(value: &str, line: usize, tags: &mut Vec<(usize, String)>) {
    let value = value.split(" #").next().unwrap_or(value);
    let value = value.trim().trim_start_matches('[').trim_end_matches(']');
    for part in value.split([',', ' ']) {
        let name = part
            .trim()
            .trim_matches(['"', '\''])
            .trim_start_matches('#');
        if valid_tag(name) {
            tags.push((line, name.to_string()));
        }
    }
}

/// The tags of a document; tags in code, links and HTML are left out.
pub(crate) fn extract_tags(markdown: &str) -> Vec<FoundTag> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_WIKILINKS);
    options.insert(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);
    let line_starts = std::iter::once(0)
        .chain(markdown.match_indices('\n').map(|(index, _)| index + 1))
        .collect::<Vec<_>>();
    let line_at = |offset: usize| line_starts.partition_point(|&start| start <= offset) - 1;
    let mut tags = Vec::new();
    let mut in_metadata = false;
    let mut in_code = false;
    let mut link_depth = 0usize;
    for (event, range) in Parser::new_ext(markdown, options).into_offset_iter() {
        match event {
            Event::Start(Tag::MetadataBlock(_)) => in_metadata = true,
            Event::End(TagEnd::MetadataBlock(_)) => in_metadata = false,
            Event::Start(Tag::CodeBlock(_)) => in_code = true,
            Event::End(TagEnd::CodeBlock) => in_code = false,
            Event::Start(Tag::Link { .. } | Tag::Image { .. }) => link_depth += 1,
            Event::End(TagEnd::Link | TagEnd::Image) => link_depth = link_depth.saturating_sub(1),
            Event::Text(text) if in_metadata => {
                let first = line_at(range.start);
                for (line, name) in front_matter_tags(&text) {
                    tags.push(FoundTag {
                        name,
                        line: first + line,
                    });
                }
            }
            Event::Text(text) if !in_code && link_depth == 0 => {
                for (offset, name) in inline_tags(&text) {
                    let line = line_at(range.start) + text[..offset].matches('\n').count();
                    tags.push(FoundTag { name, line });
                }
            }
            _ => {}
        }
    }
    tags
}

#[cfg(test)]
mod tests {
    use super::{extract_tags, inline_tags, tag_within};

    fn names(markdown: &str) -> Vec<(String, usize)> {
        extract_tags(markdown)
            .into_iter()
            .map(|tag| (tag.name, tag.line))
            .collect()
    }

    #[test]
    fn inline_tags_follow_obsidian_rules() {
        let found = inline_tags("#start a#mid (#paren) #2024 #y2024 #a/b/ #ä-ö_1 #a#b end#");
        let found = found.into_iter().map(|(_, name)| name).collect::<Vec<_>>();
        assert_eq!(found, ["start", "paren", "y2024", "a/b", "ä-ö_1", "a"]);
    }

    #[test]
    fn tags_from_text_and_front_matter_with_lines() {
        let markdown = "---\ntitle: x\ntags:\n  - person\n  - \"#team/dev\"\ntag: [a, 'b']\n---\n# Heading #not-in-heading?\n\nText #idea and `#code` [#link](x.md)\n\n```\n#fenced\n```\n";
        assert_eq!(
            names(markdown),
            [
                ("person".to_string(), 3),
                ("team/dev".to_string(), 4),
                ("a".to_string(), 5),
                ("b".to_string(), 5),
                ("not-in-heading".to_string(), 7),
                ("idea".to_string(), 9),
            ]
        );
        assert_eq!(
            names("---\ntags: one, two three\n---\n"),
            [
                ("one".to_string(), 1),
                ("two".to_string(), 1),
                ("three".to_string(), 1)
            ]
        );
    }

    #[test]
    fn a_heading_is_no_tag() {
        assert!(names("# Title\n\n## Sub").is_empty());
        assert_eq!(names("#tag at start"), [("tag".to_string(), 0)]);
    }

    #[test]
    fn nested_tags_belong_to_their_parents() {
        assert!(tag_within("project/velotype", "project"));
        assert!(tag_within("project", "project"));
        assert!(!tag_within("projects", "project"));
    }
}
