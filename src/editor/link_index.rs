//! Links between the Markdown files of a workspace folder: which document
//! links where, read in the background and shared by all tabs and windows.
//! Wiki links resolve like Obsidian's: next to the linking file, then from the
//! folder root, then by file name anywhere in the folder.

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::*;
use pulldown_cmark::{Event, LinkType, Options, Parser, Tag, TagEnd};

/// An index older than this is read again (in the background) when the Links
/// tab is shown.
const LINK_INDEX_MAX_AGE: Duration = Duration::from_secs(10);
/// Roots whose indexes are kept at most.
const LINK_INDEX_CACHE_SIZE: usize = 8;
/// Larger files are not searched for links.
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
/// Characters of a link's line shown as its context.
const CONTEXT_CHARS: usize = 160;

/// A link found in a document, before it is resolved to a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RawLink {
    pub(crate) wiki: bool,
    /// Wiki target (`Note#Heading`) or Markdown destination (`dir/note.md`).
    pub(crate) target: String,
    /// Visible text of the link.
    pub(crate) label: String,
    /// Zero-based source line of the link's start.
    pub(crate) line: usize,
    /// The link's line, trimmed and shortened around the link.
    pub(crate) context: String,
    /// Source bytes of the target as written (`Note#Heading` between the
    /// brackets, the destination in the parentheses), when it can be edited
    /// in place; None for reference links and autolinks.
    pub(crate) target_range: Option<Range<usize>>,
}

/// A link from `source` to the document the index was asked about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Backlink {
    pub(crate) source: PathBuf,
    pub(crate) label: String,
    pub(crate) line: usize,
    pub(crate) context: String,
}

/// The Markdown files of a folder, looked up the way links name them.
#[derive(Clone, Debug, Default)]
pub(crate) struct LinkTargets {
    root: PathBuf,
    /// Lookup key ([`path_key`]) to file.
    by_key: HashMap<String, PathBuf>,
    /// Lower-cased file name to the files carrying it.
    by_name: HashMap<String, Vec<PathBuf>>,
}

impl LinkTargets {
    pub(crate) fn new(root: &Path, files: impl IntoIterator<Item = PathBuf>) -> Self {
        let mut targets = Self {
            root: normalize(root),
            ..Self::default()
        };
        for file in files {
            let file = normalize(&file);
            if let Some(name) = file.file_name() {
                targets
                    .by_name
                    .entry(name.to_string_lossy().to_lowercase())
                    .or_default()
                    .push(file.clone());
            }
            targets.by_key.insert(path_key(&file), file);
        }
        for files in targets.by_name.values_mut() {
            files.sort_by(|left, right| {
                left.components()
                    .count()
                    .cmp(&right.components().count())
                    .then_with(|| left.cmp(right))
            });
        }
        targets
    }

