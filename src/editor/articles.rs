//! Sidebar > Articles: the Markdown files of the workspace folder as a flat
//! list, most recently changed first, each with its title (first heading or
//! file name), date and the start of its text. Read in the background.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use gpui::prelude::FluentBuilder as _;
use gpui::*;

use super::Editor;
use crate::i18n::I18nStrings;
use crate::theme::Theme;

/// Articles listed at most (the newest ones).
const MAX_ARTICLES: usize = 500;
/// Bytes read per file for title and excerpt.
const PREVIEW_BYTES: usize = 4096;
const EXCERPT_CHARS: usize = 120;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Article {
    pub(super) path: PathBuf,
    pub(super) title: String,
    pub(super) excerpt: String,
    pub(super) modified: Option<SystemTime>,
}

/// Title (first heading, else `fallback`) and excerpt (first lines of text)
/// of a Markdown document's beginning.
pub(crate) fn title_and_excerpt(markdown: &str, fallback: &str) -> (String, String) {
    let mut lines = markdown.lines().map(str::trim).peekable();
    // Skip YAML front matter.
    if lines
        .peek()
        .is_some_and(|line| line.trim_start_matches('\u{feff}') == "---")
    {
        lines.next();
        for line in lines.by_ref() {
            if line == "---" || line == "..." {
                break;
            }
        }
    }
    let mut title = None;
    let mut excerpt = String::new();
    let mut in_fence = false;
    for line in lines {
        if line.starts_with("```") || line.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence || line.is_empty() || line.starts_with("<!--") || line.starts_with('|') {
            continue;
        }
        if let Some(heading) = line.strip_prefix('#') {
            let heading = heading.trim_start_matches('#').trim();
            if title.is_none() && !heading.is_empty() {
                title = Some(heading.replace(['*', '_', '`'], ""));
            }
            continue;
        }
        let text = line
            .trim_start_matches(['>', '-', '*', '+', ' '])
            .replace(['*', '_', '`'], "");
        if text.is_empty() {
            continue;
        }
        if !excerpt.is_empty() {
            excerpt.push(' ');
        }
        excerpt.push_str(&text);
        if excerpt.chars().count() >= EXCERPT_CHARS {
            break;
        }
    }
    let excerpt = if excerpt.chars().count() > EXCERPT_CHARS {
        format!(
            "{}…",
            excerpt.chars().take(EXCERPT_CHARS).collect::<String>()
        )
    } else {
        excerpt
    };
    (title.unwrap_or_else(|| fallback.to_string()), excerpt)
}

/// The articles below `root`, newest first.
pub(super) fn collect_articles(root: &Path) -> Vec<Article> {
    let mut files = super::workspace_search::collect_markdown_files(root)
        .into_iter()
        .map(|path| {
            let modified = std::fs::metadata(&path)
                .and_then(|meta| meta.modified())
                .ok();
            (path, modified)
        })
        .collect::<Vec<_>>();
    files.sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(&right.0)));
    files
        .into_iter()
        .take(MAX_ARTICLES)
        .map(|(path, modified)| {
            let start = std::fs::File::open(&path)
                .and_then(|file| {
                    use std::io::Read as _;
                    let mut buffer = Vec::with_capacity(PREVIEW_BYTES);
                    file.take(PREVIEW_BYTES as u64).read_to_end(&mut buffer)?;
                    Ok(buffer)
                })
                .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
                .unwrap_or_default();
            let fallback = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().to_string())
                .unwrap_or_default();
            let (title, excerpt) = title_and_excerpt(&start, &fallback);
            Article {
                path,
                title,
                excerpt,
                modified,
            }
        })
        .collect()
}

