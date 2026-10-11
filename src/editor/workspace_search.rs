//! Workspace palette: Quick Open (Ctrl/Cmd+P) and Search in Folder
//! (Ctrl/Cmd+Shift+F).
//!
//! Both work on the workspace root like the drawer: the opened folder, or else
//! the folder of the current file.
//! Quick Open fuzzy-filters the Markdown files below it (plus recent files);
//! Search in Folder scans their contents on a background thread and lists
//! matching lines, optionally case-sensitive, whole-word or as a regex, and
//! can replace every match in the files (skipping tabs with unsaved changes). Opening goes through the tab host when there is one, and a
//! search hit is revealed in the opened document.

use std::ops::Range;
use std::path::{Path, PathBuf};

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use regex::{Regex, RegexBuilder};

use super::Editor;
use super::find::find_ranges;
use super::tabs::same_file;
use crate::components::single_line_input::{InputChanged, SingleLineInput, SingleLineOverflow};
use crate::components::{QuickOpen, SearchInWorkspace};
use crate::i18n::I18nManager;
use crate::theme::Theme;

/// Files collected for Quick Open and searched by Search in Folder.
const MAX_FILES: usize = 20_000;
/// Results listed at once.
const MAX_RESULTS: usize = 1000;
/// Quick Open entries listed at once.
const QUICK_OPEN_RESULTS: usize = 200;
/// Larger files are not searched.
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
/// Directories never descended into.
const SKIPPED_DIRS: &[&str] = &["node_modules", "target", ".git", ".svn", ".hg"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PaletteMode {
    QuickOpen,
    SearchInFolder,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PaletteItem {
    pub(super) path: PathBuf,
    /// Path relative to the workspace root (or absolute outside it).
    pub(super) label: String,
    /// Search hit: zero-based line, the trimmed line text and the match range
    /// within it.
    pub(super) hit: Option<(usize, String, Range<usize>)>,
}

pub(super) struct PaletteState {
    pub(super) mode: PaletteMode,
    input: Entity<SingleLineInput>,
    root: Option<PathBuf>,
    /// Quick Open candidates, collected once when the palette opens.
    files: Vec<PathBuf>,
    pub(super) items: Vec<PaletteItem>,
    pub(super) selected: usize,
    /// Bumped per query so stale background searches are dropped.
    generation: u64,
    /// Search in Folder: options, the replacement field and the totals of
    /// the last finished search (or why the query is invalid).
    pub(super) options: SearchOptions,
    replacement: Entity<SingleLineInput>,
    pub(super) show_replace: bool,
    pub(super) outcome: Option<SearchOutcome>,
    pub(super) error: Option<String>,
    _subscription: Subscription,
    _search: Option<Task<()>>,
}

/// Every Markdown file below `root`, skipping hidden and build directories.
pub(super) fn collect_markdown_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                if !is_skipped_dir_name(&name) {
                    pending.push(path);
                }
            } else if file_type.is_file() && is_document_path(&path) {
                files.push(path);
                if files.len() >= MAX_FILES {
                    return files;
                }
            }
        }
    }
    files.sort();
    files
}

/// Hidden folders and build/dependency folders are not part of a workspace.
pub(super) fn is_skipped_dir_name(name: &str) -> bool {
    name.starts_with('.') || SKIPPED_DIRS.contains(&name)
}

/// File extensions treated as Markdown documents of a workspace.
const DOCUMENT_EXTENSIONS: &[&str] = &["md", "markdown", "mdown", "mkd", "mkdn", "mdx"];

pub(super) fn is_document_path(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        let extension = extension.to_string_lossy();
        DOCUMENT_EXTENSIONS
            .iter()
            .any(|known| extension.eq_ignore_ascii_case(known))
    })
}