    /// Every document of the folder, in no particular order.
    pub(crate) fn files(&self) -> impl Iterator<Item = &PathBuf> {
        self.by_key.values()
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    /// The shortest `[[target]]` naming `file`: its name when no other
    /// document shares it, else its path from the root; without extension.
    pub(crate) fn link_name(&self, file: &Path) -> String {
        let unique = file
            .file_name()
            .and_then(|name| self.by_name.get(&name.to_string_lossy().to_lowercase()))
            .is_none_or(|files| files.len() == 1);
        let named = if unique {
            Path::new(file.file_name().unwrap_or_default())
        } else {
            file.strip_prefix(&self.root).unwrap_or(file)
        };
        let named = named.to_string_lossy().replace('\\', "/");
        let lower = named.to_lowercase();
        if lower.ends_with(".md") {
            named[..named.len() - 3].to_string()
        } else {
            named
        }
    }

    fn get(&self, path: &Path) -> Option<&PathBuf> {
        self.by_key.get(&path_key(&normalize(path)))
    }

    /// The file a `[[target]]` in `source` names: next to `source`, else
    /// below the root, else the file of that name closest to `source` (same
    /// folder first, then the shortest path).
    pub(crate) fn resolve_wiki(&self, source: &Path, target: &str) -> Option<PathBuf> {
        let (file, _anchor) = crate::components::wiki_link_file_target(target)?;
        let file = file.replace('\\', "/");
        let source_dir = source.parent().unwrap_or(&self.root);
        if let Some(found) = self
            .get(&source_dir.join(&file))
            .or_else(|| self.get(&self.root.join(&file)))
        {
            return Some(found.clone());
        }
        let name = Path::new(&file)
            .file_name()?
            .to_string_lossy()
            .to_lowercase();
        let suffix = format!("/{}", file.trim_start_matches('/').to_lowercase());
        let candidates = self.by_name.get(&name)?;
        let matching = || {
            candidates
                .iter()
                .filter(|path| format!("/{}", path_key(path)).ends_with(&suffix))
        };
        matching()
            .find(|path| path.parent() == Some(source_dir))
            .or_else(|| matching().next())
            .cloned()
    }

    /// The workspace document a Markdown link destination in `source` points
    /// at; None for web links, anchors and anything outside the folder.
    pub(crate) fn resolve_markdown(&self, source: &Path, destination: &str) -> Option<PathBuf> {
        let destination = destination.trim();
        if destination.is_empty() || destination.starts_with('#') || has_scheme(destination) {
            return None;
        }
        let path = destination.split(['#', '?']).next().unwrap_or_default();
        let path = percent_decode(path).replace('\\', "/");
        if !super::workspace_search::is_document_path(Path::new(&path)) {
            return None;
        }
        let joined = match path.strip_prefix('/') {
            Some(from_root) => self.root.join(from_root),
            None => source.parent().unwrap_or(&self.root).join(&path),
        };
        self.get(&joined).cloned()
    }

    fn resolve(&self, source: &Path, link: &RawLink) -> Option<PathBuf> {
        if link.wiki {
            self.resolve_wiki(source, &link.target)
        } else {
            self.resolve_markdown(source, &link.target)
        }
    }
}

/// All links between the documents of one folder.
#[derive(Debug, Default)]
pub(crate) struct LinkIndex {
    targets: LinkTargets,
    /// Lookup key of a document to the links pointing at it, by source path
    /// and then line.
    incoming: HashMap<String, Vec<Backlink>>,
    /// Tag key to the tag as first written and the documents carrying it.
    tags: HashMap<String, (String, Vec<PathBuf>)>,
    /// Lookup key of a document to its tag keys.
    document_tags: HashMap<String, Vec<String>>,
}

/// A tag with the number of documents carrying it (or a tag below it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TagCount {
    pub(crate) name: String,
    pub(crate) documents: usize,
}

impl LinkIndex {
    /// Reads every Markdown file below `root`.
    pub(crate) fn build(root: &Path) -> Self {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let files = super::workspace_search::collect_markdown_files(&root);
        let sources = files
            .iter()
            .filter_map(|path| {
                let size = std::fs::metadata(path).ok()?.len();
                if size > MAX_FILE_BYTES {
                    return None;
                }
                let bytes = std::fs::read(path).ok()?;
                Some((path.clone(), String::from_utf8_lossy(&bytes).into_owned()))
            })
            .collect::<Vec<_>>();
        Self::from_sources(&root, files, sources)
    }

