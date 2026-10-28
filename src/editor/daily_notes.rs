//! Daily notes and templates: one note per day in the vault, created from a
//! template on first open, and notes of the templates folder inserted at the
//! caret. Reads Obsidian's `.obsidian/daily-notes.json` (folder, date
//! format, template) so an existing vault keeps its layout.

use std::path::{Path, PathBuf};

use chrono::{Datelike, Local, NaiveDateTime, Timelike};
use gpui::*;

use super::Editor;
use crate::i18n::I18nManager;

const DEFAULT_FORMAT: &str = "YYYY-MM-DD";
const DEFAULT_TIME_FORMAT: &str = "HH:mm";
const DEFAULT_TEMPLATES_FOLDER: &str = "Templates";

/// Where daily notes live and how they start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DailyNotesConfig {
    /// Folder relative to the vault root; empty for the root.
    pub(crate) folder: String,
    /// Moment.js date format of the file name.
    pub(crate) format: String,
    /// Template note relative to the vault root; empty for none.
    pub(crate) template: String,
}

impl Default for DailyNotesConfig {
    fn default() -> Self {
        Self {
            folder: String::new(),
            format: DEFAULT_FORMAT.to_string(),
            template: String::new(),
        }
    }
}

impl DailyNotesConfig {
    /// The vault's Obsidian settings, or the defaults.
    pub(crate) fn load(root: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(root.join(".obsidian").join("daily-notes.json"))
        else {
            return Self::default();
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            return Self::default();
        };
        let field = |name: &str| {
            value
                .get(name)
                .and_then(|value| value.as_str())
                .map(|value| value.trim().trim_matches('/').to_string())
                .unwrap_or_default()
        };
        let format = field("format");
        Self {
            folder: field("folder"),
            format: if format.is_empty() {
                DEFAULT_FORMAT.to_string()
            } else {
                format
            },
            template: field("template"),
        }
    }

    /// The note for `now` below `root`.
    pub(crate) fn note_path(&self, root: &Path, now: NaiveDateTime) -> PathBuf {
        let mut path = root.to_path_buf();
        if !self.folder.is_empty() {
            path.push(&self.folder);
        }
        // `/` in the format nests notes in folders, as in Obsidian.
        for part in format_moment(&self.format, now).split('/') {
            if !part.is_empty() {
                path.push(part);
            }
        }
        path.set_extension("md");
        path
    }

    /// The template file below `root`, if one is set.
    pub(crate) fn template_path(&self, root: &Path) -> Option<PathBuf> {
        if self.template.is_empty() {
            return None;
        }
        let path = root.join(&self.template);
        Some(if path.extension().is_some() {
            path
        } else {
            path.with_extension("md")
        })
    }
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const WEEKDAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

/// A Moment.js token and how to write it.
type Token<'a> = (&'static str, Box<dyn Fn() -> String + 'a>);

/// Formats `time` with the common Moment.js tokens; `[text]` is literal.
pub(crate) fn format_moment(format: &str, time: NaiveDateTime) -> String {
    // Longest tokens first, so `YYYY` wins over `YY`.
    let tokens: [Token; 18] = [
        ("YYYY", Box::new(|| format!("{:04}", time.year()))),
        ("YY", Box::new(|| format!("{:02}", time.year() % 100))),
        (
            "MMMM",
            Box::new(|| MONTHS[time.month0() as usize].to_string()),
        ),
        (
            "MMM",
            Box::new(|| MONTHS[time.month0() as usize][..3].to_string()),
        ),
        ("MM", Box::new(|| format!("{:02}", time.month()))),
        ("M", Box::new(|| time.month().to_string())),
        ("DD", Box::new(|| format!("{:02}", time.day()))),
        ("D", Box::new(|| time.day().to_string())),
        (
            "dddd",
            Box::new(|| WEEKDAYS[time.weekday().num_days_from_monday() as usize].to_string()),
        ),
        (
            "ddd",
            Box::new(|| WEEKDAYS[time.weekday().num_days_from_monday() as usize][..3].to_string()),
        ),
        ("ww", Box::new(|| format!("{:02}", time.iso_week().week()))),
        ("w", Box::new(|| time.iso_week().week().to_string())),
        ("HH", Box::new(|| format!("{:02}", time.hour()))),
        ("H", Box::new(|| time.hour().to_string())),
        ("mm", Box::new(|| format!("{:02}", time.minute()))),
        ("m", Box::new(|| time.minute().to_string())),
        ("ss", Box::new(|| format!("{:02}", time.second()))),
        ("s", Box::new(|| time.second().to_string())),
    ];
    let mut out = String::new();
    let mut rest = format;
    'outer: while let Some(ch) = rest.chars().next() {
        if ch == '[' {
            if let Some(end) = rest.find(']') {
                out.push_str(&rest[1..end]);
                rest = &rest[end + 1..];
                continue;
            }
        }
        for (token, value) in &tokens {
            if let Some(after) = rest.strip_prefix(token) {
                out.push_str(&value());
                rest = after;
                continue 'outer;
            }
        }
        out.push(ch);
        rest = &rest[ch.len_utf8()..];
    }
    out
}