fn relative_label(root: Option<&Path>, path: &Path) -> String {
    root.and_then(|root| path.strip_prefix(root).ok())
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Fuzzy subsequence score of `query` in `candidate` (both compared
/// case-insensitively); `None` when not all query characters appear in order.
/// Consecutive matches and matches at word starts or in the file name score
/// higher, shorter candidates break ties.
pub(super) fn fuzzy_score(query: &str, candidate: &str) -> Option<i64> {
    let query = query.to_lowercase().chars().collect::<Vec<_>>();
    if query.is_empty() {
        return Some(0);
    }
    let candidate_chars = candidate.to_lowercase().chars().collect::<Vec<_>>();
    let file_name_start = candidate_chars
        .iter()
        .rposition(|ch| *ch == '/')
        .map_or(0, |index| index + 1);
    let mut score = 0i64;
    let mut query_index = 0;
    let mut previous_match: Option<usize> = None;
    for (index, ch) in candidate_chars.iter().enumerate() {
        if query_index == query.len() {
            break;
        }
        if *ch != query[query_index] {
            continue;
        }
        score += 1;
        if previous_match == Some(index.wrapping_sub(1)) {
            score += 5;
        }
        let at_word_start =
            index == 0 || matches!(candidate_chars[index - 1], '/' | '-' | '_' | ' ' | '.');
        if at_word_start {
            score += 3;
        }
        if index >= file_name_start {
            score += 2;
        }
        previous_match = Some(index);
        query_index += 1;
    }
    (query_index == query.len()).then(|| score * 1000 - candidate_chars.len() as i64)
}

/// Search and replace options of Search in Folder.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct SearchOptions {
    pub(super) case_sensitive: bool,
    pub(super) whole_word: bool,
    pub(super) regex: bool,
}

/// Compiles `query` into a line matcher. Plain queries are escaped; whole
/// word wraps the pattern in word boundaries. The error is the regex
/// parser's message.
pub(super) fn build_matcher(query: &str, options: SearchOptions) -> Result<Regex, String> {
    let pattern = if options.regex {
        query.to_string()
    } else {
        regex::escape(query)
    };
    let pattern = if options.whole_word {
        format!(r"\b(?:{pattern})\b")
    } else {
        pattern
    };
    RegexBuilder::new(&pattern)
        .case_insensitive(!options.case_sensitive)
        .size_limit(1 << 22)
        .build()
        .map_err(|err| err.to_string())
}

/// Non-empty matches of `matcher` in `line`.
fn line_matches<'a>(matcher: &'a Regex, line: &'a str) -> impl Iterator<Item = Range<usize>> + 'a {
    matcher
        .find_iter(line)
        .filter(|found| !found.is_empty())
        .map(|found| found.range())
}

/// The listed hits plus totals over every file of the search.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct SearchOutcome {
    pub(super) items: Vec<PaletteItem>,
    pub(super) matches: usize,
    pub(super) files: usize,
    /// More lines matched than `MAX_RESULTS`.
    pub(super) truncated: bool,
}

/// Matching lines of every file below `root`; all matches are counted, the
/// first `MAX_RESULTS` lines are listed.
pub(super) fn search_files(root: &Path, matcher: &Regex) -> SearchOutcome {
    let mut outcome = SearchOutcome::default();
    for path in collect_markdown_files(root) {
        let Some(text) = read_searchable(&path) else {
            continue;
        };
        let mut file_matched = false;
        for (line_index, line) in text.lines().enumerate() {
            let mut found = line_matches(matcher, line);
            let Some(first) = found.next() else {
                continue;
            };
            outcome.matches += 1 + found.count();
            file_matched = true;
            if outcome.items.len() >= MAX_RESULTS {
                outcome.truncated = true;
                continue;
            }
            let (snippet, range) = snippet_around(line, first);
            outcome.items.push(PaletteItem {
                label: relative_label(Some(root), &path),
                path: path.clone(),
                hit: Some((line_index, snippet, range)),
            });
        }
        if file_matched {
            outcome.files += 1;
        }
    }
    outcome
}

