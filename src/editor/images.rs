//! Image files for pasted bitmaps, pasted paths, and dropped image files.
//!
//! Placement follows the `ImagePasteBehavior` preference for saved documents.
//! Untitled documents have no folder yet, so pasted bitmaps go to a shared temp
//! folder and every image is linked by absolute path (Typora behaviour). Saved
//! documents link images inside their folder tree by a relative `./` path.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use gpui::*;

use super::{Editor, ViewMode};
use crate::components::{
    BlockKind, BlockRecord, PastedImageSource, UndoCaptureKind, parse_html_image_block,
};
use crate::config::{ImagePasteBehavior, read_app_preferences};

/// File extensions accepted when image files are dropped onto the editor.
const DROPPABLE_IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp"];

/// Whether `path` is an existing image file that a drop inserts as an image.
pub(super) fn is_droppable_image_path(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                DROPPABLE_IMAGE_EXTENSIONS
                    .iter()
                    .any(|candidate| extension.eq_ignore_ascii_case(candidate))
            })
}

/// Folder receiving pasted bitmaps while the document has never been saved.
pub(super) fn untitled_image_dir() -> PathBuf {
    std::env::temp_dir().join("velotype-images")
}

fn clipboard_image_extension(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Png => "png",
        ImageFormat::Jpeg => "jpg",
        ImageFormat::Webp => "webp",
        ImageFormat::Gif => "gif",
        ImageFormat::Svg => "svg",
        ImageFormat::Bmp => "bmp",
        ImageFormat::Tiff => "tiff",
    }
}

/// Folder that copied images land in for a saved document.
fn saved_document_image_dir(behavior: ImagePasteBehavior, document_path: &Path) -> PathBuf {
    let document_dir = document_path.parent().unwrap_or_else(|| Path::new("."));
    match behavior {
        ImagePasteBehavior::Upload => untitled_image_dir(),
        ImagePasteBehavior::None | ImagePasteBehavior::CopyToDocumentFolder => {
            document_dir.to_path_buf()
        }
        ImagePasteBehavior::CopyToAssetsFolder => document_dir.join("assets"),
        ImagePasteBehavior::CopyToNamedAssetsFolder => {
            let stem = document_path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .filter(|stem| !stem.trim().is_empty())
                .unwrap_or("untitled");
            document_dir.join(format!("{stem}.assets"))
        }
    }
}

