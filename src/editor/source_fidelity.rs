//! Keeps the Markdown of a loaded file as it was written. The editor works on
//! blocks and serializes them in a normalized form (blank lines between
//! blocks, fenced code, escaped `~` and backticks, ...). Saving that form
//! would rewrite every file that is only opened and saved, so the text of
//! regions that were not edited since loading is taken from the original.
//!
//! At load time the importer reports which original lines each top-level
//! region came from; that region's normalized lines are located in the
//! normalized document. On save, a line diff between the normalized text at
//! load time and the current normalized text shows which regions are still
//! untouched; those (and the blank lines between them) are written from the
//! original, the rest as normalized.

use std::ops::Range;

use similar::{Algorithm, DiffOp, capture_diff_slices};

/// One region of the loaded document: original lines and normalized lines.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Piece {
    original: Range<usize>,
    normalized: Range<usize>,
    /// Blank lines between regions (or before the first / after the last).
    gap: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SourceFidelity {
    original: Vec<String>,
    normalized: Vec<String>,
    pieces: Vec<Piece>,
}

/// The rewritten text and, per line of the current normalized text, the line
/// of the rewritten text it ended up in (for caret mapping).
pub(crate) struct Restored {
    pub(crate) text: String,
    pub(crate) line_map: Vec<usize>,
}

fn lines_of(text: &str) -> Vec<String> {
    text.split('\n').map(ToOwned::to_owned).collect()
}

impl SourceFidelity {
    /// `regions` are the original line ranges of the top-level regions, in
    /// order, each with its normalized text. `None` when a region's
    /// normalized text cannot be found in `normalized_text` in order (the
    /// document is then saved normalized, as before).
    pub(crate) fn build(
        original_text: &str,
        regions: &[(Range<usize>, String)],
        normalized_text: &str,
    ) -> Option<Self> {
        let original = lines_of(original_text);
        let normalized = lines_of(normalized_text);
        let mut pieces = Vec::with_capacity(regions.len() * 2 + 1);
        let mut original_cursor = 0;
        let mut normalized_cursor = 0;
        for (original_range, region_text) in regions {
            if original_range.start < original_cursor || original_range.end > original.len() {
                return None;
            }
            let region = lines_of(region_text);
            // The region follows after blank separator lines.
            let mut start = normalized_cursor;
            while normalized
                .get(start)
                .is_some_and(|line| line.trim().is_empty())
                && normalized.get(start..start + region.len()) != Some(region.as_slice())
            {
                start += 1;
            }
            if normalized.get(start..start + region.len()) != Some(region.as_slice()) {
                return None;
            }
            pieces.push(Piece {
                original: original_cursor..original_range.start,
                normalized: normalized_cursor..start,
                gap: true,
            });
            pieces.push(Piece {
                original: original_range.clone(),
                normalized: start..start + region.len(),
                gap: false,
            });
            original_cursor = original_range.end;
            normalized_cursor = start + region.len();
        }
        pieces.push(Piece {
            original: original_cursor..original.len(),
            normalized: normalized_cursor..normalized.len(),
            gap: true,
        });
        Some(Self {
            original,
            normalized,
            pieces,
        })
    }