fn read_searchable(path: &Path) -> Option<String> {
    let size = std::fs::metadata(path).ok()?.len();
    if size > MAX_FILE_BYTES {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

/// The trimmed line, shortened around `range` when it is long, and the match
/// range within the result.
fn snippet_around(line: &str, range: Range<usize>) -> (String, Range<usize>) {
    const CONTEXT_BEFORE: usize = 40;
    const MAX_LEN: usize = 160;
    let start = line.len() - line.trim_start().len();
    let end = line.trim_end().len().max(range.end);
    let mut cut_start = start;
    if range.start.saturating_sub(start) > CONTEXT_BEFORE {
        cut_start = range.start - CONTEXT_BEFORE;
        while !line.is_char_boundary(cut_start) {
            cut_start += 1;
        }
    }
    let mut cut_end = end.min(cut_start.max(range.end) + MAX_LEN).max(range.end);
    while !line.is_char_boundary(cut_end) {
        cut_end += 1;
    }
    let prefix = if cut_start > start { "…" } else { "" };
    let suffix = if cut_end < end { "…" } else { "" };
    let snippet = format!("{prefix}{}{suffix}", &line[cut_start..cut_end]);
    let offset = prefix.len();
    (
        snippet,
        range.start - cut_start + offset..range.end - cut_start + offset,
    )
}

/// What Replace in Files did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ReplaceReport {
    pub(super) replacements: usize,
    pub(super) changed: Vec<PathBuf>,
    /// Files with matches left alone (open with unsaved changes).
    pub(super) skipped: Vec<PathBuf>,
    pub(super) failed: Vec<(PathBuf, String)>,
}

/// Replaces every match line by line, keeping line endings. In regex mode
/// `$1`/`${name}` in `replacement` refer to capture groups. `None` when
/// nothing matched.
pub(super) fn replace_in_text(
    text: &str,
    matcher: &Regex,
    replacement: &str,
    expand: bool,
) -> Option<(String, usize)> {
    let mut count = 0;
    let mut out = String::with_capacity(text.len());
    for piece in text.split_inclusive('\n') {
        let content_len = piece.trim_end_matches(['\n', '\r']).len();
        let (line, ending) = piece.split_at(content_len);
        let found = line_matches(matcher, line).collect::<Vec<_>>();
        if found.is_empty() {
            out.push_str(piece);
            continue;
        }
        count += found.len();
        let mut last = 0;
        for range in found {
            out.push_str(&line[last..range.start]);
            if expand {
                if let Some(captures) = matcher.captures_at(line, range.start)
                    && captures.get(0).map(|m| m.range()) == Some(range.clone())
                {
                    captures.expand(replacement, &mut out);
                } else {
                    out.push_str(replacement);
                }
            } else {
                out.push_str(replacement);
            }
            last = range.end;
        }
        out.push_str(&line[last..]);
        out.push_str(ending);
    }
    (count > 0).then_some((out, count))
}

/// Replaces in every file below `root`. Files for which `skip` is true are
/// reported instead of written.
pub(super) fn replace_in_files(
    root: &Path,
    matcher: &Regex,
    replacement: &str,
    expand: bool,
    skip: impl Fn(&Path) -> bool,
) -> ReplaceReport {
    let mut report = ReplaceReport::default();
    for path in collect_markdown_files(root) {
        let Some(text) = read_searchable(&path) else {
            continue;
        };
        let Some((replaced, count)) = replace_in_text(&text, matcher, replacement, expand) else {
            continue;
        };
        if skip(&path) {
            report.skipped.push(path);
            continue;
        }
        match std::fs::write(&path, replaced) {
            Ok(()) => {
                report.replacements += count;
                report.changed.push(path);
            }
            Err(err) => report.failed.push((path, err.to_string())),
        }
    }
    report
}

impl Editor {
    pub(super) fn on_quick_open(
        &mut self,
        _: &QuickOpen,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_palette(PaletteMode::QuickOpen, window, cx);
    }

    pub(super) fn on_search_in_workspace(
        &mut self,
        _: &SearchInWorkspace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_palette(PaletteMode::SearchInFolder, window, cx);
    }

    fn open_palette(&mut self, mode: PaletteMode, window: &mut Window, cx: &mut Context<Self>) {
        let strings = cx.global::<I18nManager>().strings().clone();
        let placeholder = match mode {
            PaletteMode::QuickOpen => strings.quick_open_placeholder.clone(),
            PaletteMode::SearchInFolder => strings.workspace_search_placeholder.clone(),
        };
        let input = cx.new(|cx| {
            SingleLineInput::new("", placeholder, cx).with_overflow(SingleLineOverflow::Clip)
        });
        let subscription = cx.subscribe(&input, |this, _, _: &InputChanged, cx| {
            this.refresh_palette(cx);
        });
        let replacement = cx.new(|cx| {
            SingleLineInput::new("", strings.workspace_replace_placeholder.clone(), cx)
                .with_overflow(SingleLineOverflow::Clip)
        });
        let root = self.workspace_root();
        let files = match (mode, root.as_ref()) {
            (PaletteMode::QuickOpen, Some(root)) => collect_markdown_files(root),
            _ => Vec::new(),
        };
        window.focus(&input.focus_handle(cx));
        self.palette = Some(PaletteState {
            mode,
            input,
            root,
            files,
            items: Vec::new(),
            selected: 0,
            generation: 0,
            options: self.workspace.search_options,
            replacement,
            show_replace: false,
            outcome: None,
            error: None,
            _subscription: subscription,
            _search: None,
        });
        self.refresh_palette(cx);
    }

    pub(super) fn close_palette(&mut self, cx: &mut Context<Self>) -> bool {
        if self.palette.take().is_some() {
            cx.notify();
            true
        } else {
            false
        }
    }

    fn refresh_palette(&mut self, cx: &mut Context<Self>) {
        let Some(palette) = self.palette.as_mut() else {
            return;
        };
        let query = palette.input.read(cx).value().trim().to_string();
        palette.selected = 0;
        palette.generation += 1;
        match palette.mode {
            PaletteMode::QuickOpen => {
                let root = palette.root.clone();
                let mut candidates = palette.files.clone();
                // Recent files outside the folder are offered too.
                for recent in crate::config::read_recent_files().unwrap_or_default() {
                    if recent.is_file() && !candidates.contains(&recent) {
                        candidates.push(recent);
                    }
                }
                let mut scored = candidates
                    .into_iter()
                    .filter_map(|path| {
                        let label = relative_label(root.as_deref(), &path);
                        fuzzy_score(&query, &label).map(|score| (score, label, path))
                    })
                    .collect::<Vec<_>>();
                scored.sort_by(|left, right| right.0.cmp(&left.0).then(left.1.cmp(&right.1)));
                palette.items = scored
                    .into_iter()
                    .take(QUICK_OPEN_RESULTS)
                    .map(|(_, label, path)| PaletteItem {
                        path,
                        label,
                        hit: None,
                    })
                    .collect();
                cx.notify();
            }
            PaletteMode::SearchInFolder => {
                palette.items.clear();
                palette.outcome = None;
                palette.error = None;
                palette._search = None;
                let Some(root) = palette.root.clone() else {
                    cx.notify();
                    return;
                };
                if query.chars().count() < 2 {
                    cx.notify();
                    return;
                }
                let matcher = match build_matcher(&query, palette.options) {
                    Ok(matcher) => matcher,
                    Err(err) => {
                        palette.error = Some(err);
                        cx.notify();
                        return;
                    }
                };
                let generation = palette.generation;
                let search = cx
                    .background_executor()
                    .spawn(async move { search_files(&root, &matcher) });
                palette._search = Some(cx.spawn(async move |this, cx| {
                    let mut outcome = search.await;
                    let _ = this.update(cx, |this, cx| {
                        if let Some(palette) = this.palette.as_mut()
                            && palette.generation == generation
                        {
                            palette.items = std::mem::take(&mut outcome.items);
                            palette.outcome = Some(outcome);
                            cx.notify();
                        }
                    });
                }));
                cx.notify();
            }
        }
    }

    fn move_palette_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(palette) = self.palette.as_mut()
            && !palette.items.is_empty()
        {
            let count = palette.items.len() as isize;
            palette.selected = (palette.selected as isize + delta).rem_euclid(count) as usize;
            cx.notify();
        }
    }

    fn open_palette_item(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(palette) = self.palette.take() else {
            return;
        };
        let Some(item) = palette.items.get(index).cloned() else {
            return;
        };
        cx.notify();
        // Reveal the matched text itself, which also works for regex queries.
        let reveal = item
            .hit
            .map(|(line, snippet, range)| (line, snippet[range].to_string()));
        if self.file_path.as_deref() == Some(item.path.as_path()) {
            if let Some((line, query)) = reveal {
                self.reveal_search_hit(line, &query, cx);
            }
            return;
        }
        if self.is_tab_hosted() {
            cx.emit(super::EditorTabEvent::OpenPathAndReveal {
                path: item.path,
                reveal,
            });
        } else {
            self.request_dropped_markdown_replace(item.path, window, cx);
        }
    }

    fn toggle_search_option(
        &mut self,
        toggle: impl FnOnce(&mut SearchOptions),
        cx: &mut Context<Self>,
    ) {
        if let Some(palette) = self.palette.as_mut() {
            toggle(&mut palette.options);
            self.workspace.search_options = palette.options;
            self.refresh_palette(cx);
        }
    }

    fn toggle_palette_replace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(palette) = self.palette.as_mut() {
            palette.show_replace = !palette.show_replace;
            let target = if palette.show_replace {
                palette.replacement.focus_handle(cx)
            } else {
                palette.input.focus_handle(cx)
            };
            window.focus(&target);
            cx.notify();
        }
    }

    /// Asks before replacing every match below the workspace root.
    fn request_replace_in_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(palette) = self.palette.as_ref() else {
            return;
        };
        let (Some(root), Some(outcome)) = (palette.root.clone(), palette.outcome.as_ref()) else {
            return;
        };
        if outcome.matches == 0 {
            return;
        }
        let query = palette.input.read(cx).value().trim().to_string();
        let replacement = palette.replacement.read(cx).value().to_string();
        let options = palette.options;
        let Ok(matcher) = build_matcher(&query, options) else {
            return;
        };
        let strings = cx.global::<I18nManager>().strings().clone();
        let detail = strings
            .workspace_replace_confirm_message
            .replace("{matches}", &outcome.matches.to_string())
            .replace("{files}", &outcome.files.to_string())
            .replace("{replacement}", &replacement);
        let buttons = [
            strings.workspace_replace_confirm_button.as_str(),
            strings.preferences_cancel.as_str(),
        ];
        let answer = window.prompt(
            PromptLevel::Warning,
            &strings.workspace_replace_title,
            Some(&detail),
            &buttons,
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            let _ = this.update_in(cx, |this, window, cx| {
                this.replace_in_files(&root, &matcher, &replacement, options.regex, window, cx);
            });
        })
        .detach();
    }

    /// Writes the replacements. Files open in a tab with unsaved changes are
    /// skipped; clean open tabs are reloaded from disk afterwards.
    pub(super) fn replace_in_files(
        &mut self,
        root: &Path,
        matcher: &Regex,
        replacement: &str,
        expand: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut open = vec![(cx.entity(), self.file_path.clone(), self.document_dirty)];
        if let Some(host) = self.tab_host.as_ref().and_then(WeakEntity::upgrade) {
            open = host
                .read(cx)
                .editors()
                .map(|editor| {
                    if editor.entity_id() == cx.entity_id() {
                        (editor.clone(), self.file_path.clone(), self.document_dirty)
                    } else {
                        let read = editor.read(cx);
                        (editor.clone(), read.file_path.clone(), read.document_dirty)
                    }
                })
                .collect();
        }
        let is_open_dirty = |path: &Path| {
            open.iter().any(|(_, open_path, dirty)| {
                *dirty
                    && open_path
                        .as_deref()
                        .is_some_and(|open| same_file(open, path))
            })
        };
        let report = replace_in_files(root, matcher, replacement, expand, is_open_dirty);

        for (editor, open_path, _) in &open {
            let Some(open_path) = open_path else {
                continue;
            };
            if !report.changed.iter().any(|path| same_file(path, open_path)) {
                continue;
            }
            let path = open_path.clone();
            if editor.entity_id() == cx.entity_id() {
                let _ = self.replace_document_from_path(&path, cx);
            } else {
                editor.update(cx, |editor, cx| {
                    let _ = editor.replace_document_from_path(&path, cx);
                });
            }
        }

        let strings = cx.global::<I18nManager>().strings().clone();
        let mut detail = strings
            .workspace_replace_done_message
            .replace("{matches}", &report.replacements.to_string())
            .replace("{files}", &report.changed.len().to_string());
        let list = |paths: Vec<String>| {
            paths
                .into_iter()
                .map(|path| {
                    format!(
                        "
• {path}"
                    )
                })
                .collect::<String>()
        };
        if !report.skipped.is_empty() {
            detail.push_str(
                "

",
            );
            detail.push_str(&strings.workspace_replace_skipped);
            detail.push_str(&list(
                report
                    .skipped
                    .iter()
                    .map(|path| relative_label(Some(root), path))
                    .collect(),
            ));
        }
        if !report.failed.is_empty() {
            detail.push_str(
                "

",
            );
            detail.push_str(&strings.workspace_replace_failed);
            detail.push_str(&list(
                report
                    .failed
                    .iter()
                    .map(|(path, err)| format!("{}: {err}", relative_label(Some(root), path)))
                    .collect(),
            ));
        }
        let buttons = [strings.info_dialog_ok.as_str()];
        let _ = window.prompt(
            PromptLevel::Info,
            &strings.workspace_replace_title,
            Some(&detail),
            &buttons,
            cx,
        );
        self.refresh_palette(cx);
    }

    /// Focuses source `line` and selects the first `query` match in it.
    pub(crate) fn reveal_search_hit(&mut self, line: usize, query: &str, cx: &mut Context<Self>) {
        if !self.jump_to_source_line(line, cx) {
            return;
        }
        let Some(block) = self
            .active_entity_id
            .and_then(|id| self.focusable_entity_by_id(id))
        else {
            return;
        };
        block.update(cx, |block, cx| {
            if let Some(range) = find_ranges(block.clean_text(), query, false)
                .into_iter()
                .next()
            {
                block.selected_range = block.clean_to_current_range(range);
                block.selection_reversed = false;
                cx.notify();
            }
        });
    }

    fn on_palette_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(palette) = self.palette.as_ref() else {
            return;
        };
        let search_mode = palette.mode == PaletteMode::SearchInFolder;
        let alt = event.keystroke.modifiers.alt;
        match event.keystroke.key.as_str() {
            "c" if alt && search_mode => {
                self.toggle_search_option(|options| options.case_sensitive ^= true, cx)
            }
            "w" if alt && search_mode => {
                self.toggle_search_option(|options| options.whole_word ^= true, cx)
            }
            "r" if alt && search_mode => {
                self.toggle_search_option(|options| options.regex ^= true, cx)
            }
            "down" => self.move_palette_selection(1, cx),
            "up" => self.move_palette_selection(-1, cx),
            "enter" => {
                let selected = palette.selected;
                self.open_palette_item(selected, window, cx);
            }
            "escape" => {
                self.close_palette(cx);
                if let Some(active) = self.active_entity_id {
                    self.focus_block(active);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    /// The query field; in Search in Folder with option toggles and the
    /// optional replace row.
    fn render_palette_query_rows(
        &self,
        palette: &PaletteState,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if palette.mode == PaletteMode::QuickOpen {
            return palette.input.clone().into_any_element();
        }
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let strings = cx.global::<I18nManager>().strings().clone();
        let button = |id: &'static str, label: SharedString, active: bool, tooltip: String| {
            div()
                .id(id)
                .h(px(28.0))
                .min_w(px(28.0))
                .px(px(6.0))
                .flex()
                .flex_shrink_0()
                .items_center()
                .justify_center()
                .rounded(px(d.menu_item_radius))
                .text_size(px(d.menu_text_size))
                .font_weight(t.dialog_button_weight.to_font_weight())
                .text_color(c.dialog_secondary_button_text)
                .when(active, |this| this.bg(c.dialog_secondary_button_hover))
                .hover(|this| this.bg(c.dialog_secondary_button_hover))
                .cursor_pointer()
                .tooltip(move |_window, cx| {
                    cx.new(|_| super::find::FindTooltip(tooltip.clone().into()))
                        .into()
                })
                .child(label)
        };
        let options = palette.options;
        let query_row = div()
            .flex()
            .items_center()
            .gap(px(4.0))
            .child(
                button(
                    "palette-toggle-replace",
                    if palette.show_replace { "▾" } else { "▸" }.into(),
                    false,
                    strings.workspace_search_toggle_replace.clone(),
                )
                .on_click(
                    cx.listener(|this, _, window, cx| this.toggle_palette_replace(window, cx)),
                ),
            )
            .child(div().flex_1().min_w(px(0.0)).child(palette.input.clone()))
            .child(
                button(
                    "palette-match-case",
                    "Aa".into(),
                    options.case_sensitive,
                    strings.workspace_search_match_case.clone(),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.toggle_search_option(|options| options.case_sensitive ^= true, cx)
                })),
            )
            .child(
                button(
                    "palette-whole-word",
                    "ab|".into(),
                    options.whole_word,
                    strings.workspace_search_whole_word.clone(),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.toggle_search_option(|options| options.whole_word ^= true, cx)
                })),
            )
            .child(
                button(
                    "palette-regex",
                    ".*".into(),
                    options.regex,
                    strings.workspace_search_regex.clone(),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.toggle_search_option(|options| options.regex ^= true, cx)
                })),
            );
        let can_replace = palette
            .outcome
            .as_ref()
            .is_some_and(|outcome| outcome.matches > 0);
        let replace_row =
            palette.show_replace.then(|| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .pl(px(32.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .child(palette.replacement.clone()),
                    )
                    .child(
                        button(
                            "palette-replace-all",
                            strings.workspace_replace_confirm_button.clone().into(),
                            false,
                            strings.workspace_replace_title.clone(),
                        )
                        .when(!can_replace, |this| this.opacity(0.5))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.request_replace_in_files(window, cx)
                        })),
                    )
            });
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(query_row)
            .children(replace_row)
            .into_any_element()
    }

    pub(super) fn render_palette(
        &self,
        theme: &Theme,
        top: Pixels,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let palette = self.palette.as_ref()?;
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let strings = cx.global::<I18nManager>().strings().clone();
        let query_empty = palette.input.read(cx).value().trim().is_empty();

        let status = match palette.mode {
            _ if palette.root.is_none() && palette.mode == PaletteMode::SearchInFolder => {
                Some(strings.workspace_no_file_message.clone())
            }
            PaletteMode::SearchInFolder if palette.error.is_some() => Some(format!(
                "{}: {}",
                strings.workspace_search_invalid_regex,
                palette.error.as_deref().unwrap_or_default()
            )),
            PaletteMode::SearchInFolder if query_empty => None,
            PaletteMode::SearchInFolder => palette.outcome.as_ref().map(|outcome| {
                if outcome.matches == 0 {
                    return strings.workspace_search_no_results.clone();
                }
                let mut summary = strings
                    .workspace_search_summary
                    .replace("{matches}", &outcome.matches.to_string())
                    .replace("{files}", &outcome.files.to_string());
                if outcome.truncated {
                    summary.push_str(" · ");
                    summary.push_str(
                        &strings
                            .workspace_search_truncated
                            .replace("{shown}", &MAX_RESULTS.to_string()),
                    );
                }
                summary
            }),
            PaletteMode::QuickOpen if palette.items.is_empty() => {
                Some(strings.workspace_empty_files.clone())
            }
            _ => None,
        };

        let query_rows = self.render_palette_query_rows(palette, theme, cx);
        let rows =
            palette.items.iter().enumerate().map(|(index, item)| {
                let selected = index == palette.selected;
                let (title, detail) = match &item.hit {
                    None => {
                        let name = item
                            .path
                            .file_name()
                            .map(|name| name.to_string_lossy().to_string())
                            .unwrap_or_else(|| item.label.clone());
                        (name, item.label.clone())
                    }
                    Some((line, snippet, _)) => {
                        (snippet.clone(), format!("{}:{}", item.label, line + 1))
                    }
                };
                div()
                    .id(ElementId::named_usize("palette-item", index))
                    .px(px(10.0))
                    .py(px(4.0))
                    .flex()
                    .flex_col()
                    .rounded(px(d.menu_item_radius))
                    .when(selected, |row| row.bg(c.dialog_secondary_button_hover))
                    .hover(|row| row.bg(c.dialog_secondary_button_hover))
                    .cursor_pointer()
                    .child(
                        div()
                            .text_size(px(d.menu_text_size))
                            .text_color(c.dialog_body)
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(d.menu_text_size * 0.85))
                            .text_color(c.text_placeholder)
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(detail),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_palette_item(index, window, cx)
                    }))
                    .into_any_element()
            });

        Some(
            div()
                .id("workspace-palette-overlay")
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom_0()
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.close_palette(cx);
                    }),
                )
                .child(
                    div()
                        .id("workspace-palette")
                        .absolute()
                        .top(top + px(12.0))
                        .left(relative(0.5))
                        .ml(px(-300.0))
                        .w(px(600.0))
                        .max_h(px(460.0))
                        .p(px(8.0))
                        .flex()
                        .flex_col()
                        .gap(px(6.0))
                        .bg(c.dialog_surface)
                        .border(px(d.dialog_border_width))
                        .border_color(c.dialog_border)
                        .rounded(px(d.menu_panel_radius))
                        .shadow_lg()
                        .text_size(px(t.dialog_body_size))
                        .capture_key_down(cx.listener(Self::on_palette_key_down))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(query_rows)
                        .children(status.map(|status| {
                            div()
                                .px(px(10.0))
                                .text_size(px(d.menu_text_size))
                                .text_color(c.text_placeholder)
                                .child(status)
                        }))
                        .child(
                            div()
                                .id("workspace-palette-results")
                                .flex()
                                .flex_col()
                                .overflow_y_scroll()
                                .children(rows),
                        ),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{
        SearchOptions, build_matcher, collect_markdown_files, fuzzy_score, replace_in_files,
        replace_in_text, search_files, snippet_around,
    };

    fn plain() -> SearchOptions {
        SearchOptions::default()
    }

    #[test]
    fn matcher_options_cover_case_whole_word_and_regex() {
        let text = "Note notes NOTE note_x";
        let count = |query: &str, options: SearchOptions| {
            build_matcher(query, options)
                .unwrap()
                .find_iter(text)
                .count()
        };
        assert_eq!(count("note", plain()), 4);
        let case = SearchOptions {
            case_sensitive: true,
            ..plain()
        };
        assert_eq!(count("note", case), 2);
        let word = SearchOptions {
            whole_word: true,
            ..plain()
        };
        assert_eq!(
            count("note", word),
            2,
            "notes and note_x are not whole words"
        );
        // Plain queries are literal, regex queries are not.
        assert_eq!(count("no.e", plain()), 0);
        let regex = SearchOptions {
            regex: true,
            ..plain()
        };
        assert_eq!(count("no.e", regex), 4);
        assert!(build_matcher("(", regex).is_err());
        assert!(build_matcher("(", plain()).is_ok());
    }

    #[test]
    fn replace_keeps_line_endings_and_expands_groups_only_in_regex_mode() {
        let text = "a 1-2\r\nb 3-4\nend";
        let regex = SearchOptions {
            regex: true,
            ..plain()
        };
        let matcher = build_matcher(r"(\d)-(\d)", regex).unwrap();
        let (replaced, count) = replace_in_text(text, &matcher, "$2-$1", true).unwrap();
        assert_eq!(replaced, "a 2-1\r\nb 4-3\nend");
        assert_eq!(count, 2);

        let matcher = build_matcher("1-2", plain()).unwrap();
        let (replaced, _) = replace_in_text(text, &matcher, "$0", false).unwrap();
        assert_eq!(replaced, "a $0\r\nb 3-4\nend");
        assert!(replace_in_text("nothing", &matcher, "x", false).is_none());

        // Empty matches never replace anything.
        let empty = build_matcher("x*", regex).unwrap();
        assert!(replace_in_text("abc", &empty, "-", true).is_none());
    }

    #[test]
    fn long_lines_are_cut_around_the_match() {
        let line = format!("{}needle{}", "a".repeat(200), "b".repeat(300));
        let start = 200;
        let (snippet, range) = snippet_around(&line, start..start + 6);
        assert_eq!(&snippet[range], "needle");
        assert!(snippet.starts_with('…') && snippet.ends_with('…'));
        assert!(snippet.chars().count() < 220);

        let (snippet, range) = snippet_around("   short needle  ", 9..15);
        assert_eq!(snippet, "short needle");
        assert_eq!(&snippet[range], "needle");
    }

    #[test]
    fn replace_in_files_writes_and_skips() {
        let root = std::env::temp_dir().join(format!("velotype-rep-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("a.md"), "foo foo\n").unwrap();
        fs::write(root.join("b.md"), "foo\n").unwrap();
        fs::write(root.join("c.md"), "bar\n").unwrap();
        let matcher = build_matcher("foo", plain()).unwrap();
        let report = replace_in_files(&root, &matcher, "baz", false, |path| path.ends_with("b.md"));
        assert_eq!(report.replacements, 2);
        assert_eq!(report.changed, vec![root.join("a.md")]);
        assert_eq!(report.skipped, vec![root.join("b.md")]);
        assert_eq!(fs::read_to_string(root.join("a.md")).unwrap(), "baz baz\n");
        assert_eq!(fs::read_to_string(root.join("b.md")).unwrap(), "foo\n");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn fuzzy_prefers_file_name_and_consecutive_matches() {
        let name = fuzzy_score("note", "projects/notes.md").unwrap();
        let scattered = fuzzy_score("note", "n/o/t/e.md").unwrap();
        assert!(name > scattered);
        assert!(fuzzy_score("xyz", "notes.md").is_none());
        assert!(fuzzy_score("NTS", "notes.md").is_some());
    }

    #[test]
    fn collects_and_searches_markdown_files() {
        let root = std::env::temp_dir().join(format!("velotype-ws-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::create_dir_all(root.join("node_modules")).unwrap();
        fs::write(root.join("a.md"), "# Title\n  Some Straße here\n").unwrap();
        fs::write(root.join("sub/b.markdown"), "nothing\nstraße again").unwrap();
        fs::write(root.join("c.txt"), "Straße").unwrap();
        fs::write(root.join("node_modules/d.md"), "Straße").unwrap();

        let files = collect_markdown_files(&root);
        assert_eq!(files.len(), 2);

        let outcome = search_files(&root, &build_matcher("STRASSE", plain()).unwrap());
        assert!(outcome.items.is_empty(), "ß is not folded to ss");
        let outcome = search_files(&root, &build_matcher("straße", plain()).unwrap());
        assert_eq!(
            (outcome.matches, outcome.files, outcome.truncated),
            (2, 2, false)
        );
        let summary = outcome
            .items
            .iter()
            .map(|item| {
                let (line, snippet, range) = item.hit.clone().unwrap();
                (item.label.clone(), line, snippet[range].to_string())
            })
            .collect::<Vec<_>>();
        assert_eq!(
            summary,
            vec![
                ("a.md".to_string(), 1, "Straße".to_string()),
                ("sub/b.markdown".to_string(), 1, "straße".to_string()),
            ]
        );
        let _ = fs::remove_dir_all(root);
    }
}

#[cfg(test)]
mod editor_tests {
    use gpui::{AppContext as _, TestAppContext};

    use crate::editor::Editor;

    #[gpui::test]
    async fn reveal_selects_the_hit_on_its_line(cx: &mut TestAppContext) {
        let markdown = "# Title\n\nfirst Straße\n\nsecond **Straße** here".to_string();
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));
        editor.update(cx, |editor, cx| {
            editor.reveal_search_hit(4, "straße", cx);
            let block = editor
                .active_entity_id
                .and_then(|id| editor.focusable_entity_by_id(id))
                .expect("revealed block");
            let block = block.read(cx);
            let clean = block.current_to_clean_range(block.selected_range.clone());
            assert_eq!(block.clean_text(), "second Straße here");
            assert_eq!(&block.clean_text()[clean], "Straße");
        });
    }
}