/// Returns `dir/preferred_name`, or `dir/<stem>-<n>.<ext>` for the first free
/// `n` when that name is taken.
pub(super) fn unique_file_path(dir: &Path, preferred_name: &str) -> PathBuf {
    let preferred = Path::new(preferred_name);
    let stem = preferred
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("image");
    let extension = preferred.extension().and_then(|ext| ext.to_str());
    for index in 0.. {
        let file_name = if index == 0 {
            preferred_name.to_string()
        } else if let Some(extension) = extension {
            format!("{stem}-{index}.{extension}")
        } else {
            format!("{stem}-{index}")
        };
        let candidate = dir.join(file_name);
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!("unbounded search should always return");
}

fn same_dir(left: &Path, right: &Path) -> bool {
    let left = left.canonicalize().unwrap_or_else(|_| left.to_path_buf());
    let right = right.canonicalize().unwrap_or_else(|_| right.to_path_buf());
    left == right
}

/// Places an image according to `behavior` and returns the path to link.
///
/// `timestamp` names pasted bitmaps (`image-<timestamp>.<ext>`). Local files
/// are linked in place when the behaviour is `None`, when the document is
/// untitled, or when they already sit in the target folder; otherwise they are
/// copied there under a free name.
pub(super) fn materialize_image(
    source: &PastedImageSource,
    behavior: ImagePasteBehavior,
    document_path: Option<&Path>,
    timestamp: &str,
) -> anyhow::Result<PathBuf> {
    match source {
        PastedImageSource::LocalPath(path) => {
            let Some(document_path) = document_path else {
                return Ok(path.clone());
            };
            if matches!(
                behavior,
                ImagePasteBehavior::None | ImagePasteBehavior::Upload
            ) {
                return Ok(path.clone());
            }
            let target_dir = saved_document_image_dir(behavior, document_path);
            if path
                .parent()
                .is_some_and(|parent| same_dir(parent, &target_dir))
            {
                return Ok(path.clone());
            }
            fs::create_dir_all(&target_dir)
                .with_context(|| format!("failed to create '{}'", target_dir.display()))?;
            let file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("image");
            let target = unique_file_path(&target_dir, file_name);
            fs::copy(path, &target).with_context(|| {
                format!(
                    "failed to copy '{}' to '{}'",
                    path.display(),
                    target.display()
                )
            })?;
            Ok(target)
        }
        PastedImageSource::ClipboardImage(image) => {
            let target_dir = document_path
                .map(|document_path| saved_document_image_dir(behavior, document_path))
                .unwrap_or_else(untitled_image_dir);
            fs::create_dir_all(&target_dir)
                .with_context(|| format!("failed to create '{}'", target_dir.display()))?;
            let file_name = format!(
                "image-{timestamp}.{}",
                clipboard_image_extension(image.format)
            );
            let target = unique_file_path(&target_dir, &file_name);
            fs::write(&target, &image.bytes)
                .with_context(|| format!("failed to write '{}'", target.display()))?;
            Ok(target)
        }
    }
}

/// `path` relative to `dir` with `/` separators, when it lies inside `dir`.
fn relative_path_under(dir: &Path, path: &Path) -> Option<String> {
    let join = |relative: &Path| {
        relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/")
    };
    if let Ok(relative) = path.strip_prefix(dir) {
        return Some(join(relative));
    }
    let dir = dir.canonicalize().ok()?;
    let path = path.canonicalize().ok()?;
    path.strip_prefix(&dir).ok().map(join)
}

/// Text written into the image destination: `./relative` inside the document
/// folder, otherwise the absolute path with `/` separators.
pub(super) fn image_link_path(path: &Path, document_dir: Option<&Path>) -> String {
    if let Some(relative) = document_dir.and_then(|dir| relative_path_under(dir, path))
        && !relative.is_empty()
    {
        return format!("./{relative}");
    }
    path.to_string_lossy().replace('\\', "/")
}

/// Encodes a path as a CommonMark link destination: spaces and `%` are
/// percent-encoded (as Typora does) and parentheses and quotes are escaped.
pub(super) fn markdown_link_destination(path: &str) -> String {
    let mut destination = String::with_capacity(path.len());
    for ch in path.chars() {
        match ch {
            ' ' => destination.push_str("%20"),
            '%' => destination.push_str("%25"),
            '<' => destination.push_str("%3C"),
            '>' => destination.push_str("%3E"),
            '\\' | '(' | ')' | '"' => {
                destination.push('\\');
                destination.push(ch);
            }
            _ => destination.push(ch),
        }
    }
    destination
}

fn markdown_image_alt(path: &Path) -> String {
    let alt = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("image");
    let mut escaped = String::with_capacity(alt.len());
    for ch in alt.chars() {
        if matches!(ch, '\\' | '[' | ']') {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

/// `![stem](destination)` for an image file linked from a document whose
/// folder is `document_dir` (`None` for untitled documents).
pub(super) fn image_markdown(path: &Path, document_dir: Option<&Path>) -> String {
    format!(
        "![{}]({})",
        markdown_image_alt(path),
        markdown_link_destination(&image_link_path(path, document_dir))
    )
}

/// `YYYYMMDD-HHMMSS` for the given civil date and time.
fn format_image_timestamp(
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
) -> String {
    format!("{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}")
}

/// Civil (year, month, day) for a count of days since 1970-01-01.
#[cfg_attr(windows, allow(dead_code))]
fn civil_from_unix_days(days: i64) -> (i64, u32, u32) {
    // Howard Hinnant's days-to-civil algorithm.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg_attr(windows, allow(dead_code))]
fn utc_image_timestamp(unix_seconds: i64) -> String {
    let (year, month, day) = civil_from_unix_days(unix_seconds.div_euclid(86_400));
    let seconds_of_day = unix_seconds.rem_euclid(86_400) as u32;
    format_image_timestamp(
        year,
        month,
        day,
        seconds_of_day / 3600,
        seconds_of_day / 60 % 60,
        seconds_of_day % 60,
    )
}

/// Current local time as `YYYYMMDD-HHMMSS` for pasted image names.
///
/// Uses the local clock on Windows; other platforms fall back to UTC because
/// the crate has no time-zone dependency.
pub(super) fn image_timestamp() -> String {
    #[cfg(windows)]
    {
        // SAFETY: GetLocalTime only writes the returned SYSTEMTIME value.
        let now = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
        format_image_timestamp(
            i64::from(now.wYear),
            u32::from(now.wMonth),
            u32::from(now.wDay),
            u32::from(now.wHour),
            u32::from(now.wMinute),
            u32::from(now.wSecond),
        )
    }
    #[cfg(not(windows))]
    {
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs() as i64)
            .unwrap_or_default();
        utc_image_timestamp(seconds)
    }
}

/// Zoom levels offered by the image context menu, in percent.
pub(crate) const IMAGE_ZOOM_LEVELS: [u32; 9] = [25, 33, 50, 67, 75, 80, 100, 150, 200];

fn escape_html_attr(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '"' => escaped.push_str("&quot;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

/// Source text for an image block shown at `percent` zoom: Typora's
/// `<img src=".." style="zoom:NN%;" />` form, or plain Markdown at 100%.
pub(super) fn zoomed_image_source(src: &str, alt: &str, percent: u32) -> String {
    if percent == 100 {
        let mut escaped_alt = String::with_capacity(alt.len());
        for ch in alt.chars() {
            if matches!(ch, '\\' | '[' | ']') {
                escaped_alt.push('\\');
            }
            escaped_alt.push(ch);
        }
        // `src` is already a destination, so existing `%` escapes are kept.
        let mut destination = String::with_capacity(src.len());
        for ch in src.chars() {
            match ch {
                ' ' => destination.push_str("%20"),
                '\\' | '(' | ')' | '"' => {
                    destination.push('\\');
                    destination.push(ch);
                }
                _ => destination.push(ch),
            }
        }
        return format!("![{escaped_alt}]({destination})");
    }
    format!(
        "<img src=\"{}\" alt=\"{}\" style=\"zoom:{percent}%;\" />",
        escape_html_attr(src),
        escape_html_attr(alt)
    )
}

impl Editor {
    /// `(src, alt)` of a standalone image block that the zoom menu can rewrite:
    /// a Markdown image paragraph or an HTML `<img>` block.
    pub(super) fn zoomable_image(&self, entity_id: EntityId, cx: &App) -> Option<(String, String)> {
        let block = self.focusable_entity_by_id(entity_id)?;
        let block = block.read(cx);
        match block.kind() {
            BlockKind::HtmlBlock => {
                let image = parse_html_image_block(&block.record.title.visible_text())?;
                Some((image.src, image.alt))
            }
            BlockKind::Paragraph => block
                .image_runtime()
                .map(|runtime| (runtime.src.clone(), runtime.alt.clone())),
            _ => None,
        }
    }

    /// Replaces an image block with its zoomed source (see
    /// [`zoomed_image_source`]) as one undoable edit.
    pub(super) fn apply_image_zoom(
        &mut self,
        entity_id: EntityId,
        percent: u32,
        cx: &mut Context<Self>,
    ) {
        let Some((src, alt)) = self.zoomable_image(entity_id, cx) else {
            return;
        };
        let Some(block) = self.focusable_entity_by_id(entity_id) else {
            return;
        };
        let is_markdown = block.read(cx).kind() == BlockKind::Paragraph;
        if percent == 100 && is_markdown {
            return;
        }
        let source = zoomed_image_source(&src, &alt, percent);
        let record = if percent == 100 {
            BlockRecord::paragraph(source)
        } else {
            BlockRecord::html(source)
        };
        self.replace_image_block(entity_id, record, cx);
    }

    pub(super) fn current_image_paste_behavior() -> ImagePasteBehavior {
        read_app_preferences()
            .map(|preferences| preferences.image_paste_behavior)
            .unwrap_or(ImagePasteBehavior::None)
    }

    fn document_dir(&self) -> Option<&Path> {
        self.file_path.as_deref().and_then(Path::parent)
    }

    /// Places the image per `behavior` and returns its Markdown image syntax.
    pub(super) fn image_markdown_for_source(
        &self,
        source: &PastedImageSource,
        behavior: ImagePasteBehavior,
    ) -> anyhow::Result<String> {
        let path = materialize_image(
            source,
            behavior,
            self.file_path.as_deref(),
            &image_timestamp(),
        )?;
        Ok(image_markdown(&path, self.document_dir()))
    }

    /// Inserts image blocks for dropped image files, handling the files with
    /// the configured image paste behaviour.
    pub(super) fn insert_dropped_images(
        &mut self,
        paths: &[PathBuf],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let behavior = Self::current_image_paste_behavior();
        let mut markdowns = Vec::with_capacity(paths.len());
        for path in paths {
            match self
                .image_markdown_for_source(&PastedImageSource::LocalPath(path.clone()), behavior)
            {
                Ok(markdown) => markdowns.push(markdown),
                Err(err) => {
                    self.show_image_paste_error(err, cx);
                    return;
                }
            }
        }
        let focused = self.document.focused_block_entity_id(window, cx);
        self.insert_image_markdown_blocks(markdowns.clone(), focused, cx);
        self.upload_inserted_images(&markdowns, cx);
    }

    /// Inserts each Markdown image as its own block after `anchor` (or the
    /// active block, or the end of the document). An empty anchor paragraph is
    /// reused for the first image. In source view the images are inserted as
    /// text at the caret instead.
    pub(super) fn insert_image_markdown_blocks(
        &mut self,
        markdowns: Vec<String>,
        anchor: Option<EntityId>,
        cx: &mut Context<Self>,
    ) {
        if markdowns.is_empty() || self.read_only {
            return;
        }
        self.dismiss_contextual_overlays(cx);
        let anchor = anchor
            .or(self.active_entity_id)
            .and_then(|entity_id| self.focusable_entity_by_id(entity_id));

        if self.view_mode == ViewMode::Source {
            let Some(block) = anchor.or_else(|| self.document.first_root().cloned()) else {
                return;
            };
            let (leading, trailing) = block.read(cx).paste_image_split();
            let mut text = markdowns.join("\n\n");
            if leading.visible_len() > 0 && !leading.visible_text().ends_with('\n') {
                text.insert_str(0, "\n\n");
            }
            if trailing.visible_len() > 0 {
                text.push_str("\n\n");
            }
            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            self.replace_current_block_selection_with_image_text(
                &block, &leading, &text, &trailing, cx,
            );
            self.mark_dirty(cx);
            self.finalize_pending_undo_capture(cx);
            cx.notify();
            return;
        }

        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        let mut markdowns = markdowns.into_iter();
        let mut last_image = None;
        let insert_after = match anchor {
            Some(block)
                if self.table_cell_binding(block.entity_id()).is_none()
                    && block.read(cx).kind() == BlockKind::Paragraph
                    && block.read(cx).display_text().is_empty()
                    && block.read(cx).children.is_empty() =>
            {
                let first = markdowns.next().expect("markdowns is non-empty");
                block.update(cx, |block, cx| {
                    block.record.title = crate::components::InlineTextTree::plain(first);
                    block.sync_edit_mode_from_kind();
                    block.sync_render_cache();
                    cx.notify();
                });
                last_image = Some(block.clone());
                Some(block)
            }
            Some(block)
                if self.table_cell_binding(block.entity_id()).is_none()
                    && block.read(cx).kind() == BlockKind::Paragraph =>
            {
                Some(block)
            }
            Some(block) => {
                let root_id = self.root_ancestor_entity_id(block.entity_id());
                self.document.block_entity_by_id(root_id)
            }
            None => None,
        };

        let new_blocks = markdowns
            .map(|markdown| Self::new_block(cx, BlockRecord::paragraph(markdown)))
            .collect::<Vec<_>>();
        if let Some(block) = new_blocks.last() {
            last_image = Some(block.clone());
        }
        if !new_blocks.is_empty() {
            match insert_after
                .and_then(|block| self.document.find_block_location(block.entity_id()))
            {
                Some(location) => self.document.insert_blocks_at(
                    location.parent,
                    location.index + 1,
                    new_blocks,
                    cx,
                ),
                None => {
                    let end = self.document.root_count();
                    self.document.insert_blocks_at(None, end, new_blocks, cx);
                }
            }
        }

        self.rebuild_image_runtimes(cx);
        if let Some(block) = last_image {
            self.ensure_trailing_paragraph_after_structural(&block, cx);
            self.focus_block(block.entity_id());
        }
        self.mark_dirty(cx);
        self.finalize_pending_undo_capture(cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use gpui::{AppContext as _, Image, ImageFormat, TestAppContext};
    use uuid::Uuid;

    use super::{
        Editor, ImagePasteBehavior, PastedImageSource, image_markdown, is_droppable_image_path,
        markdown_link_destination, materialize_image, untitled_image_dir, utc_image_timestamp,
        zoomed_image_source,
    };
    use crate::components::BlockKind;

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("velotype-images-{name}-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create temp root");
        root
    }

    fn png_image() -> PastedImageSource {
        PastedImageSource::ClipboardImage(Image::from_bytes(
            ImageFormat::Png,
            vec![137, 80, 78, 71],
        ))
    }

    fn init_test_app(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
    }

    #[gpui::test]
    async fn dropped_images_append_blocks_and_reuse_empty_paragraph(cx: &mut TestAppContext) {
        init_test_app(cx);
        let editor = cx.new(|cx| Editor::from_markdown(cx, "intro".to_string(), None));

        editor.update(cx, |editor, cx| {
            editor.insert_image_markdown_blocks(
                vec!["![a](/img/a.png)".into(), "![b](/img/b.png)".into()],
                None,
                cx,
            );
            assert_eq!(
                editor.document.markdown_text(cx).trim_end(),
                "intro\n\n![a](/img/a.png)\n\n![b](/img/b.png)"
            );
            assert!(editor.document_dirty);
            let image = editor.document.root_blocks()[2].clone();
            assert!(image.read(cx).image_runtime().is_some());
        });

        let editor = cx.new(|cx| Editor::from_markdown(cx, String::new(), None));
        editor.update(cx, |editor, cx| {
            let empty = editor.document.first_root().expect("paragraph").entity_id();
            editor.insert_image_markdown_blocks(vec!["![a](/img/a.png)".into()], Some(empty), cx);
            // The image reuses the empty paragraph; a trailing one follows it.
            assert_eq!(editor.document.root_count(), 2);
            assert_eq!(
                editor.document.markdown_text(cx).trim_end(),
                "![a](/img/a.png)"
            );
        });
    }

    #[gpui::test]
    async fn dropped_images_insert_after_anchor_paragraph(cx: &mut TestAppContext) {
        init_test_app(cx);
        let editor = cx.new(|cx| Editor::from_markdown(cx, "one\n\ntwo".to_string(), None));
        editor.update(cx, |editor, cx| {
            let first = editor.document.first_root().expect("first").entity_id();
            editor.insert_image_markdown_blocks(vec!["![a](/img/a.png)".into()], Some(first), cx);
            assert_eq!(
                editor.document.markdown_text(cx),
                "one\n\n![a](/img/a.png)\n\ntwo"
            );
        });
    }

    #[gpui::test]
    async fn image_zoom_rewrites_block_to_html_and_back(cx: &mut TestAppContext) {
        init_test_app(cx);
        let editor =
            cx.new(|cx| Editor::from_markdown(cx, "![shot](./a b.png)\n\nafter".to_string(), None));
        editor.update(cx, |editor, cx| {
            let image = editor.document.first_root().expect("image").entity_id();
            assert!(editor.zoomable_image(image, cx).is_some());
            editor.apply_image_zoom(image, 50, cx);
            assert_eq!(
                editor.document.markdown_text(cx),
                "<img src=\"./a b.png\" alt=\"shot\" style=\"zoom:50%;\" />\n\nafter"
            );
            let html = editor.document.first_root().expect("html").clone();
            assert_eq!(html.read(cx).kind(), BlockKind::HtmlBlock);

            editor.apply_image_zoom(html.entity_id(), 100, cx);
            assert_eq!(
                editor.document.markdown_text(cx),
                "![shot](./a%20b.png)\n\nafter"
            );
            let last = editor
                .document
                .root_blocks()
                .last()
                .expect("text")
                .entity_id();
            assert!(editor.zoomable_image(last, cx).is_none());
        });
    }

    #[test]
    fn zoomed_image_source_escapes_attributes() {
        assert_eq!(
            zoomed_image_source("a\"b.png", "x<y", 75),
            "<img src=\"a&quot;b.png\" alt=\"x&lt;y\" style=\"zoom:75%;\" />"
        );
        assert_eq!(
            zoomed_image_source("./img (1).png", "a]b", 100),
            "![a\\]b](./img%20\\(1\\).png)"
        );
    }

    #[test]
    fn droppable_image_paths_require_existing_image_files() {
        let root = temp_root("droppable");
        let png = root.join("shot.PNG");
        let text = root.join("notes.txt");
        fs::write(&png, b"png").unwrap();
        fs::write(&text, b"txt").unwrap();

        assert!(is_droppable_image_path(&png));
        assert!(!is_droppable_image_path(&text));
        assert!(!is_droppable_image_path(&root.join("missing.png")));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn timestamps_use_civil_dates() {
        assert_eq!(utc_image_timestamp(0), "19700101-000000");
        // 2026-10-11 15:30:12 UTC.
        assert_eq!(utc_image_timestamp(1_791_732_612), "20261011-153012");
        assert_eq!(utc_image_timestamp(951_782_400), "20000229-000000");
    }

    #[test]
    fn link_destinations_encode_spaces_and_escape_parentheses() {
        assert_eq!(
            markdown_link_destination("./my shots/a (1).png"),
            "./my%20shots/a%20\\(1\\).png"
        );
        assert_eq!(markdown_link_destination("100%.png"), "100%25.png");
    }

    #[test]
    fn image_markdown_is_relative_inside_document_folder() {
        let dir = Path::new("/docs/notes");
        assert_eq!(
            image_markdown(&dir.join("assets").join("a b.png"), Some(dir)),
            "![a b](./assets/a%20b.png)"
        );
        assert_eq!(
            image_markdown(Path::new("/elsewhere/x.png"), Some(dir)),
            "![x](/elsewhere/x.png)"
        );
        assert_eq!(
            image_markdown(Path::new("/elsewhere/x.png"), None),
            "![x](/elsewhere/x.png)"
        );
    }

    #[test]
    fn pasted_bitmap_without_copy_behavior_lands_next_to_saved_document() {
        let root = temp_root("paste-none");
        let document = root.join("doc.md");
        let path = materialize_image(
            &png_image(),
            ImagePasteBehavior::None,
            Some(&document),
            "20261011-153012",
        )
        .expect("materialize");
        assert_eq!(path, root.join("image-20261011-153012.png"));
        assert_eq!(
            image_markdown(&path, Some(&root)),
            "![image-20261011-153012](./image-20261011-153012.png)"
        );

        let second = materialize_image(
            &png_image(),
            ImagePasteBehavior::None,
            Some(&document),
            "20261011-153012",
        )
        .expect("materialize again");
        assert_eq!(second, root.join("image-20261011-153012-1.png"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn pasted_bitmap_for_untitled_document_goes_to_temp_folder() {
        let path = materialize_image(
            &png_image(),
            ImagePasteBehavior::CopyToAssetsFolder,
            None,
            &format!("test-{}", Uuid::new_v4()),
        )
        .expect("materialize");
        assert_eq!(path.parent(), Some(untitled_image_dir().as_path()));
        assert!(image_markdown(&path, None).contains(&path.to_string_lossy().replace('\\', "/")));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn local_files_are_copied_into_configured_folders() {
        let root = temp_root("copy");
        let source_dir = temp_root("copy-source");
        let source = source_dir.join("photo.jpg");
        fs::write(&source, b"jpg").unwrap();
        let document = root.join("Trip Notes.md");
        let local = PastedImageSource::LocalPath(source.clone());

        let linked = materialize_image(&local, ImagePasteBehavior::None, Some(&document), "t")
            .expect("link");
        assert_eq!(linked, source);

        let assets = materialize_image(
            &local,
            ImagePasteBehavior::CopyToAssetsFolder,
            Some(&document),
            "t",
        )
        .expect("copy to assets");
        assert_eq!(assets, root.join("assets").join("photo.jpg"));
        assert_eq!(
            image_markdown(&assets, Some(&root)),
            "![photo](./assets/photo.jpg)"
        );

        let named = materialize_image(
            &local,
            ImagePasteBehavior::CopyToNamedAssetsFolder,
            Some(&document),
            "t",
        )
        .expect("copy to named assets");
        assert_eq!(named, root.join("Trip Notes.assets").join("photo.jpg"));

        // A file already in the target folder is linked, not duplicated.
        let again = materialize_image(
            &PastedImageSource::LocalPath(assets.clone()),
            ImagePasteBehavior::CopyToAssetsFolder,
            Some(&document),
            "t",
        )
        .expect("reuse");
        assert_eq!(again, assets);

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&source_dir);
    }
}
