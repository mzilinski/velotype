//! Workspace palette: Quick Open (Ctrl/Cmd+P) and Search in Folder
//! (Ctrl/Cmd+Shift+F).
//!
//! Both work on the workspace root like the drawer: the opened folder, or else
//! the folder of the current file.
//! Quick Open fuzzy-filters the Markdown files below it (plus recent files);
//! Search in Folder scans their contents on a background thread and lists
//! matching lines. Opening goes through the tab host when there is one, and a
//! search hit is revealed in the opened document.

use std::ops::Range;
use std::path::{Path, PathBuf};

use gpui::prelude::FluentBuilder as _;
use gpui::*;

use super::Editor;
use super::find::find_ranges;
use crate::components::single_line_input::{InputChanged, SingleLineInput, SingleLineOverflow};
use crate::components::{QuickOpen, SearchInWorkspace};
use crate::i18n::I18nManager;
use crate::theme::Theme;

/// Files collected for Quick Open and searched by Search in Folder.
const MAX_FILES: usize = 5000;
/// Results listed at once.
const MAX_RESULTS: usize = 200;
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
            } else if file_type.is_file() && is_markdown_path(&path) {
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

fn is_markdown_path(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        let extension = extension.to_string_lossy();
        extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown")
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

/// Matching lines of every file, case-insensitive, at most `MAX_RESULTS`.
pub(super) fn search_files(root: &Path, query: &str) -> Vec<PaletteItem> {
    let mut items = Vec::new();
    for path in collect_markdown_files(root) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (line_index, line) in text.lines().enumerate() {
            let Some(found) = find_ranges(line, query, false).into_iter().next() else {
                continue;
            };
            let trimmed_start = line.len() - line.trim_start().len();
            let snippet = line.trim().to_string();
            let range = found.start.saturating_sub(trimmed_start)
                ..found.end.saturating_sub(trimmed_start).min(snippet.len());
            items.push(PaletteItem {
                label: relative_label(Some(root), &path),
                path: path.clone(),
                hit: Some((line_index, snippet, range)),
            });
            if items.len() >= MAX_RESULTS {
                return items;
            }
        }
    }
    items
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
                    .take(MAX_RESULTS)
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
                palette._search = None;
                let Some(root) = palette.root.clone() else {
                    cx.notify();
                    return;
                };
                if query.chars().count() < 2 {
                    cx.notify();
                    return;
                }
                let generation = palette.generation;
                let search = cx
                    .background_executor()
                    .spawn(async move { search_files(&root, &query) });
                palette._search = Some(cx.spawn(async move |this, cx| {
                    let items = search.await;
                    let _ = this.update(cx, |this, cx| {
                        if let Some(palette) = this.palette.as_mut()
                            && palette.generation == generation
                        {
                            palette.items = items;
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
        let query = palette.input.read(cx).value().trim().to_string();
        let Some(item) = palette.items.get(index).cloned() else {
            return;
        };
        cx.notify();
        let reveal = item.hit.map(|(line, _, _)| (line, query));
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
        match event.keystroke.key.as_str() {
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
            PaletteMode::SearchInFolder if !query_empty && palette.items.is_empty() => {
                Some(strings.workspace_search_no_results.clone())
            }
            PaletteMode::QuickOpen if palette.items.is_empty() => {
                Some(strings.workspace_empty_files.clone())
            }
            _ => None,
        };

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
                        .child(palette.input.clone())
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

    use super::{collect_markdown_files, fuzzy_score, search_files};

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

        let hits = search_files(&root, "STRASSE");
        assert!(hits.is_empty(), "ß is not folded to ss");
        let hits = search_files(&root, "straße");
        let summary = hits
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
