//! Links tab: the current document's outgoing links (from its live text) and
//! its unlinked mentions, other documents naming it in plain text.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use super::Editor;
use super::link_index::{LinkIndex, extract_links};
use gpui::*;

/// Mentions listed at most.
const MAX_MENTIONS: usize = 200;
const CONTEXT_CHARS: usize = 160;

/// A link of the current document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OutgoingLink {
    pub(crate) label: String,
    /// The note it opens; None when no document of the vault matches.
    pub(crate) target: Option<PathBuf>,
    /// The target as written, for links to missing notes.
    pub(crate) written: String,
}

/// A plain-text mention of the current document's name elsewhere.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Mention {
    pub(crate) source: PathBuf,
    pub(crate) line: usize,
    pub(crate) context: String,
    /// The mention as written, for selecting it when opened.
    pub(crate) text: String,
}

/// The wiki and document links of `markdown` (the document at `source`),
/// one entry per target in order of first appearance.
pub(crate) fn outgoing_links(
    index: &LinkIndex,
    source: &Path,
    markdown: &str,
) -> Vec<OutgoingLink> {
    let mut links: Vec<OutgoingLink> = Vec::new();
    for link in extract_links(markdown) {
        let target = if link.wiki {
            index.targets().resolve_wiki(source, &link.target)
        } else {
            index.targets().resolve_markdown(source, &link.target)
        };
        // Web links and anchors are not notes.
        if target.is_none() && !link.wiki {
            continue;
        }
        let written = link
            .target
            .split('#')
            .next()
            .unwrap_or_default()
            .to_string();
        if target.as_deref() == Some(source) || (target.is_none() && written.trim().is_empty()) {
            continue;
        }
        let duplicate = links.iter().any(|known| match (&known.target, &target) {
            (Some(left), Some(right)) => left == right,
            (None, None) => known.written.eq_ignore_ascii_case(&written),
            _ => false,
        });
        if !duplicate {
            links.push(OutgoingLink {
                label: link.label,
                target,
                written,
            });
        }
    }
    links
}

/// Byte offsets of `name` in `text` as a whole word, ignoring case.
fn word_matches(text: &str, name: &str) -> Vec<usize> {
    let lower = text.to_lowercase();
    let needle = name.to_lowercase();
    // Lower-casing can change byte lengths; only search where it did not.
    if needle.is_empty() || lower.len() != text.len() {
        return Vec::new();
    }
    let is_word = |ch: Option<char>| ch.is_some_and(|ch| ch.is_alphanumeric() || ch == '_');
    lower
        .match_indices(&needle)
        .filter(|(start, _)| {
            let before = text[..*start].chars().next_back();
            let after = text[start + needle.len()..].chars().next();
            !is_word(before) && !is_word(after)
        })
        .map(|(start, _)| start)
        .collect()
}

/// Where `name` appears in `markdown` as plain text (not in links, code,
/// headings' link syntax or front matter): line and context of each.
pub(crate) fn find_mentions_in(markdown: &str, name: &str) -> Vec<(usize, String, String)> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_WIKILINKS);
    options.insert(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);
    let line_starts = std::iter::once(0)
        .chain(markdown.match_indices('\n').map(|(index, _)| index + 1))
        .collect::<Vec<_>>();
    let mut found = Vec::new();
    let mut skip_depth = 0usize;
    let mut last_line = None;
    for (event, range) in Parser::new_ext(markdown, options).into_offset_iter() {
        match event {
            Event::Start(
                Tag::Link { .. } | Tag::Image { .. } | Tag::CodeBlock(_) | Tag::MetadataBlock(_),
            ) => skip_depth += 1,
            Event::End(
                TagEnd::Link | TagEnd::Image | TagEnd::CodeBlock | TagEnd::MetadataBlock(_),
            ) => skip_depth = skip_depth.saturating_sub(1),
            Event::Text(text) if skip_depth == 0 => {
                for offset in word_matches(&text, name) {
                    let line = line_starts.partition_point(|&start| start <= range.start) - 1
                        + text[..offset].matches('\n').count();
                    if last_line == Some(line) {
                        continue;
                    }
                    last_line = Some(line);
                    let start = line_starts[line];
                    let end = line_starts
                        .get(line + 1)
                        .map(|next| next - 1)
                        .unwrap_or(markdown.len());
                    let context = markdown[start..end].trim();
                    let context = if context.chars().count() > CONTEXT_CHARS {
                        format!(
                            "{}…",
                            context.chars().take(CONTEXT_CHARS).collect::<String>()
                        )
                    } else {
                        context.to_string()
                    };
                    found.push((line, context, text[offset..offset + name.len()].to_string()));
                }
            }
            _ => {}
        }
    }
    found
}