    /// Indexes `sources` (path and text) among the documents `files`.
    pub(crate) fn from_sources(
        root: &Path,
        files: Vec<PathBuf>,
        sources: impl IntoIterator<Item = (PathBuf, String)>,
    ) -> Self {
        let targets = LinkTargets::new(root, files);
        let mut incoming = HashMap::<String, Vec<Backlink>>::new();
        let mut tags = HashMap::<String, (String, Vec<PathBuf>)>::new();
        let mut document_tags = HashMap::<String, Vec<String>>::new();
        for (source, text) in sources {
            let source = normalize(&source);
            let mut keys = Vec::new();
            for tag in super::tags::extract_tags(&text) {
                let key = super::tags::tag_key(&tag.name);
                if keys.contains(&key) {
                    continue;
                }
                let entry = tags
                    .entry(key.clone())
                    .or_insert_with(|| (tag.name, Vec::new()));
                entry.1.push(source.clone());
                keys.push(key);
            }
            if !keys.is_empty() {
                document_tags.insert(path_key(&source), keys);
            }
            for link in extract_links(&text) {
                let Some(target) = targets.resolve(&source, &link) else {
                    continue;
                };
                if target == source {
                    continue;
                }
                incoming
                    .entry(path_key(&target))
                    .or_default()
                    .push(Backlink {
                        source: source.clone(),
                        label: link.label,
                        line: link.line,
                        context: link.context,
                    });
            }
        }
        for links in incoming.values_mut() {
            links.sort_by(|left, right| {
                left.source
                    .cmp(&right.source)
                    .then(left.line.cmp(&right.line))
            });
            links.dedup_by(|right, left| left.source == right.source && left.line == right.line);
        }
        for (_, documents) in tags.values_mut() {
            documents.sort();
        }
        Self {
            targets,
            incoming,
            tags,
            document_tags,
        }
    }

    /// Every tag with its document count (nested tags count for their
    /// parents too), most used first.
    pub(crate) fn all_tags(&self) -> Vec<TagCount> {
        let mut counts = self
            .tags
            .iter()
            .map(|(key, (name, _))| TagCount {
                name: name.clone(),
                documents: self.documents_with_tag(key).len(),
            })
            .collect::<Vec<_>>();
        counts.sort_by(|left, right| {
            right
                .documents
                .cmp(&left.documents)
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        });
        counts
    }

    /// The tags of `document`, as first written in the vault, with counts.
    pub(crate) fn tags_of(&self, document: &Path) -> Vec<TagCount> {
        let Some(path) = self
            .targets
            .get(document)
            .or_else(|| self.targets.get(&document.canonicalize().ok()?))
        else {
            return Vec::new();
        };
        self.document_tags
            .get(&path_key(path))
            .into_iter()
            .flatten()
            .filter_map(|key| {
                let (name, _) = self.tags.get(key)?;
                Some(TagCount {
                    name: name.clone(),
                    documents: self.documents_with_tag(key).len(),
                })
            })
            .collect()
    }

    /// The documents tagged `tag` or a tag nested below it, sorted by path.
    pub(crate) fn documents_with_tag(&self, tag: &str) -> Vec<PathBuf> {
        let parent = super::tags::tag_key(tag.trim_start_matches('#'));
        let mut documents = self
            .tags
            .iter()
            .filter(|(key, _)| super::tags::tag_within(key, &parent))
            .flat_map(|(_, (_, documents))| documents.iter().cloned())
            .collect::<Vec<_>>();
        documents.sort();
        documents.dedup();
        documents
    }

    pub(crate) fn targets(&self) -> &LinkTargets {
        &self.targets
    }

    /// The folder, with symbolic links resolved like the indexed paths.
    pub(crate) fn root(&self) -> &Path {
        &self.targets.root
    }

    /// Links from other documents to `document`, by source and line.
    pub(crate) fn backlinks(&self, document: &Path) -> &[Backlink] {
        let key = self
            .targets
            .get(document)
            .or_else(|| self.targets.get(&document.canonicalize().ok()?))
            .map(|path| path_key(path));
        key.and_then(|key| self.incoming.get(&key))
            .map(Vec::as_slice)
            .unwrap_or_default()
    }
}

