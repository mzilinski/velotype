//! Typing `[[` in a block suggests the notes of the folder below the caret:
//! Up/Down choose, Enter or Tab complete the link to `[[Note]]`, Escape
//! closes the list. Suggestions come from the shared link index.

use std::path::{Path, PathBuf};

use gpui::prelude::FluentBuilder as _;
use gpui::*;

use super::Editor;
use super::link_index::LinkTargets;
use crate::components::{Block, DismissTransientUi, FocusNext, FocusPrev, IndentBlock, Newline};
use crate::theme::Theme;

/// Suggestions shown at most.
const MAX_ITEMS: usize = 8;
const ROW_HEIGHT: f32 = 30.0;
const PANEL_WIDTH: f32 = 320.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Suggestion {
    path: PathBuf,
    /// Text placed between the brackets.
    link: String,
    title: String,
    folder: String,
}

pub(super) struct LinkCompletionState {
    block: Entity<Block>,
    /// Display-text offset just after the `[[`.
    start: usize,
    query: String,
    pub(super) items: Vec<Suggestion>,
    pub(super) selected: usize,
}

/// The unfinished wiki link the caret is in: the offset after its `[[` and
/// the text typed since. None once the link is closed or holds a `|` or `#`
/// (label and heading are not completed).
pub(super) fn open_wiki_query(before_caret: &str) -> Option<(usize, &str)> {
    let start = before_caret.rfind("[[")? + 2;
    let query = &before_caret[start..];
    if query.contains([']', '[', '|', '#', '\n']) || query.chars().count() > 120 {
        return None;
    }
    Some((start, query))
}

/// The documents best matching `query` by name and path, nearest first for
/// equal scores; `current` is left out.
pub(super) fn rank_suggestions(
    targets: &LinkTargets,
    query: &str,
    current: Option<&Path>,
) -> Vec<Suggestion> {
    let query = query.trim();
    let mut scored = targets
        .files()
        .filter(|path| current != Some(path.as_path()))
        .filter_map(|path| {
            let relative = path
                .strip_prefix(targets.root())
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");
            let score = super::workspace_search::fuzzy_score(query, &relative)?;
            Some((score, relative, path))
        })
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    scored
        .into_iter()
        .take(MAX_ITEMS)
        .map(|(_, relative, path)| {
            let folder = Path::new(&relative)
                .parent()
                .map(|parent| parent.to_string_lossy().into_owned())
                .unwrap_or_default();
            Suggestion {
                path: path.clone(),
                link: targets.link_name(path),
                title: path
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                folder,
            }
        })
        .collect()
}

impl Editor {
    /// Opens, updates or closes the suggestions for the focused block after
    /// its text or caret changed.
    pub(super) fn sync_link_completion(&mut self, block: &Entity<Block>, cx: &mut Context<Self>) {
        let (start, query) = {
            let block = block.read(cx);
            let caret = block.cursor_offset();
            let text = block.display_text();
            if block.kind().is_code_block()
                || block.marked_range.is_some()
                || !block.selected_range.is_empty()
                || caret > text.len()
                || !text.is_char_boundary(caret)
            {
                (None, String::new())
            } else {
                match open_wiki_query(&text[..caret]) {
                    Some((start, query)) => (Some(start), query.to_string()),
                    None => (None, String::new()),
                }
            }
        };
        let Some(start) = start else {
            self.close_link_completion(cx);
            return;
        };
        if let Some(state) = self.link_completion.as_ref()
            && state.block == *block
            && state.start == start
            && state.query == query
        {
            return;
        }
        let Some(root) = self.link_root() else {
            return;
        };
        // Starts a background read the first time; the list opens once the
        // index is there and the next keystroke arrives.
        let Some(index) = super::link_index::link_index(&root, true, cx) else {
            self.close_link_completion(cx);
            return;
        };
        let current = self
            .file_path
            .as_deref()
            .and_then(|path| path.canonicalize().ok());
        let items = rank_suggestions(index.targets(), &query, current.as_deref());
        self.link_completion = Some(LinkCompletionState {
            block: block.clone(),
            start,
            query,
            items,
            selected: 0,
        });
        cx.notify();
    }

    pub(super) fn close_link_completion(&mut self, cx: &mut Context<Self>) {
        if self.link_completion.take().is_some() {
            cx.notify();
        }
    }

    /// True while suggestions are shown, so keys go to the list.
    fn link_completion_active(&self) -> bool {
        self.link_completion
            .as_ref()
            .is_some_and(|state| !state.items.is_empty())
    }

    /// Replaces the typed query (and closing brackets right after the caret)
    /// with the chosen link, closed with `]]`, and puts the caret behind it.
    pub(super) fn accept_link_completion(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.link_completion.take() else {
            return;
        };
        let Some(item) = state.items.get(index) else {
            cx.notify();
            return;
        };
        let insert = format!("{}]]", item.link);
        state.block.update(cx, |block, cx| {
            let caret = block.cursor_offset();
            let text = block.display_text();
            let closing = text[caret..]
                .chars()
                .take(2)
                .take_while(|ch| *ch == ']')
                .count();
            block.selected_range = state.start..caret + closing;
            block.selection_reversed = false;
            block.replace_text_in_range(None, &insert, window, cx);
        });
        cx.notify();
    }

    fn move_link_completion(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(state) = self.link_completion.as_mut()
            && !state.items.is_empty()
        {
            let count = state.items.len() as isize;
            state.selected = (state.selected as isize + delta).rem_euclid(count) as usize;
            cx.notify();
        }
    }