/// Fills Obsidian's template variables: `{{title}}`, `{{date}}`, `{{time}}`
/// and `{{date:FORMAT}}` / `{{time:FORMAT}}`.
pub(crate) fn fill_template(template: &str, title: &str, now: NaiveDateTime) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            out.push_str(&rest[start..]);
            return out;
        };
        let inner = after[..end].trim();
        let (name, format) = inner
            .split_once(':')
            .map(|(name, format)| (name.trim(), Some(format.trim())))
            .unwrap_or((inner, None));
        match name.to_lowercase().as_str() {
            "title" => out.push_str(title),
            "date" => out.push_str(&format_moment(format.unwrap_or(DEFAULT_FORMAT), now)),
            "time" => out.push_str(&format_moment(format.unwrap_or(DEFAULT_TIME_FORMAT), now)),
            _ => out.push_str(&rest[start..start + 2 + end + 2]),
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

/// Creates the note at `path` from `template` unless it exists.
pub(crate) fn ensure_note(
    path: &Path,
    template: Option<&Path>,
    now: NaiveDateTime,
) -> std::io::Result<bool> {
    if path.exists() {
        return Ok(false);
    }
    let title = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let text = template
        .and_then(|template| std::fs::read_to_string(template).ok())
        .map(|template| fill_template(&template, &title, now))
        .unwrap_or_default();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, text)?;
    Ok(true)
}

/// The templates folder of the vault: Obsidian's `.obsidian/templates.json`
/// setting, else `Templates`.
pub(crate) fn templates_folder(root: &Path) -> PathBuf {
    let folder = std::fs::read_to_string(root.join(".obsidian").join("templates.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| {
            value
                .get("folder")
                .and_then(|folder| folder.as_str())
                .map(|folder| folder.trim().trim_matches('/').to_string())
        })
        .filter(|folder| !folder.is_empty())
        .unwrap_or_else(|| DEFAULT_TEMPLATES_FOLDER.to_string());
    root.join(folder)
}

impl Editor {
    pub(super) fn templates_folder(&self) -> Option<PathBuf> {
        self.link_root().map(|root| templates_folder(&root))
    }

    pub(super) fn on_insert_template(
        &mut self,
        _: &crate::components::InsertTemplate,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_palette(
            super::workspace_search::PaletteMode::InsertTemplate,
            window,
            cx,
        );
    }

    /// Inserts the template at `path`, its variables filled, at the caret
    /// of the block edited last.
    pub(super) fn insert_template(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Ok(template) = std::fs::read_to_string(path) else {
            return;
        };
        let title = self
            .file_path
            .as_deref()
            .and_then(|path| path.file_stem())
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let text = fill_template(&template, &title, Local::now().naive_local());
        let Some(block) = self.current_edit_target_from_state(cx) else {
            return;
        };
        window.focus(&block.read(cx).focus_handle.clone(), cx);
        block.update(cx, |block, cx| block.paste_text(text, window, cx));
    }