    /// The text to save (or show in source mode) for the current normalized
    /// text: untouched regions as originally written, edited ones normalized.
    pub(crate) fn restore(&self, current_text: &str) -> Restored {
        let current = lines_of(current_text);
        // Line of the current text each load-time normalized line still is.
        let mut kept: Vec<Option<usize>> = vec![None; self.normalized.len()];
        for op in capture_diff_slices(Algorithm::Myers, &self.normalized, &current) {
            if let DiffOp::Equal {
                old_index,
                new_index,
                len,
            } = op
            {
                for offset in 0..len {
                    kept[old_index + offset] = Some(new_index + offset);
                }
            }
        }
        // Where an untouched piece now sits in the current text.
        let placement = |piece: &Piece| -> Option<Range<usize>> {
            let first = kept.get(piece.normalized.start).copied().flatten()?;
            for (offset, line) in piece.normalized.clone().enumerate() {
                if kept[line] != Some(first + offset) {
                    return None;
                }
            }
            Some(first..first + piece.normalized.len())
        };
        let placements = self.pieces.iter().map(placement).collect::<Vec<_>>();

        // Current line -> piece emitted from the original at that line.
        let mut from_original: Vec<Option<usize>> = vec![None; current.len()];
        // Gaps without normalized lines are emitted between their neighbours.
        let mut gap_after_line: Vec<Option<usize>> = vec![None; current.len() + 1];
        for (index, piece) in self.pieces.iter().enumerate() {
            if !piece.normalized.is_empty() {
                if let Some(range) = &placements[index] {
                    for line in range.clone() {
                        from_original[line] = Some(index);
                    }
                }
                continue;
            }
            if piece.original.is_empty() {
                continue;
            }
            // An empty normalized gap sits between two regions (or a region
            // and the document edge); keep it when both sides are untouched
            // and still adjacent.
            let before = index
                .checked_sub(1)
                .map(|previous| placements[previous].clone().map(|range| range.end));
            let after = self
                .pieces
                .get(index + 1)
                .map(|_| placements[index + 1].clone().map(|range| range.start));
            let position = match (before, after) {
                (None, Some(Some(0))) => Some(0),
                (Some(Some(end)), None) if end == current.len() => Some(end),
                (Some(Some(end)), Some(Some(start))) if end == start => Some(end),
                _ => None,
            };
            if let Some(position) = position {
                gap_after_line[position] = Some(index);
            }
        }

        let mut lines: Vec<&str> = Vec::with_capacity(current.len());
        let mut line_map = Vec::with_capacity(current.len());
        let mut line = 0;
        while line <= current.len() {
            if let Some(gap) = gap_after_line[line] {
                lines.extend(
                    self.original[self.pieces[gap].original.clone()]
                        .iter()
                        .map(String::as_str),
                );
            }
            if line == current.len() {
                break;
            }
            match from_original[line] {
                Some(index) => {
                    let piece = &self.pieces[index];
                    let start = lines.len();
                    lines.extend(
                        self.original[piece.original.clone()]
                            .iter()
                            .map(String::as_str),
                    );
                    let emitted = piece.original.len();
                    for offset in 0..piece.normalized.len() {
                        line_map.push(start + offset.min(emitted.saturating_sub(1)));
                    }
                    line += piece.normalized.len();
                }
                None => {
                    line_map.push(lines.len());
                    lines.push(&current[line]);
                    line += 1;
                }
            }
        }
        // `line_map` may point past the end when a piece emitted no lines.
        let last = lines.len().saturating_sub(1);
        for target in &mut line_map {
            *target = (*target).min(last);
        }
        Restored {
            text: lines.join("\n"),
            line_map,
        }
    }

    /// The normalized line an original line belongs to (its region's first
    /// normalized line plus the offset, clamped), for caret mapping when the
    /// original is parsed again.
    pub(crate) fn normalized_line_for_original(&self, line: usize) -> usize {
        let piece = self
            .pieces
            .iter()
            .find(|piece| piece.original.contains(&line))
            .or_else(|| self.pieces.iter().rev().find(|piece| !piece.gap));
        let Some(piece) = piece else {
            return 0;
        };
        let offset = line.saturating_sub(piece.original.start);
        let target = piece.normalized.start + offset.min(piece.normalized.len().saturating_sub(1));
        target.min(self.normalized.len().saturating_sub(1))
    }
}