/// Plain-text mentions of `name` in the documents of `index` other than
/// `document`.
pub(crate) fn find_mentions(index: &LinkIndex, document: &Path, name: &str) -> Vec<Mention> {
    let mut files = index.targets().files().cloned().collect::<Vec<_>>();
    files.sort();
    let mut mentions = Vec::new();
    for path in files {
        if path == document {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
        for (line, context, written) in find_mentions_in(&text, name) {
            mentions.push(Mention {
                source: path.clone(),
                line,
                context,
                text: written,
            });
            if mentions.len() >= MAX_MENTIONS {
                return mentions;
            }
        }
    }
    mentions
}

/// Mentions computed for one document and index.
pub(crate) struct MentionsState {
    pub(crate) document: PathBuf,
    pub(crate) index: Arc<LinkIndex>,
    /// None while they are searched.
    pub(crate) mentions: Option<Vec<Mention>>,
    _task: Task<()>,
}

impl Editor {
    /// Searches the mentions of the current document in the background when
    /// the document or the index changed.
    pub(super) fn sync_workspace_mentions(&mut self, cx: &mut Context<Self>) {
        let (Some(index), Some(path)) = (self.workspace.link_index.clone(), self.file_path.clone())
        else {
            self.workspace.mentions = None;
            return;
        };
        let document = path.canonicalize().unwrap_or(path);
        if self
            .workspace
            .mentions
            .as_ref()
            .is_some_and(|state| state.document == document && Arc::ptr_eq(&state.index, &index))
        {
            return;
        }
        let Some(name) = document
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
        else {
            return;
        };
        let search_index = index.clone();
        let search_document = document.clone();
        let search = cx
            .background_executor()
            .spawn(async move { find_mentions(&search_index, &search_document, &name) });
        let task = cx.spawn(async move |this, cx| {
            let mentions = search.await;
            let _ = this.update(cx, |editor, cx| {
                if let Some(state) = editor.workspace.mentions.as_mut() {
                    state.mentions = Some(mentions);
                    cx.notify();
                }
            });
        });
        self.workspace.mentions = Some(MentionsState {
            document,
            index,
            mentions: None,
            _task: task,
        });
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{find_mentions_in, outgoing_links, word_matches};
    use crate::editor::link_index::LinkIndex;

    #[test]
    fn mentions_are_whole_words_outside_links_and_code() {
        assert_eq!(word_matches("Anna, anna and Annabel", "Anna"), [0, 6]);
        let found = find_mentions_in(
            "---\nwho: Anna\n---\nMet Anna today.\n\n[[Anna]] [Anna](Anna.md) `Anna`\n\n```\nAnna\n```\nanna again, Anna twice",
            "Anna",
        );
        let lines = found
            .iter()
            .map(|(line, _, text)| (*line, text.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(lines, [(3, "Anna"), (10, "anna")]);
        assert_eq!(found[1].1, "anna again, Anna twice");
    }

    #[test]
    fn outgoing_links_resolve_and_keep_missing_notes() {
        let files = ["Anna.md", "Home.md"]
            .iter()
            .map(|name| Path::new("/v").join(name))
            .collect::<Vec<PathBuf>>();
        let index = LinkIndex::from_sources(Path::new("/v"), files, []);
        let links = outgoing_links(
            &index,
            Path::new("/v/Home.md"),
            "[[Anna]] [[anna#x|again]] [[Missing]] [[missing]] [web](https://x.y) [[Home]]",
        );
        let summary = links
            .iter()
            .map(|link| {
                (
                    link.label.as_str(),
                    link.target.is_some(),
                    link.written.as_str(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            summary,
            [("Anna", true, "Anna"), ("Missing", false, "Missing")]
        );
    }
}