    /// Opens today's note of the vault, creating it on first use.
    pub(super) fn on_open_daily_note(
        &mut self,
        _: &crate::components::OpenDailyNote,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.link_root() else {
            return;
        };
        let config = DailyNotesConfig::load(&root);
        let now = Local::now().naive_local();
        let path = config.note_path(&root, now);
        match ensure_note(&path, config.template_path(&root).as_deref(), now) {
            Ok(created) => {
                if created {
                    self.refresh_workspace_trees(cx);
                }
                self.open_workspace_file(path, window, cx);
            }
            Err(err) => {
                let strings = cx.global::<I18nManager>().strings().clone();
                let detail = format!("{}: {err}", path.display());
                let buttons = [strings.info_dialog_ok.as_str()];
                let _ = window.prompt(
                    PromptLevel::Warning,
                    &strings.menu_daily_note,
                    Some(&detail),
                    &buttons,
                    cx,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use chrono::NaiveDate;

    use super::{DailyNotesConfig, ensure_note, fill_template, format_moment, templates_folder};

    fn now() -> chrono::NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 3, 5)
            .unwrap()
            .and_hms_opt(9, 7, 0)
            .unwrap()
    }

    #[test]
    fn formats_moment_tokens() {
        assert_eq!(format_moment("YYYY-MM-DD", now()), "2026-03-05");
        assert_eq!(
            format_moment("dddd, D. MMMM YY [Week] ww HH:mm", now()),
            "Thursday, 5. March 26 Week 10 09:07"
        );
        assert_eq!(
            format_moment("YYYY/MM/YYYY-MM-DD", now()),
            "2026/03/2026-03-05"
        );
    }

    #[test]
    fn fills_template_variables() {
        assert_eq!(
            fill_template(
                "# {{title}}\n{{date}} {{time}} {{date:dddd}} {{ time : HH }} {{other}} {{",
                "Note",
                now()
            ),
            "# Note\n2026-03-05 09:07 Thursday 09 {{other}} {{"
        );
    }

    #[test]
    fn reads_obsidian_settings_and_creates_the_note() {
        let dir = std::env::temp_dir().join(format!("velotype-daily-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let root = dir.as_path();
        assert_eq!(DailyNotesConfig::load(root), DailyNotesConfig::default());
        assert_eq!(
            DailyNotesConfig::default().note_path(Path::new("/v"), now()),
            Path::new("/v/2026-03-05.md")
        );

        std::fs::create_dir_all(root.join(".obsidian")).unwrap();
        std::fs::write(
            root.join(".obsidian/daily-notes.json"),
            r#"{"folder":"Journal/","format":"YYYY/MM-DD","template":"Templates/Daily"}"#,
        )
        .unwrap();
        std::fs::create_dir_all(root.join("Templates")).unwrap();
        std::fs::write(root.join("Templates/Daily.md"), "# {{date:D. MMMM}}\n").unwrap();

        let config = DailyNotesConfig::load(root);
        let path = config.note_path(root, now());
        assert_eq!(path, root.join("Journal/2026/03-05.md"));
        let template = config.template_path(root);
        assert!(ensure_note(&path, template.as_deref(), now()).unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "# 5. March\n");
        // An existing note is left alone.
        std::fs::write(&path, "kept").unwrap();
        assert!(!ensure_note(&path, template.as_deref(), now()).unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "kept");

        assert_eq!(templates_folder(root), root.join("Templates"));
        std::fs::write(
            root.join(".obsidian/templates.json"),
            r#"{"folder":"Meta/Vorlagen/"}"#,
        )
        .unwrap();
        assert_eq!(templates_folder(root), root.join("Meta/Vorlagen"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