    pub(super) fn capture_link_completion_prev(
        &mut self,
        _: &FocusPrev,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.link_completion_active() {
            cx.stop_propagation();
            self.move_link_completion(-1, cx);
        }
    }

    pub(super) fn capture_link_completion_next(
        &mut self,
        _: &FocusNext,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.link_completion_active() {
            cx.stop_propagation();
            self.move_link_completion(1, cx);
        }
    }

    pub(super) fn capture_link_completion_newline(
        &mut self,
        _: &Newline,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.link_completion_active() {
            cx.stop_propagation();
            let selected = self
                .link_completion
                .as_ref()
                .map_or(0, |state| state.selected);
            self.accept_link_completion(selected, window, cx);
        }
    }

    pub(super) fn capture_link_completion_tab(
        &mut self,
        _: &IndentBlock,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.link_completion_active() {
            cx.stop_propagation();
            let selected = self
                .link_completion
                .as_ref()
                .map_or(0, |state| state.selected);
            self.accept_link_completion(selected, window, cx);
        }
    }

    pub(super) fn capture_link_completion_escape(
        &mut self,
        _: &DismissTransientUi,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.link_completion.is_some() {
            cx.stop_propagation();
            self.close_link_completion(cx);
        }
    }

    /// The suggestion list below the caret, or above it near the window's
    /// bottom edge. Closes the list when its block lost focus.
    pub(super) fn render_link_completion(
        &mut self,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let block = self.link_completion.as_ref()?.block.clone();
        if !block.read(cx).focus_handle.is_focused(window) {
            self.link_completion = None;
            return None;
        }
        // The caret may have moved out of the link without an edit.
        self.sync_link_completion(&block, cx);
        let state = self.link_completion.as_ref()?;
        if state.items.is_empty() {
            return None;
        }
        let caret = state.block.read(cx).caret_bounds()?;
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let viewport = window.viewport_size();
        let height = state.items.len() as f32 * ROW_HEIGHT + 2.0 * d.menu_panel_padding;
        let below = caret.bottom() + px(4.0);
        let top = if below + px(height) > viewport.height {
            (caret.top() - px(height + 4.0)).max(px(0.0))
        } else {
            below
        };
        let left = caret
            .left()
            .min(viewport.width - px(PANEL_WIDTH + 8.0))
            .max(px(0.0));
        let selected = state.selected;
        let rows = state.items.iter().enumerate().map(|(index, item)| {
            div()
                .id(("link-completion-item", index))
                .h(px(ROW_HEIGHT))
                .px(px(d.menu_item_padding_x))
                .flex()
                .items_center()
                .gap(px(8.0))
                .rounded(px(d.menu_item_radius))
                .when(index == selected, |row| row.bg(c.selection))
                .hover(|row| row.bg(c.dialog_secondary_button_hover))
                .cursor_pointer()
                .child(
                    div()
                        .flex_shrink_0()
                        .max_w(px(PANEL_WIDTH * 0.6))
                        .text_size(px(d.menu_text_size))
                        .text_color(c.text_default)
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(item.title.clone()),
                )
                .child(
                    div()
                        .min_w(px(0.0))
                        .text_size(px(t.text_size * 0.74))
                        .text_color(c.dialog_muted)
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(item.folder.clone()),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _event, window, cx| {
                        cx.stop_propagation();
                        this.accept_link_completion(index, window, cx);
                    }),
                )
        });
        Some(
            div()
                .id("link-completion")
                .absolute()
                .left(left)
                .top(top)
                .w(px(PANEL_WIDTH))
                .p(px(d.menu_panel_padding))
                .flex()
                .flex_col()
                .bg(c.dialog_surface)
                .border(px(d.dialog_border_width))
                .border_color(c.dialog_border)
                .rounded(px(d.menu_panel_radius))
                .shadow_lg()
                .occlude()
                .children(rows)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{open_wiki_query, rank_suggestions};
    use crate::editor::link_index::LinkTargets;

    #[test]
    fn finds_the_unfinished_wiki_link_before_the_caret() {
        assert_eq!(open_wiki_query("See [[An"), Some((6, "An")));
        assert_eq!(open_wiki_query("[["), Some((2, "")));
        assert_eq!(open_wiki_query("[[Done]] and [[Pl"), Some((15, "Pl")));
        assert_eq!(open_wiki_query("[[Done]] after"), None);
        assert_eq!(open_wiki_query("[[Anna|La"), None);
        assert_eq!(open_wiki_query("[[Anna#Ko"), None);
        assert_eq!(open_wiki_query("one [bracket"), None);
    }

    #[test]
    fn ranks_names_first_and_skips_the_current_document() {
        let files = [
            "people/Anna.md",
            "projects/Plan Anna.md",
            "Annual.md",
            "Home.md",
        ]
        .iter()
        .map(|name| Path::new("/v").join(name))
        .collect::<Vec<PathBuf>>();
        let targets = LinkTargets::new(Path::new("/v"), files);
        let titles = |query: &str, current: Option<&Path>| {
            rank_suggestions(&targets, query, current)
                .into_iter()
                .map(|item| (item.title, item.link, item.folder))
                .collect::<Vec<_>>()
        };
        let anna = titles("anna", Some(Path::new("/v/Home.md")));
        assert_eq!(anna[0], ("Anna".into(), "Anna".into(), "people".into()));
        assert_eq!(titles("plan", None)[0].0, "Plan Anna");
        let all = titles("", Some(Path::new("/v/Home.md")));
        assert_eq!(all.len(), 3);
        assert!(all.iter().all(|(title, _, _)| title != "Home"));
    }
}