/// Moves a byte offset from `from` to `to` through a line mapping, keeping
/// the column where the target line is long enough.
pub(crate) fn map_offset(
    from: &str,
    to: &str,
    offset: usize,
    map_line: impl Fn(usize) -> usize,
) -> usize {
    let offset = offset.min(from.len());
    let line = from[..offset].matches('\n').count();
    let line_start = from[..offset].rfind('\n').map_or(0, |index| index + 1);
    let column = offset - line_start;
    let target_line = map_line(line);
    let mut start = 0;
    for (index, text) in to.split('\n').enumerate() {
        if index == target_line {
            let mut end = start + column.min(text.len());
            while !to.is_char_boundary(end) {
                end -= 1;
            }
            return end;
        }
        start += text.len() + 1;
    }
    to.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The PowerShell script from the bug report, roughly: comment lines
    /// read as headings, an indented block read as code.
    const ORIGINAL: &str = "# Title\n# Second\ntext ~5 and `n\n    indented\n    code\nend\n";

    fn fixture() -> SourceFidelity {
        // Regions as the importer reports them, with their normalized text.
        let regions = vec![
            (0..1, "# Title".to_string()),
            (1..2, "# Second".to_string()),
            (2..3, "text \\~5 and \\`n".to_string()),
            (3..5, "```\nindented\ncode\n```".to_string()),
            (5..6, "end".to_string()),
        ];
        let normalized =
            "# Title\n\n# Second\n\ntext \\~5 and \\`n\n\n```\nindented\ncode\n```\n\nend";
        SourceFidelity::build(ORIGINAL, &regions, normalized).expect("regions found")
    }

    #[test]
    fn unchanged_document_is_restored_exactly() {
        let fidelity = fixture();
        let normalized = fidelity.normalized.join("\n");
        assert_eq!(fidelity.restore(&normalized).text, ORIGINAL);
    }

    #[test]
    fn only_the_edited_region_is_normalized() {
        let fidelity = fixture();
        let edited = fidelity
            .normalized
            .join("\n")
            .replace("# Second", "# Second edited");
        let restored = fidelity.restore(&edited).text;
        assert_eq!(
            restored,
            "# Title\n# Second edited\ntext ~5 and `n\n    indented\n    code\nend\n"
        );
    }

    #[test]
    fn inserted_blocks_keep_the_rest_original() {
        let fidelity = fixture();
        let edited = fidelity
            .normalized
            .join("\n")
            .replace("\n\nend", "\n\nnew paragraph\n\nend");
        let restored = fidelity.restore(&edited).text;
        assert!(
            restored.starts_with("# Title\n# Second\ntext ~5 and `n\n    indented\n    code\n"),
            "{restored}"
        );
        assert!(restored.contains("new paragraph"), "{restored}");
        assert!(restored.ends_with("end\n"), "{restored}");
    }

    #[test]
    fn deleted_regions_disappear() {
        let fidelity = fixture();
        let edited = fidelity.normalized.join("\n").replace("# Second\n\n", "");
        let restored = fidelity.restore(&edited).text;
        assert!(!restored.contains("Second"), "{restored}");
        assert!(
            restored.contains("text ~5 and `n\n    indented"),
            "{restored}"
        );
    }

    #[test]
    fn missing_region_disables_fidelity() {
        let regions = vec![(0..1, "# Other".to_string())];
        assert!(SourceFidelity::build("# Title", &regions, "# Title").is_none());
    }

    #[test]
    fn offsets_follow_lines() {
        let fidelity = fixture();
        let normalized = fidelity.normalized.join("\n");
        let restored = fidelity.restore(&normalized);
        // Caret on "end" (normalized line 11) lands on "end" in the original.
        let offset = normalized.find("end").unwrap() + 2;
        let mapped = map_offset(&normalized, &restored.text, offset, |line| {
            restored.line_map[line]
        });
        assert_eq!(&restored.text[mapped - 2..mapped], "en");
        // And back: original line 5 ("end") is normalized line 11.
        assert_eq!(fidelity.normalized_line_for_original(5), 11);
    }
}