/// The wiki and Markdown links of a document; links in code, images and
/// front matter are left out.
pub(crate) fn extract_links(markdown: &str) -> Vec<RawLink> {
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
    let mut links = Vec::new();
    let mut open: Option<(RawLink, Range<usize>)> = None;
    for (event, range) in Parser::new_ext(markdown, options).into_offset_iter() {
        match event {
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                ..
            }) => {
                let wiki = matches!(link_type, LinkType::WikiLink { .. });
                if !wiki
                    && !matches!(
                        link_type,
                        LinkType::Inline
                            | LinkType::Reference
                            | LinkType::Collapsed
                            | LinkType::Shortcut
                            | LinkType::Autolink
                    )
                {
                    continue;
                }
                let line = line_starts.partition_point(|&start| start <= range.start) - 1;
                open = Some((
                    RawLink {
                        wiki,
                        target: dest_url.into_string(),
                        label: String::new(),
                        line,
                        context: String::new(),
                        target_range: None,
                    },
                    range,
                ));
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some((link, _)) = open.as_mut() {
                    link.label.push_str(&text);
                }
            }
            Event::End(TagEnd::Link) => {
                if let Some((mut link, range)) = open.take() {
                    link.target_range = target_range(markdown, &range, &link);
                    link.context = context_around(markdown, &line_starts, link.line, range);
                    if link.label.trim().is_empty() {
                        link.label = link.target.clone();
                    }
                    links.push(link);
                }
            }
            _ => {}
        }
    }
    links
}

/// Where the target of `link` (spanning `range`) is written: after `[[` up to
/// `|` or `]]`, or the destination at the end of `[label](destination)`.
fn target_range(markdown: &str, range: &Range<usize>, link: &RawLink) -> Option<Range<usize>> {
    let source = markdown.get(range.clone())?;
    if link.wiki {
        let inner = source.strip_prefix("[[")?;
        let end = inner.find(['|', ']'])?;
        return Some(range.start + 2..range.start + 2 + end);
    }
    let close = source.strip_suffix(')')?;
    let open = close.rfind("](")? + 2;
    let destination = &close[open..];
    let start = destination.find(link.target.as_str())?;
    // A destination needing escapes is not written as parsed; leave it.
    (destination[start + link.target.len()..]
        .trim_start_matches('>')
        .trim()
        .is_empty()
        || destination[start + link.target.len()..]
            .trim_start()
            .starts_with(['"', '\'', '(']))
    .then(|| range.start + open + start..range.start + open + start + link.target.len())
}

/// The source line `line` without list, quote and heading markers, cut to
/// [`CONTEXT_CHARS`] around the link at byte `range`.
fn context_around(
    markdown: &str,
    line_starts: &[usize],
    line: usize,
    range: Range<usize>,
) -> String {
    let start = line_starts[line];
    let end = line_starts
        .get(line + 1)
        .map(|next| next - 1)
        .unwrap_or(markdown.len());
    let text = markdown[start..end].trim_end_matches('\r');
    let trimmed = text
        .trim_start()
        .trim_start_matches(['>', '#', '-', '*', '+', ' '])
        .trim();
    let offset = text.len() - text.trim_start().len() + (text.trim_start().len() - trimmed.len());
    let chars = trimmed.chars().count();
    if chars <= CONTEXT_CHARS {
        return trimmed.to_string();
    }
    // Center the window on the link.
    let link_start = range
        .start
        .saturating_sub(start + offset)
        .min(trimmed.len());
    let link_char = trimmed
        .char_indices()
        .take_while(|(index, _)| *index < link_start)
        .count();
    let first = link_char
        .saturating_sub(CONTEXT_CHARS / 3)
        .min(chars - CONTEXT_CHARS);
    let mut context = trimmed
        .chars()
        .skip(first)
        .take(CONTEXT_CHARS)
        .collect::<String>();
    if first > 0 {
        context.insert(0, '…');
    }
    if first + CONTEXT_CHARS < chars {
        context.push('…');
    }
    context
}

/// `path` with `.` and `..` components removed, without touching the disk.
fn normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push(component);
                }
            }
            _ => normalized.push(component),
        }
    }
    normalized
}

/// Case-insensitive lookup key of a normalized path with `/` separators, as
/// links ignore case like Obsidian (and the macOS and Windows file systems).
fn path_key(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/").to_lowercase()
}