fn format_date(time: SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(time)
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

impl Editor {
    /// Reads the articles of the workspace folder in the background, unless
    /// they are loaded (or loading) already.
    pub(super) fn sync_workspace_articles(&mut self, cx: &mut Context<Self>) {
        if self.workspace.articles.is_some() || self.workspace.articles_task.is_some() {
            return;
        }
        let Some(root) = self.workspace_root() else {
            self.workspace.articles = Some(Vec::new());
            return;
        };
        let scan = cx
            .background_executor()
            .spawn(async move { collect_articles(&root) });
        self.workspace.articles_task = Some(cx.spawn(async move |this, cx| {
            let articles = scan.await;
            let _ = this.update(cx, |editor, cx| {
                editor.workspace.articles = Some(articles);
                editor.workspace.articles_task = None;
                cx.notify();
            });
        }));
    }

    pub(super) fn render_workspace_articles(
        &self,
        theme: &Theme,
        strings: &I18nStrings,
        editor: &WeakEntity<Editor>,
    ) -> AnyElement {
        let c = &theme.colors;
        let t = &theme.typography;
        let Some(articles) = self.workspace.articles.as_ref() else {
            return self.render_workspace_empty_state(
                "",
                &strings.workspace_articles_loading,
                theme,
            );
        };
        if articles.is_empty() {
            return self.render_workspace_empty_state("", &strings.workspace_empty_files, theme);
        }
        let current = self.file_path.clone();
        let rows = articles.iter().enumerate().map(|(index, article)| {
            let editor = editor.clone();
            let path = article.path.clone();
            let active = current
                .as_deref()
                .is_some_and(|current| super::tabs::same_file(current, &article.path));
            let date = article.modified.map(format_date).unwrap_or_default();
            div()
                .id(ElementId::named_usize("workspace-article", index))
                .w_full()
                .px(px(10.0))
                .py(px(8.0))
                .flex()
                .flex_col()
                .gap(px(2.0))
                .rounded(px(6.0))
                .when(active, |row| row.bg(c.selection))
                .hover(|row| row.bg(c.dialog_secondary_button_hover))
                .cursor_pointer()
                .child(
                    div()
                        .text_size(px(t.text_size * 0.92))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(c.text_default)
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(article.title.clone()),
                )
                .child(
                    div()
                        .text_size(px(t.text_size * 0.74))
                        .text_color(c.dialog_muted)
                        .child(date),
                )
                .when(!article.excerpt.is_empty(), |row| {
                    row.child(
                        div()
                            .text_size(px(t.text_size * 0.8))
                            .line_height(px(t.text_size * 0.8 * 1.4))
                            .text_color(c.dialog_muted)
                            .max_h(px(t.text_size * 0.8 * 1.4 * 2.0))
                            .overflow_hidden()
                            .child(article.excerpt.clone()),
                    )
                })
                .on_click(move |_event, window, cx| {
                    let path = path.clone();
                    let _ = editor.update(cx, |editor, cx| {
                        editor.open_workspace_file(path, window, cx);
                    });
                })
        });
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .children(rows)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{collect_articles, title_and_excerpt};

    #[test]
    fn titles_and_excerpts_skip_markup() {
        let (title, excerpt) = title_and_excerpt(
            "---\ntitle: x\n---\n\n# My **Note**\n\nSome *text* here.\n\n- item\n\n```\ncode\n```",
            "file",
        );
        assert_eq!(title, "My Note");
        assert_eq!(excerpt, "Some text here. item");
        let (title, excerpt) = title_and_excerpt("Just text", "file");
        assert_eq!((title.as_str(), excerpt.as_str()), ("file", "Just text"));
        let long = "word ".repeat(60);
        let (_, excerpt) = title_and_excerpt(&long, "f");
        assert!(excerpt.ends_with('…'));
        assert_eq!(excerpt.chars().count(), 121);
    }

    #[test]
    fn articles_are_listed_newest_first() {
        let dir = std::env::temp_dir().join(format!("velotype-articles-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("old.md"), "# Old\n\nfirst").unwrap();
        let old = std::fs::File::options()
            .write(true)
            .open(dir.join("old.md"))
            .unwrap();
        old.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600))
            .unwrap();
        std::fs::write(dir.join("sub").join("new.md"), "# New\n\nsecond").unwrap();
        std::fs::write(dir.join("notes.txt"), "not markdown").unwrap();
        let articles = collect_articles(&dir);
        let titles = articles
            .iter()
            .map(|article| article.title.as_str())
            .collect::<Vec<_>>();
        assert_eq!(titles, ["New", "Old"]);
        let _ = std::fs::remove_dir_all(dir);
    }
}