/// Remembers the original text of the regions `document` was just built
/// from (`regions` from [`super::Editor::build_root_blocks_with_regions`]).
/// Call it once the roots are in the document: serializing depends on the
/// document metadata (e.g. the numbers of ordered list items).
pub(super) fn fidelity_for_document(
    document: &super::tree::DocumentTree,
    original: &str,
    regions: &[super::document::SourceRegion],
    cx: &gpui::App,
) -> Option<SourceFidelity> {
    let roots = document.roots();
    let region_texts = regions
        .iter()
        .map(|region| {
            let mut lines = Vec::new();
            super::tree::DocumentTree::collect_root_markdown_lines(
                roots.get(region.roots.clone())?,
                cx,
                &mut lines,
            );
            Some((region.lines.clone(), lines.join("\n")))
        })
        .collect::<Option<Vec<_>>>()?;
    SourceFidelity::build(original, &region_texts, &document.markdown_text(cx))
}

impl super::Editor {
    /// `normalized` (the serialized document) with unedited regions written
    /// as they were loaded.
    pub(super) fn restore_original_source(&self, normalized: String) -> String {
        match &self.source_fidelity {
            Some(fidelity) => fidelity.restore(&normalized).text,
            None => normalized,
        }
    }
}

#[cfg(test)]
mod editor_tests {
    use gpui::TestAppContext;

    use crate::editor::{Editor, ViewMode};

    /// Like the PowerShell script from the bug report: `#` comment lines,
    /// indented lines, `~` and a lone backtick, CRLF and a final newline.
    const SCRIPT: &str = "# Kommentar eins\r\n# Kommentar zwei\r\n$drives = @{\r\n    \"P:\" = \"data\"\r\n    \"M:\" = \"music\"\r\n}\r\n# Warten (max. ~5 Minuten)\r\n$m = (mount.exe) -join \"`n\"\r\n";

    fn init(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
    }

    fn temp_file(content: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("velotype-fidelity-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("script.md");
        std::fs::write(&path, content).unwrap();
        path
    }

    #[gpui::test]
    async fn open_and_save_keeps_the_file_byte_for_byte(cx: &mut TestAppContext) {
        init(cx);
        let path = temp_file(SCRIPT);
        let (editor, cx) = cx.add_window_view(|_window, cx| {
            Editor::from_markdown(cx, SCRIPT.to_string(), Some(path.clone()))
        });
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.save_to_existing_path(&path, window, cx)
            })
        });
        assert_eq!(std::fs::read_to_string(&path).unwrap(), SCRIPT);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[gpui::test]
    async fn editing_one_block_leaves_the_others_as_written(cx: &mut TestAppContext) {
        init(cx);
        let path = temp_file(SCRIPT);
        let (editor, cx) = cx.add_window_view(|_window, cx| {
            Editor::from_markdown(cx, SCRIPT.to_string(), Some(path.clone()))
        });
        editor.update(cx, |editor, cx| {
            let first = editor.document.first_root().unwrap().clone();
            first.update(cx, |block, cx| {
                block.move_to(block.visible_len(), cx);
            });
        });
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                let first = editor.document.first_root().unwrap().clone();
                first.update(cx, |block, cx| {
                    gpui::EntityInputHandler::replace_text_in_range(block, None, "!", window, cx)
                });
                editor.save_to_existing_path(&path, window, cx);
            })
        });
        let saved = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            saved,
            SCRIPT.replacen("# Kommentar eins", "# Kommentar eins!", 1)
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[gpui::test]
    async fn source_mode_shows_the_file_as_written(cx: &mut TestAppContext) {
        init(cx);
        let (editor, cx) =
            cx.add_window_view(|_window, cx| Editor::from_markdown(cx, SCRIPT.to_string(), None));
        let original = SCRIPT.replace("\r\n", "\n");
        editor.update(cx, |editor, cx| {
            editor.toggle_view_mode(cx);
            assert!(editor.view_mode == ViewMode::Source);
            assert_eq!(editor.document.raw_source_text(cx), original);
            // Back to rendered and saved again: still the original.
            editor.toggle_view_mode(cx);
            assert!(editor.view_mode == ViewMode::Rendered);
            assert_eq!(editor.document_text_for_disk(cx), SCRIPT.to_string());
        });
    }
}
