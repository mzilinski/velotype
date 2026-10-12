//! Image upload (Format > Image > Upload Image / Upload All Local Images and
//! the "Upload" image paste behaviour). Like Typora's custom command: the
//! configured command (e.g. `picgo upload`) gets the image files as
//! arguments and prints one URL per image in its last output lines. The
//! local links are then replaced with the URLs as one undo step.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context as _, bail};
use gpui::*;
use pulldown_cmark::{Event, Parser, Tag};

use super::Editor;
use super::markdown_commands::SourceEdit;
use crate::components::{UploadAllImages, UploadImage};
use crate::i18n::I18nManager;

/// Image sources in `markdown` (Markdown images and HTML `<img>`) that point
/// to local files, in document order without duplicates.
pub(crate) fn local_image_sources(markdown: &str) -> Vec<String> {
    let mut sources = Vec::new();
    let mut push = |src: String| {
        if !is_remote(&src) && !src.is_empty() && !sources.contains(&src) {
            sources.push(src);
        }
    };
    for event in Parser::new_ext(markdown, pulldown_cmark::Options::all()) {
        match event {
            Event::Start(Tag::Image { dest_url, .. }) => push(dest_url.to_string()),
            Event::Html(html) | Event::InlineHtml(html) => {
                for src in html_image_sources(&html) {
                    push(src);
                }
            }
            _ => {}
        }
    }
    sources
}

fn html_image_sources(html: &str) -> Vec<String> {
    let pattern = regex::Regex::new(r#"(?i)<img\b[^>]*?\bsrc\s*=\s*["']([^"']+)["']"#)
        .expect("valid image source pattern");
    pattern
        .captures_iter(html)
        .map(|captures| captures[1].to_string())
        .collect()
}

fn is_remote(src: &str) -> bool {
    let lower = src.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("data:")
}

/// The file an image source refers to, relative to `document_dir`.
pub(crate) fn resolve_image_path(src: &str, document_dir: Option<&Path>) -> Option<PathBuf> {
    let decoded = src.replace("%20", " ");
    let path = match decoded.strip_prefix("file://") {
        Some(rest) => {
            let rest = rest
                .strip_prefix('/')
                .filter(|_| cfg!(windows))
                .unwrap_or(rest);
            PathBuf::from(rest)
        }
        None => PathBuf::from(&decoded),
    };
    let path = if path.is_absolute() {
        path
    } else {
        document_dir?.join(path)
    };
    path.is_file().then_some(path)
}

/// The URLs for `count` uploaded images: the last `count` output lines that
/// are http(s) URLs.
pub(crate) fn parse_uploaded_urls(output: &str, count: usize) -> Option<Vec<String>> {
    let urls = output
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("http://") || line.starts_with("https://"))
        .collect::<Vec<_>>();
    (urls.len() >= count && count > 0).then(|| {
        urls[urls.len() - count..]
            .iter()
            .map(|url| url.to_string())
            .collect()
    })
}

/// `markdown` with every Markdown or HTML reference to a source replaced by
/// its URL.
pub(crate) fn replace_image_sources(markdown: &str, replacements: &[(String, String)]) -> String {
    let mut text = markdown.to_string();
    for (src, url) in replacements {
        for (before, after) in [
            (format!("]({src}"), format!("]({url}")),
            (format!("](<{src}>"), format!("]({url}")),
            (format!("src=\"{src}\""), format!("src=\"{url}\"")),
            (format!("src='{src}'"), format!("src='{url}'")),
        ] {
            text = text.replace(&before, &after);
        }
    }
    text
}