/// `https:`, `mailto:` and the like; a Windows drive letter is no scheme.
fn has_scheme(destination: &str) -> bool {
    destination.split_once(':').is_some_and(|(scheme, _)| {
        scheme.len() > 1
            && scheme
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
    })
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && let Some(hex) = text.get(index + 1..index + 3)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            decoded.push(byte);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

enum CachedIndex {
    /// First read, with its id.
    Building(u64),
    Ready {
        index: Arc<LinkIndex>,
        built_at: Instant,
        /// Id of a newer read that replaces `index` when done.
        rebuild: Option<u64>,
        /// A document of the folder was saved since `built_at`.
        stale: bool,
    },
}

/// Link indexes by root, shared by all tabs and windows.
#[derive(Default)]
pub(crate) struct LinkIndexCache {
    entries: HashMap<PathBuf, CachedIndex>,
    next_build: u64,
}

impl Global for LinkIndexCache {}

/// The link index of `root`, or None while it is first read. Starts a
/// background read when the root is not indexed, or with `revalidate` when
/// its index is older than [`LINK_INDEX_MAX_AGE`] or a document was saved
/// since (the old index is returned meanwhile).
pub(crate) fn link_index(root: &Path, revalidate: bool, cx: &mut App) -> Option<Arc<LinkIndex>> {
    let cache = cx.default_global::<LinkIndexCache>();
    let (result, rebuild) = match cache.entries.get(root) {
        Some(CachedIndex::Building(_)) => return None,
        Some(CachedIndex::Ready {
            index,
            built_at,
            rebuild,
            stale,
        }) => (
            Some(index.clone()),
            revalidate && rebuild.is_none() && (*stale || built_at.elapsed() > LINK_INDEX_MAX_AGE),
        ),
        None => (None, true),
    };
    if !rebuild {
        return result;
    }

    cache.next_build += 1;
    let build = cache.next_build;
    match cache.entries.get_mut(root) {
        Some(CachedIndex::Ready { rebuild, .. }) => *rebuild = Some(build),
        _ => {
            if cache.entries.len() >= LINK_INDEX_CACHE_SIZE {
                cache
                    .entries
                    .retain(|_, entry| matches!(entry, CachedIndex::Building(_)));
            }
            cache
                .entries
                .insert(root.to_path_buf(), CachedIndex::Building(build));
        }
    }

    let root = root.to_path_buf();
    let read_root = root.clone();
    let read = cx.background_executor().spawn(async move {
        let start = Instant::now();
        let index = LinkIndex::build(&read_root);
        crate::perf_log::note(format!(
            "Link-Index im Hintergrund gelesen: {} – {} ms",
            read_root.display(),
            start.elapsed().as_millis(),
        ));
        index
    });
    cx.spawn(async move |cx| {
        let index = read.await;
        let _ = cx.update(|cx| {
            let cache = cx.default_global::<LinkIndexCache>();
            let current = match cache.entries.get(&root) {
                Some(CachedIndex::Building(id)) => *id == build,
                Some(CachedIndex::Ready { rebuild, .. }) => *rebuild == Some(build),
                None => false,
            };
            // A dropped entry or a newer read wins over this one.
            if current {
                cache.entries.insert(
                    root,
                    CachedIndex::Ready {
                        index: Arc::new(index),
                        built_at: Instant::now(),
                        rebuild: None,
                        stale: false,
                    },
                );
                cx.refresh_windows();
            }
        });
    })
    .detach();
    result
}

/// The finished index of `root`, if any, without reading anything.
pub(crate) fn cached_link_index(root: &Path, cx: &App) -> Option<Arc<LinkIndex>> {
    match cx.try_global::<LinkIndexCache>()?.entries.get(root)? {
        CachedIndex::Ready { index, .. } => Some(index.clone()),
        CachedIndex::Building(_) => None,
    }
}

/// Marks the indexes of every root containing `path` for a new read, after
/// that document was saved.
pub(crate) fn mark_link_index_stale(path: &Path, cx: &mut App) {
    let Some(cache) = cx.try_global::<LinkIndexCache>() else {
        return;
    };
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let roots = cache
        .entries
        .keys()
        .filter(|root| {
            let canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
            path.starts_with(&canonical)
        })
        .cloned()
        .collect::<Vec<_>>();
    if roots.is_empty() {
        return;
    }
    let cache = cx.global_mut::<LinkIndexCache>();
    for root in roots {
        if let Some(CachedIndex::Ready { stale, .. }) = cache.entries.get_mut(&root) {
            *stale = true;
        }
    }
}

/// Drops the index of `root` (after files were renamed, moved or deleted).
pub(crate) fn forget_link_index(root: &Path, cx: &mut App) {
    if let Some(cache) = cx.try_global::<LinkIndexCache>()
        && cache.entries.contains_key(root)
    {
        cx.global_mut::<LinkIndexCache>().entries.remove(root);
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{LinkIndex, LinkTargets, extract_links};

    fn files(names: &[&str]) -> Vec<PathBuf> {
        names
            .iter()
            .map(|name| Path::new("/v").join(name))
            .collect()
    }

    #[test]
    fn extracts_wiki_and_markdown_links_with_lines() {
        let links = extract_links(
            "---\nlink: [[Front]]\n---\n# Title\n\nSee [[Note|the note]] and [x](dir/a.md).\n\n```\n[[InCode]]\n```\n\n- `[[InSpan]]` ![img](p.png) <https://example.com>",
        );
        let summary = links
            .iter()
            .map(|link| {
                (
                    link.wiki,
                    link.target.as_str(),
                    link.label.as_str(),
                    link.line,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            summary,
            [
                (true, "Note", "the note", 5),
                (false, "dir/a.md", "x", 5),
                (false, "https://example.com", "https://example.com", 11),
            ]
        );
        assert_eq!(links[0].context, "See [[Note|the note]] and [x](dir/a.md).");
    }

    #[test]
    fn long_context_is_cut_around_the_link() {
        let line = format!("{} [[Target]] {}", "a".repeat(300), "b".repeat(300));
        let links = extract_links(&line);
        assert_eq!(links.len(), 1);
        let context = &links[0].context;
        assert!(context.contains("[[Target]]"), "{context}");
        assert!(context.starts_with('…') && context.ends_with('…'));
        assert_eq!(context.chars().count(), super::CONTEXT_CHARS + 2);
    }

    #[test]
    fn wiki_links_resolve_like_obsidian() {
        let targets = LinkTargets::new(
            Path::new("/v"),
            files(&[
                "Home.md",
                "people/Anna.md",
                "projects/velotype/Plan.md",
                "projects/other/Plan.md",
                "Plan.md",
            ]),
        );
        let source = Path::new("/v/projects/velotype/Notes.md");
        let resolve = |target: &str| targets.resolve_wiki(source, target);
        // Next to the source wins over a shorter path.
        assert_eq!(resolve("Plan"), Some("/v/projects/velotype/Plan.md".into()));
        // Anywhere by name, ignoring case and anchors.
        assert_eq!(resolve("anna#Kontakt"), Some("/v/people/Anna.md".into()));
        // Paths from the root and partial paths.
        assert_eq!(resolve("Home"), Some("/v/Home.md".into()));
        assert_eq!(
            resolve("other/Plan"),
            Some("/v/projects/other/Plan.md".into())
        );
        assert_eq!(resolve("Missing"), None);
        // Elsewhere the shortest path wins.
        assert_eq!(
            targets.resolve_wiki(Path::new("/v/people/Anna.md"), "Plan"),
            Some("/v/Plan.md".into())
        );
    }

    #[test]
    fn link_names_are_unique_and_short() {
        let targets = LinkTargets::new(
            Path::new("/v"),
            files(&["people/Anna.md", "a/Plan.md", "b/Plan.md", "Notes.markdown"]),
        );
        assert_eq!(targets.link_name(Path::new("/v/people/Anna.md")), "Anna");
        assert_eq!(targets.link_name(Path::new("/v/b/Plan.md")), "b/Plan");
        assert_eq!(
            targets.link_name(Path::new("/v/Notes.markdown")),
            "Notes.markdown"
        );
        assert_eq!(
            targets.resolve_wiki(Path::new("/v/a/x.md"), "b/Plan"),
            Some("/v/b/Plan.md".into())
        );
    }

    #[test]
    fn markdown_links_resolve_relative_documents_only() {
        let targets = LinkTargets::new(Path::new("/v"), files(&["a/My Note.md", "b/c.md"]));
        let source = Path::new("/v/b/c.md");
        let resolve = |destination: &str| targets.resolve_markdown(source, destination);
        assert_eq!(
            resolve("../a/My%20Note.md#part"),
            Some("/v/a/My Note.md".into())
        );
        assert_eq!(resolve("/a/my note.md"), Some("/v/a/My Note.md".into()));
        assert_eq!(resolve("https://example.com/a.md"), None);
        assert_eq!(resolve("#heading"), None);
        assert_eq!(resolve("image.png"), None);
        assert_eq!(resolve("../missing.md"), None);
    }

    #[test]
    fn backlinks_group_by_source_and_skip_self_links() {
        let index = LinkIndex::from_sources(
            Path::new("/v"),
            files(&["Anna.md", "b.md", "a.md"]),
            [
                (
                    "/v/b.md".into(),
                    "[[Anna]] twice [[Anna]]\n\nagain [x](Anna.md)".into(),
                ),
                ("/v/a.md".into(), "Hi [[anna|Anna]]".into()),
                ("/v/Anna.md".into(), "Me: [[Anna]]".into()),
            ],
        );
        let backlinks = index
            .backlinks(Path::new("/v/Anna.md"))
            .iter()
            .map(|link| {
                (
                    link.source.to_string_lossy().into_owned(),
                    link.line,
                    link.label.as_str(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            backlinks,
            [
                ("/v/a.md".to_string(), 0, "Anna"),
                ("/v/b.md".to_string(), 0, "Anna"),
                ("/v/b.md".to_string(), 2, "x"),
            ]
        );
        assert!(index.backlinks(Path::new("/v/b.md")).is_empty());
    }

    #[test]
    fn tags_are_counted_per_document_and_nest() {
        let index = LinkIndex::from_sources(
            Path::new("/v"),
            files(&["a.md", "b.md", "c.md"]),
            [
                ("/v/a.md".into(), "#Project/velotype #idea #idea".into()),
                ("/v/b.md".into(), "---\ntags: [project]\n---\n#IDEA".into()),
                ("/v/c.md".into(), "no tags".into()),
            ],
        );
        let all = index
            .all_tags()
            .into_iter()
            .map(|tag| (tag.name, tag.documents))
            .collect::<Vec<_>>();
        assert_eq!(
            all,
            [
                ("idea".to_string(), 2),
                ("project".to_string(), 2),
                ("Project/velotype".to_string(), 1),
            ]
        );
        assert_eq!(index.documents_with_tag("#PROJECT").len(), 2);
        let of_a = index
            .tags_of(Path::new("/v/a.md"))
            .into_iter()
            .map(|tag| tag.name)
            .collect::<Vec<_>>();
        assert_eq!(of_a, ["Project/velotype", "idea"]);
        assert!(index.tags_of(Path::new("/v/c.md")).is_empty());
    }

    #[test]
    fn builds_from_disk() {
        let dir = std::env::temp_dir().join(format!("velotype-links-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("Target.md"), "# Target").unwrap();
        std::fs::write(dir.join("sub").join("Source.md"), "Links to [[Target]].").unwrap();
        let index = LinkIndex::build(&dir);
        let backlinks = index.backlinks(&dir.join("Target.md"));
        assert_eq!(backlinks.len(), 1);
        assert!(backlinks[0].source.ends_with("sub/Source.md"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