/// Runs the upload command with `files` appended as arguments.
pub(crate) fn run_upload_command(command: &str, files: &[PathBuf]) -> anyhow::Result<Vec<String>> {
    let quoted = files
        .iter()
        .map(|file| format!("\"{}\"", file.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(" ");
    let mut process = shell_command(&format!("{command} {quoted}"));
    let output = process
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("failed to run '{command}'"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        bail!(
            "{}\n{}",
            stdout.trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    parse_uploaded_urls(&stdout, files.len()).with_context(|| {
        format!(
            "expected {} URL(s) in the output of '{command}':\n{}",
            files.len(),
            stdout.trim()
        )
    })
}

#[cfg(windows)]
fn shell_command(line: &str) -> Command {
    use std::os::windows::process::CommandExt as _;
    let mut command = Command::new("cmd");
    // CREATE_NO_WINDOW; `/S /C "..."` keeps the inner quotes intact.
    command
        .creation_flags(0x0800_0000)
        .raw_arg(format!("/S /C \"{line}\""));
    command
}

#[cfg(not(windows))]
fn shell_command(line: &str) -> Command {
    let mut command = Command::new("sh");
    command.arg("-c").arg(line);
    command
}

impl Editor {
    fn upload_command() -> String {
        crate::config::read_app_preferences()
            .map(|preferences| preferences.image_upload_command.trim().to_string())
            .unwrap_or_default()
    }

    /// Uploads the local images behind `sources` and replaces their links.
    pub(super) fn upload_image_sources(&mut self, sources: Vec<String>, cx: &mut Context<Self>) {
        if sources.is_empty() || self.read_only {
            return;
        }
        let command = Self::upload_command();
        let strings = cx.global::<I18nManager>().strings().clone();
        if command.is_empty() {
            self.show_upload_error(strings.image_upload_no_command.clone(), cx);
            return;
        }
        let document_dir = self.file_path.as_deref().and_then(Path::parent);
        let resolved = sources
            .into_iter()
            .filter_map(|src| resolve_image_path(&src, document_dir).map(|path| (src, path)))
            .collect::<Vec<_>>();
        if resolved.is_empty() {
            self.show_upload_error(strings.image_upload_nothing.clone(), cx);
            return;
        }
        let files = resolved
            .iter()
            .map(|(_, path)| path.clone())
            .collect::<Vec<_>>();
        let upload = cx
            .background_executor()
            .spawn(async move { run_upload_command(&command, &files) });
        cx.spawn(async move |this, cx| {
            let result = upload.await;
            let _ = this.update(cx, |editor, cx| match result {
                Ok(urls) => {
                    let replacements = resolved
                        .into_iter()
                        .map(|(src, _)| src)
                        .zip(urls)
                        .collect::<Vec<_>>();
                    editor.replace_uploaded_images(&replacements, cx);
                }
                Err(err) => editor.show_upload_error(err.to_string(), cx),
            });
        })
        .detach();
    }

    fn replace_uploaded_images(
        &mut self,
        replacements: &[(String, String)],
        cx: &mut Context<Self>,
    ) {
        let source = self.current_document_source(cx);
        let replaced = replace_image_sources(&source, replacements);
        let selection = self.capture_source_selection_snapshot(cx).range;
        let end = replaced.len();
        self.apply_source_edit(
            SourceEdit {
                source: replaced,
                selection: selection.start.min(end)..selection.end.min(end),
            },
            cx,
        );
    }

    fn show_upload_error(&self, detail: String, cx: &mut Context<Self>) {
        let strings = cx.global::<I18nManager>().strings().clone();
        if let Some(window) = cx.active_window() {
            let _ = window.update(cx, |_view, window, cx| {
                let buttons = [strings.info_dialog_ok.as_str()];
                let _ = window.prompt(
                    PromptLevel::Critical,
                    &strings.image_upload_failed_title,
                    Some(&detail),
                    &buttons,
                    cx,
                );
            });
        }
    }

    /// Uploads the images of freshly inserted Markdown when the image paste
    /// behaviour is "Upload".
    pub(super) fn upload_inserted_images(&mut self, markdowns: &[String], cx: &mut Context<Self>) {
        if Self::current_image_paste_behavior() != crate::config::ImagePasteBehavior::Upload {
            return;
        }
        let sources = markdowns
            .iter()
            .flat_map(|markdown| local_image_sources(markdown))
            .collect();
        self.upload_image_sources(sources, cx);
    }

    pub(super) fn on_upload_image(
        &mut self,
        _: &UploadImage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let image = self
            .document
            .focused_block_entity_id(window, cx)
            .or(self.active_entity_id)
            .and_then(|id| self.zoomable_image(id, cx));
        match image {
            Some((src, _)) => self.upload_image_sources(vec![src], cx),
            None => {
                let strings = cx.global::<I18nManager>().strings().clone();
                self.show_upload_error(strings.image_upload_nothing.clone(), cx);
            }
        }
    }

    pub(super) fn on_upload_all_images(
        &mut self,
        _: &UploadAllImages,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let sources = local_image_sources(&self.current_document_source(cx));
        if sources.is_empty() {
            let strings = cx.global::<I18nManager>().strings().clone();
            self.show_upload_error(strings.image_upload_nothing.clone(), cx);
            return;
        }
        self.upload_image_sources(sources, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        local_image_sources, parse_uploaded_urls, replace_image_sources, resolve_image_path,
        run_upload_command,
    };

    #[test]
    fn finds_local_markdown_and_html_images() {
        let markdown = "![a](img/a.png) ![b](https://x.org/b.png)\n\n<img src=\"c.jpg\" />\n\n![a again](img/a.png)";
        assert_eq!(local_image_sources(markdown), ["img/a.png", "c.jpg"]);
    }

    #[test]
    fn parses_the_last_url_lines() {
        let output =
            "[PicGo INFO]: uploading\n[PicGo SUCCESS]:\nhttps://cdn/a.png\nhttps://cdn/b.png\n";
        assert_eq!(
            parse_uploaded_urls(output, 2).unwrap(),
            ["https://cdn/a.png", "https://cdn/b.png"]
        );
        assert_eq!(
            parse_uploaded_urls(output, 1).unwrap(),
            ["https://cdn/b.png"]
        );
        assert!(parse_uploaded_urls(output, 3).is_none());
        assert!(parse_uploaded_urls("error", 1).is_none());
    }

    #[test]
    fn replaces_markdown_and_html_references() {
        let markdown = "![a](img/a.png \"t\") ![x](<my pic.png>) <img src=\"img/a.png\">";
        let replaced = replace_image_sources(
            markdown,
            &[
                ("img/a.png".into(), "https://cdn/a.png".into()),
                ("my pic.png".into(), "https://cdn/p.png".into()),
            ],
        );
        assert_eq!(
            replaced,
            "![a](https://cdn/a.png \"t\") ![x](https://cdn/p.png) <img src=\"https://cdn/a.png\">"
        );
    }

    #[test]
    fn resolves_relative_and_encoded_paths() {
        let dir = std::env::temp_dir().join(format!("velotype-upload-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("img")).unwrap();
        std::fs::write(dir.join("img").join("my pic.png"), b"png").unwrap();
        assert_eq!(
            resolve_image_path("img/my%20pic.png", Some(&dir)),
            Some(dir.join("img/my pic.png"))
        );
        assert_eq!(resolve_image_path("img/missing.png", Some(&dir)), None);
        assert_eq!(resolve_image_path("img/my pic.png", None), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn upload_command_output_is_parsed() {
        let dir = std::env::temp_dir().join(format!("velotype-upload-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a b.png");
        std::fs::write(&file, b"png").unwrap();
        // A stand-in uploader that prints one URL per argument.
        let command = if cfg!(windows) {
            "echo log line & echo https://cdn/uploaded.png & rem"
        } else {
            "printf 'log line\\nhttps://cdn/uploaded.png\\n' #"
        };
        let urls = run_upload_command(command, std::slice::from_ref(&file)).unwrap();
        assert_eq!(urls, ["https://cdn/uploaded.png"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[gpui::test]
    async fn uploaded_urls_replace_links_in_one_undo_step(cx: &mut gpui::TestAppContext) {
        use crate::editor::Editor;
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let source = "Intro

![a](img/a.png)
";
        let (editor, cx) =
            cx.add_window_view(|_window, cx| Editor::from_markdown(cx, source.to_string(), None));
        editor.update(cx, |editor, cx| {
            editor.replace_uploaded_images(
                &[("img/a.png".to_string(), "https://cdn/a.png".to_string())],
                cx,
            );
            assert!(
                editor
                    .current_document_source(cx)
                    .contains("![a](https://cdn/a.png)")
            );
            assert!(editor.document_dirty);
            editor.undo_document(cx);
            assert!(
                editor
                    .current_document_source(cx)
                    .contains("![a](img/a.png)")
            );
        });
    }

    /// A real uploader script in a folder with spaces gets every image path
    /// intact and in order.
    #[test]
    fn upload_script_receives_paths_with_spaces() {
        let dir = std::env::temp_dir()
            .join(format!("velotype upload {}", uuid::Uuid::new_v4()))
            .join("Bilder Ordner");
        std::fs::create_dir_all(&dir).unwrap();
        let files = [dir.join("erstes bild.png"), dir.join("zweites.png")];
        for file in &files {
            std::fs::write(file, b"png").unwrap();
        }
        let command = if cfg!(windows) {
            let script = dir.join("upload.cmd");
            let body = [
                "@echo off",
                "echo uploading %*",
                ":loop",
                "if \"%~1\"==\"\" goto :eof",
                "if not exist \"%~1\" exit /b 1",
                "echo https://cdn/%~nx1",
                "shift",
                "goto loop",
            ]
            .join("\r\n");
            std::fs::write(&script, body).unwrap();
            format!("\"{}\"", script.display())
        } else {
            let script = dir.join("upload.sh");
            let body = "#!/bin/sh\necho uploading\nfor f in \"$@\"; do\n  [ -f \"$f\" ] || exit 1\n  echo \"https://cdn/$(basename \"$f\")\"\ndone\n";
            std::fs::write(&script, body).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
            format!("\x27{}\x27", script.display())
        };
        let urls = run_upload_command(&command, &files).unwrap();
        assert_eq!(
            urls,
            ["https://cdn/erstes bild.png", "https://cdn/zweites.png"]
        );
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }
}
