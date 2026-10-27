//! Persistent app preferences and the preferences window.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Context as _;
use gpui::prelude::FluentBuilder;
use gpui::*;
use serde::{Deserialize, Serialize};

use super::{VelotypeConfigDirs, read_recent_files};
use crate::components::single_line_input::{InputChanged, SingleLineInput, SingleLineOverflow};
use crate::components::{
    ShortcutCategory, ShortcutCommand, ShortcutDefinition, install_keybindings,
    normalize_shortcut_config, normalize_shortcut_keys, resolved_shortcut_keys,
    shortcut_conflict_for, shortcut_definitions, switch::Switch,
};
use crate::export::{DEFAULT_PDF_MARGIN_MM, PdfExportOptions, PdfOrientation, PdfPaperSize};
use crate::fonts::{FontCatalog, FontPreferences, FontSettings, FontStackParseError};
use crate::i18n::{I18nManager, I18nStrings, language_id_for_locale_preferences};
use crate::theme::{AppearanceThemes, Theme, ThemeCatalogEntry, ThemeManager};
use crate::window_chrome::{
    custom_titlebar_height, render_custom_titlebar, velotype_window_options,
};

/// Built-in dark theme used when a configured theme is absent or unavailable.
const DEFAULT_THEME_ID: &str = "velotype";
/// Language fallback for ordinary preference reads without locale detection.
const DEFAULT_LANGUAGE_ID: &str = "en-US";

/// A user-configurable button shown in the status bar.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StatusBarButton {
    /// Stable identifier for the configured button.
    pub id: String,
    /// Text displayed on the button.
    pub label: String,
    /// Action identifier stored with the button configuration.
    pub action_id: String,
}

/// Status bar visibility and component toggles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StatusBarPreferences {
    /// Whether the status bar is visible.
    pub enabled: bool,
    /// Whether to display the document word count.
    pub show_word_count: bool,
    /// Whether to display the cursor's line and column.
    pub show_cursor_position: bool,
    /// Whether to display the workspace drawer toggle.
    pub show_sidebar_toggle: bool,
    /// Whether to display the source/rendered mode switch.
    pub show_mode_switch: bool,
    /// Additional button definitions retained in the preferences file.
    pub custom_buttons: Vec<StatusBarButton>,
}

impl Default for StatusBarPreferences {
    /// Enables the bar and all built-in items, with no custom buttons.
    fn default() -> Self {
        Self {
            enabled: true,
            show_word_count: true,
            show_cursor_position: true,
            show_sidebar_toggle: true,
            show_mode_switch: true,
            custom_buttons: Vec::new(),
        }
    }
}

/// Startup document selection stored in `config.toml`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StartupOpenPreference {
    /// Start with an empty document.
    NewFile,
    /// Try the first existing recent file, falling back to an empty document.
    LastOpenedFile,
}

impl StartupOpenPreference {
    /// Returns the stable TOML value for this startup mode.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::NewFile => "new_file",
            Self::LastOpenedFile => "last_opened_file",
        }
    }

    /// Parses a persisted value, using a new document for unrecognized values.
    fn from_str(value: &str) -> Self {
        match value {
            "last_opened_file" => Self::LastOpenedFile,
            _ => Self::NewFile,
        }
    }
}

/// Where pasted clipboard images should be stored before inserting Markdown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImagePasteBehavior {
    /// Keep existing image paths; write clipboard image bytes into the paste root.
    None,
    /// Copy into the document directory, or the working directory for unsaved files.
    CopyToDocumentFolder,
    /// Copy into an `assets` subdirectory of the document or working directory.
    CopyToAssetsFolder,
    /// Copy into `<document-stem>.assets`, choosing an unused `untitle` name if unsaved.
    CopyToNamedAssetsFolder,
    /// Keep the image local first, then upload it with the upload command.
    Upload,
}

impl ImagePasteBehavior {
    /// Returns the stable TOML value for this image storage policy.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::CopyToDocumentFolder => "copy_to_document_folder",
            Self::CopyToAssetsFolder => "copy_to_assets_folder",
            Self::CopyToNamedAssetsFolder => "copy_to_named_assets_folder",
            Self::Upload => "upload",
        }
    }

    /// Parses a persisted policy, disabling file copying for unrecognized values.
    fn from_str(value: &str) -> Self {
        match value {
            "copy_to_document_folder" => Self::CopyToDocumentFolder,
            "copy_to_assets_folder" => Self::CopyToAssetsFolder,
            "copy_to_named_assets_folder" => Self::CopyToNamedAssetsFolder,
            "upload" => Self::Upload,
            _ => Self::None,
        }
    }
}

/// Writing aids toggled from the View menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WritingPreferences {
    /// Close brackets/quotes as they are typed and wrap selections.
    pub(crate) auto_pair: bool,
    /// Dim every block except the one being edited.
    pub(crate) focus_mode: bool,
    /// Keep the caret line vertically centered while typing.
    pub(crate) typewriter_mode: bool,
    /// Underline misspelled words using the system dictionaries.
    pub(crate) spellcheck: bool,
    /// BCP-47 dictionary language; empty follows the system locale.
    pub(crate) spellcheck_language: String,
    /// Document zoom from View > Zoom, in percent of the actual size.
    pub(crate) zoom_percent: u16,
    /// End saved files with a line break (Edit > Line Endings).
    pub(crate) final_newline: bool,
    /// Type curly quotes and apostrophes (Edit > Smart Punctuation).
    pub(crate) smart_quotes: bool,
    /// Turn `--` into an en dash and `---` into an em dash.
    pub(crate) smart_dashes: bool,
    /// Save documents that have a file shortly after each change.
    pub(crate) autosave: bool,
    /// Show the formatting toolbar above the document (View > Toolbar).
    pub(crate) toolbar: bool,
}

/// Zoom levels View > Zoom In/Out steps through, in percent.
pub(crate) const ZOOM_STEPS: &[u16] = &[50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200, 250, 300];

/// The next zoom level after `current` in the given direction; stays at the
/// first or last level.
pub(crate) fn step_zoom(current: u16, zoom_in: bool) -> u16 {
    if zoom_in {
        ZOOM_STEPS
            .iter()
            .copied()
            .find(|step| *step > current)
            .unwrap_or(ZOOM_STEPS[ZOOM_STEPS.len() - 1])
    } else {
        ZOOM_STEPS
            .iter()
            .rev()
            .copied()
            .find(|step| *step < current)
            .unwrap_or(ZOOM_STEPS[0])
    }
}

fn clamp_zoom(percent: u16) -> u16 {
    percent.clamp(ZOOM_STEPS[0], ZOOM_STEPS[ZOOM_STEPS.len() - 1])
}

impl Default for WritingPreferences {
    fn default() -> Self {
        Self {
            auto_pair: true,
            focus_mode: false,
            typewriter_mode: false,
            spellcheck: true,
            spellcheck_language: String::new(),
            zoom_percent: 100,
            final_newline: false,
            smart_quotes: false,
            smart_dashes: false,
            autosave: false,
            toolbar: false,
        }
    }
}

/// User preferences persisted under the app config directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AppPreferences {
    /// Document selection policy used at startup.
    pub(crate) startup_open: StartupOpenPreference,
    /// Selected built-in or imported UI language identifier.
    pub(crate) default_language_id: String,
    /// Selected built-in or imported theme identifier.
    pub(crate) default_theme_id: String,
    /// Light/dark theme pair applied when the theme follows the OS appearance.
    pub(crate) appearance_themes: AppearanceThemes,
    /// Whether table top rows receive header styling.
    pub(crate) show_table_headers: bool,
    pub(crate) writing: WritingPreferences,
    /// Storage policy for images inserted from the clipboard.
    pub(crate) image_paste_behavior: ImagePasteBehavior,
    /// Command that uploads images given as arguments and prints their URLs
    /// (Format > Image > Upload Image), e.g. `picgo upload`.
    pub(crate) image_upload_command: String,
    /// Command identifiers mapped to user-supplied shortcut lists.
    pub(crate) keybindings: BTreeMap<String, Vec<String>>,
    /// Status bar visibility, built-in items, and stored custom buttons.
    pub(crate) status_bar: StatusBarPreferences,
    /// Unresolved family lists for body, code, and UI text.
    pub(crate) fonts: FontPreferences,
    /// Page setup for File > Export > PDF.
    pub(crate) pdf_export: PdfExportOptions,
}

impl Default for AppPreferences {
    /// Uses a new document, English, the dark theme, and built-in editor defaults.
    fn default() -> Self {
        Self {
            startup_open: StartupOpenPreference::NewFile,
            default_language_id: DEFAULT_LANGUAGE_ID.into(),
            default_theme_id: DEFAULT_THEME_ID.into(),
            appearance_themes: AppearanceThemes::default(),
            show_table_headers: true,
            writing: WritingPreferences::default(),
            image_paste_behavior: ImagePasteBehavior::None,
            image_upload_command: String::new(),
            keybindings: BTreeMap::new(),
            status_bar: StatusBarPreferences::default(),
            fonts: FontPreferences::default(),
            pdf_export: PdfExportOptions::default(),
        }
    }
}

/// In-memory visibility flags read by the status bar renderer.
struct StatusBarSettings {
    /// Visibility of the entire status bar.
    status_bar_enabled: bool,
    /// Visibility of the word count item.
    status_bar_show_word_count: bool,
    /// Visibility of the cursor position item.
    status_bar_show_cursor_position: bool,
    /// Visibility of the workspace drawer toggle.
    status_bar_show_sidebar_toggle: bool,
    /// Visibility of the source/rendered mode switch.
    status_bar_show_mode_switch: bool,
}

/// Runtime-accessible editor settings mirrored from [`AppPreferences`] so the
/// render path can read them without touching disk. Toggling persists the new
/// value back to the preferences file.
pub struct EditorSettings {
    /// Whether native tables style their top row as a header.
    show_table_headers: bool,
    writing: WritingPreferences,
    /// Status bar flags mirrored from persisted preferences.
    status_bar_settings: StatusBarSettings,
}

impl Global for EditorSettings {}

impl EditorSettings {
    /// Installs table styling and status bar flags, defaulting unreadable bar settings.
    pub fn init(cx: &mut App, show_table_headers: bool) {
        let preferences = read_app_preferences().ok();
        let status_bar = preferences
            .as_ref()
            .map(|p| p.status_bar.clone())
            .unwrap_or_default();
        let writing = preferences.map(|p| p.writing).unwrap_or_default();
        Self::set_global(cx, show_table_headers, writing, &status_bar);
    }

    /// Replaces the runtime flags without reading or writing preferences.
    fn set_global(
        cx: &mut App,
        show_table_headers: bool,
        writing: WritingPreferences,
        status_bar: &StatusBarPreferences,
    ) {
        cx.set_global(Self {
            show_table_headers,
            writing,
            status_bar_settings: StatusBarSettings {
                status_bar_enabled: status_bar.enabled,
                status_bar_show_word_count: status_bar.show_word_count,
                status_bar_show_cursor_position: status_bar.show_cursor_position,
                status_bar_show_sidebar_toggle: status_bar.show_sidebar_toggle,
                status_bar_show_mode_switch: status_bar.show_mode_switch,
            },
        });
    }

    /// Whether table top rows are styled as headers. Defaults to `true` when
    /// the global has not been installed (e.g. in unit tests).
    pub fn show_table_headers(cx: &App) -> bool {
        cx.try_global::<Self>()
            .map(|settings| settings.show_table_headers)
            .unwrap_or(true)
    }

    /// Updates table styling immediately and attempts to persist it, logging failures.
    pub fn set_show_table_headers(cx: &mut App, show_table_headers: bool) {
        let status_bar = cx
            .try_global::<Self>()
            .map(|s| StatusBarPreferences {
                enabled: s.status_bar_settings.status_bar_enabled,
                show_word_count: s.status_bar_settings.status_bar_show_word_count,
                show_cursor_position: s.status_bar_settings.status_bar_show_cursor_position,
                show_sidebar_toggle: s.status_bar_settings.status_bar_show_sidebar_toggle,
                show_mode_switch: s.status_bar_settings.status_bar_show_mode_switch,
                custom_buttons: Vec::new(),
            })
            .unwrap_or_default();
        let writing = Self::writing(cx);
        Self::set_global(cx, show_table_headers, writing, &status_bar);
        match read_app_preferences() {
            Ok(mut preferences) => {
                preferences.show_table_headers = show_table_headers;
                if let Err(err) = save_app_preferences(&preferences) {
                    eprintln!("failed to save table header preference: {err}");
                }
            }
            Err(err) => eprintln!("failed to read table header preference: {err}"),
        }
    }

    /// Current writing aids; defaults apply when the global is not installed
    /// (e.g. in unit tests).
    pub(crate) fn writing(cx: &App) -> WritingPreferences {
        cx.try_global::<Self>()
            .map(|settings| settings.writing.clone())
            .unwrap_or_default()
    }

    /// Installs writing aids without touching the preferences file.
    #[cfg(test)]
    pub(crate) fn set_writing_for_test(cx: &mut App, writing: WritingPreferences) {
        Self::set_global(cx, true, writing, &StatusBarPreferences::default());
    }

    pub(crate) fn auto_pair(cx: &App) -> bool {
        Self::writing(cx).auto_pair
    }

    pub(crate) fn focus_mode(cx: &App) -> bool {
        Self::writing(cx).focus_mode
    }

    pub(crate) fn typewriter_mode(cx: &App) -> bool {
        Self::writing(cx).typewriter_mode
    }

    /// Applies `update` to the writing aids and persists the result.
    pub(crate) fn update_writing(cx: &mut App, update: impl FnOnce(&mut WritingPreferences)) {
        let mut writing = Self::writing(cx);
        update(&mut writing);
        let show_table_headers = Self::show_table_headers(cx);
        let status_bar = Self::status_bar_preferences(cx);
        Self::set_global(cx, show_table_headers, writing.clone(), &status_bar);
        match read_app_preferences() {
            Ok(mut preferences) => {
                preferences.writing = writing;
                if let Err(err) = save_app_preferences(&preferences) {
                    eprintln!("failed to save writing preferences: {err}");
                }
            }
            Err(err) => eprintln!("failed to read writing preferences: {err}"),
        }
    }

    /// Shows or hides the status bar (View > Status Bar) and persists it.
    pub(crate) fn set_status_bar_enabled(cx: &mut App, enabled: bool) {
        let mut status_bar = Self::status_bar_preferences(cx);
        status_bar.enabled = enabled;
        let show_table_headers = Self::show_table_headers(cx);
        let writing = Self::writing(cx);
        Self::set_global(cx, show_table_headers, writing, &status_bar);
        match read_app_preferences() {
            Ok(mut preferences) => {
                preferences.status_bar.enabled = enabled;
                if let Err(err) = save_app_preferences(&preferences) {
                    eprintln!("failed to save status bar preference: {err}");
                }
            }
            Err(err) => eprintln!("failed to read status bar preference: {err}"),
        }
    }

    /// Returns runtime bar flags or defaults; custom buttons are not stored globally.
    pub fn status_bar_preferences(cx: &App) -> StatusBarPreferences {
        cx.try_global::<Self>()
            .map(|s| StatusBarPreferences {
                enabled: s.status_bar_settings.status_bar_enabled,
                show_word_count: s.status_bar_settings.status_bar_show_word_count,
                show_cursor_position: s.status_bar_settings.status_bar_show_cursor_position,
                show_sidebar_toggle: s.status_bar_settings.status_bar_show_sidebar_toggle,
                show_mode_switch: s.status_bar_settings.status_bar_show_mode_switch,
                custom_buttons: Vec::new(),
            })
            .unwrap_or_default()
    }
}

/// TOML output shape for normalized app preferences.
#[derive(Serialize)]
struct PreferencesFile {
    /// Startup document selection table.
    startup: StartupPreferencesFile,
    /// Default UI language table.
    language: LanguagePreferencesFile,
    /// Default theme table.
    theme: ThemePreferencesFile,
    /// Editor behavior table.
    editor: EditorPreferencesFile,
    /// Status bar configuration table.
    status_bar: StatusBarPreferencesFile,
    /// Normalized user shortcut overrides.
    keybindings: BTreeMap<String, Vec<String>>,
    /// Font family lists table.
    fonts: FontPreferencesFile,
    pdf_export: PdfExportPreferencesFile,
}

#[derive(Serialize)]
struct PdfExportPreferencesFile {
    paper_size: String,
    orientation: String,
    margin_mm: u32,
    page_numbers: bool,
}

impl From<&PdfExportOptions> for PdfExportPreferencesFile {
    fn from(value: &PdfExportOptions) -> Self {
        Self {
            paper_size: value.paper_size.as_str().into(),
            orientation: value.orientation.as_str().into(),
            margin_mm: value.margin_mm,
            page_numbers: value.page_numbers,
        }
    }
}

/// Persisted font family lists, kept as user-editable strings.
#[derive(Serialize)]
struct FontPreferencesFile {
    /// Body text family list.
    body_stack: String,
    /// Code text family list.
    code_stack: String,
    /// Application UI family list.
    ui_stack: String,
}

impl From<&FontPreferences> for FontPreferencesFile {
    /// Copies unresolved font preferences into their TOML representation.
    fn from(value: &FontPreferences) -> Self {
        Self {
            body_stack: value.body_stack.clone(),
            code_stack: value.code_stack.clone(),
            ui_stack: value.ui_stack.clone(),
        }
    }
}

/// Persisted startup document policy.
#[derive(Serialize)]
struct StartupPreferencesFile {
    /// Serialized startup mode identifier.
    open: String,
}

/// Persisted editor rendering and paste settings.
#[derive(Serialize)]
struct EditorPreferencesFile {
    /// Whether to style the first table row as a header.
    show_table_headers: bool,
    /// Serialized clipboard image storage policy.
    image_paste_behavior: String,
    image_upload_command: String,
    auto_pair: bool,
    focus_mode: bool,
    typewriter_mode: bool,
    spellcheck: bool,
    spellcheck_language: String,
    zoom_percent: u16,
    final_newline: bool,
    smart_quotes: bool,
    smart_dashes: bool,
    autosave: bool,
    toolbar: bool,
}

/// Persisted default UI language selection.
#[derive(Serialize)]
struct LanguagePreferencesFile {
    /// Built-in or imported language identifier.
    default_language_id: String,
}

/// Persisted default theme selection.
#[derive(Serialize)]
struct ThemePreferencesFile {
    /// Built-in or imported theme identifier.
    default_theme_id: String,
    follow_system_appearance: bool,
    light_theme_id: String,
    dark_theme_id: String,
}

/// TOML status bar settings, omitting an empty custom button list.
#[derive(Serialize)]
struct StatusBarPreferencesFile {
    /// Visibility of the entire bar.
    enabled: bool,
    /// Visibility of the word count.
    show_word_count: bool,
    /// Visibility of the cursor position.
    show_cursor_position: bool,
    /// Visibility of the workspace drawer toggle.
    show_sidebar_toggle: bool,
    /// Visibility of the source/rendered mode switch.
    show_mode_switch: bool,
    /// Stored custom button definitions.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    custom_buttons: Vec<StatusBarButton>,
}

impl From<&StatusBarPreferences> for StatusBarPreferencesFile {
    /// Copies visibility flags and custom buttons into the TOML output shape.
    fn from(value: &StatusBarPreferences) -> Self {
        Self {
            enabled: value.enabled,
            show_word_count: value.show_word_count,
            show_cursor_position: value.show_cursor_position,
            show_sidebar_toggle: value.show_sidebar_toggle,
            show_mode_switch: value.show_mode_switch,
            custom_buttons: value.custom_buttons.clone(),
        }
    }
}

impl From<&AppPreferences> for PreferencesFile {
    /// Converts enum values and normalizes shortcuts for TOML serialization.
    fn from(value: &AppPreferences) -> Self {
        Self {
            startup: StartupPreferencesFile {
                open: value.startup_open.as_str().into(),
            },
            language: LanguagePreferencesFile {
                default_language_id: value.default_language_id.clone(),
            },
            theme: ThemePreferencesFile {
                default_theme_id: value.default_theme_id.clone(),
                follow_system_appearance: value.appearance_themes.follow_system,
                light_theme_id: value.appearance_themes.light_theme_id.clone(),
                dark_theme_id: value.appearance_themes.dark_theme_id.clone(),
            },
            editor: EditorPreferencesFile {
                show_table_headers: value.show_table_headers,
                image_paste_behavior: value.image_paste_behavior.as_str().into(),
                image_upload_command: value.image_upload_command.clone(),
                auto_pair: value.writing.auto_pair,
                focus_mode: value.writing.focus_mode,
                typewriter_mode: value.writing.typewriter_mode,
                spellcheck: value.writing.spellcheck,
                spellcheck_language: value.writing.spellcheck_language.clone(),
                zoom_percent: value.writing.zoom_percent,
                final_newline: value.writing.final_newline,
                smart_quotes: value.writing.smart_quotes,
                smart_dashes: value.writing.smart_dashes,
                autosave: value.writing.autosave,
                toolbar: value.writing.toolbar,
            },
            status_bar: StatusBarPreferencesFile::from(&value.status_bar),
            keybindings: normalize_shortcut_config(&value.keybindings),
            fonts: FontPreferencesFile::from(&value.fonts),
            pdf_export: PdfExportPreferencesFile::from(&value.pdf_export),
        }
    }
}

/// Reads system preferences without writing, using English for a missing language.
pub(crate) fn read_app_preferences() -> anyhow::Result<AppPreferences> {
    read_app_preferences_with_dirs(&VelotypeConfigDirs::from_system()?)
}

/// Reads preferences from the supplied root; missing or malformed TOML uses defaults.
pub(crate) fn read_app_preferences_with_dirs(
    dirs: &VelotypeConfigDirs,
) -> anyhow::Result<AppPreferences> {
    let path = dirs.app_config_file();
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(AppPreferences::default());
        }
        Err(err) => {
            return Err(err).with_context(|| format!("failed to read '{}'", path.display()));
        }
    };
    let Ok(value) = toml::from_str::<toml::Value>(&text) else {
        return Ok(AppPreferences::default());
    };

    Ok(app_preferences_from_toml_value(&value, DEFAULT_LANGUAGE_ID))
}

/// Loads and rewrites normalized preferences, detecting an unset language from locales.
pub(crate) fn load_or_create_app_preferences() -> anyhow::Result<AppPreferences> {
    let dirs = VelotypeConfigDirs::from_system()?;
    load_or_create_app_preferences_with_dirs_and_locales(&dirs, sys_locale::get_locales())
}

/// Extracts known settings with per-field defaults and independent font validation.
fn app_preferences_from_toml_value(
    value: &toml::Value,
    fallback_language_id: &str,
) -> AppPreferences {
    let startup_open = value
        .get("startup")
        .and_then(|startup| startup.get("open"))
        .and_then(|open| open.as_str())
        .map(StartupOpenPreference::from_str)
        .unwrap_or(StartupOpenPreference::NewFile);
    let default_language_id = value
        .get("language")
        .and_then(|language| language.get("default_language_id"))
        .and_then(|id| id.as_str())
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .unwrap_or(fallback_language_id)
        .to_string();
    let default_theme_id = value
        .get("theme")
        .and_then(|theme| theme.get("default_theme_id"))
        .and_then(|id| id.as_str())
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .unwrap_or(DEFAULT_THEME_ID)
        .to_string();
    let appearance_themes = appearance_themes_from_toml_value(value);
    let keybindings = value
        .get("keybindings")
        .and_then(|keybindings| keybindings.as_table())
        .map(|table| {
            table
                .iter()
                .filter_map(|(key, value)| {
                    let keys = value
                        .as_array()?
                        .iter()
                        .filter_map(|value| value.as_str().map(str::to_string))
                        .collect::<Vec<_>>();
                    Some((key.clone(), keys))
                })
                .collect::<BTreeMap<_, _>>()
        })
        .map(|keybindings| normalize_shortcut_config(&keybindings))
        .unwrap_or_default();

    let show_table_headers = value
        .get("editor")
        .and_then(|editor| editor.get("show_table_headers"))
        .and_then(|value| value.as_bool())
        .unwrap_or(true);
    let image_paste_behavior = value
        .get("editor")
        .and_then(|editor| editor.get("image_paste_behavior"))
        .and_then(|value| value.as_str())
        .map(ImagePasteBehavior::from_str)
        .unwrap_or(ImagePasteBehavior::None);
    let default_writing = WritingPreferences::default();
    let editor_flag = |key: &str, fallback: bool| {
        value
            .get("editor")
            .and_then(|editor| editor.get(key))
            .and_then(|value| value.as_bool())
            .unwrap_or(fallback)
    };
    let writing = WritingPreferences {
        auto_pair: editor_flag("auto_pair", default_writing.auto_pair),
        focus_mode: editor_flag("focus_mode", default_writing.focus_mode),
        typewriter_mode: editor_flag("typewriter_mode", default_writing.typewriter_mode),
        spellcheck: editor_flag("spellcheck", default_writing.spellcheck),
        spellcheck_language: value
            .get("editor")
            .and_then(|editor| editor.get("spellcheck_language"))
            .and_then(|value| value.as_str())
            .map(|language| language.trim().to_string())
            .unwrap_or_default(),
        zoom_percent: value
            .get("editor")
            .and_then(|editor| editor.get("zoom_percent"))
            .and_then(|value| value.as_integer())
            .and_then(|percent| u16::try_from(percent).ok())
            .map(clamp_zoom)
            .unwrap_or(default_writing.zoom_percent),
        final_newline: editor_flag("final_newline", default_writing.final_newline),
        smart_quotes: editor_flag("smart_quotes", default_writing.smart_quotes),
        smart_dashes: editor_flag("smart_dashes", default_writing.smart_dashes),
        autosave: editor_flag("autosave", default_writing.autosave),
        toolbar: editor_flag("toolbar", default_writing.toolbar),
    };

    let status_bar = value
        .get("status_bar")
        .map(|sb| {
            let enabled = sb.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
            let show_word_count = sb
                .get("show_word_count")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let show_cursor_position = sb
                .get("show_cursor_position")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let show_sidebar_toggle = sb
                .get("show_sidebar_toggle")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let show_mode_switch = sb
                .get("show_mode_switch")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let custom_buttons = sb
                .get("custom_buttons")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|item| {
                            let id = item.get("id")?.as_str()?.to_string();
                            let label = item.get("label")?.as_str()?.to_string();
                            Some(StatusBarButton {
                                id,
                                label,
                                action_id: item
                                    .get("action_id")
                                    .and_then(|a| a.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            StatusBarPreferences {
                enabled,
                show_word_count,
                show_cursor_position,
                show_sidebar_toggle,
                show_mode_switch,
                custom_buttons,
            }
        })
        .unwrap_or_default();

    let default_fonts = FontPreferences::default();
    let font_value = |key: &str, fallback: &str| {
        let candidate = value
            .get("fonts")
            .and_then(|fonts| fonts.get(key))
            .and_then(|value| value.as_str())
            .unwrap_or(fallback);
        if crate::fonts::parse_font_stack(candidate).is_ok() {
            candidate.to_string()
        } else {
            fallback.to_string()
        }
    };
    let fonts = FontPreferences {
        body_stack: font_value("body_stack", &default_fonts.body_stack),
        code_stack: font_value("code_stack", &default_fonts.code_stack),
        ui_stack: font_value("ui_stack", &default_fonts.ui_stack),
    };

    let image_upload_command = value
        .get("editor")
        .and_then(|editor| editor.get("image_upload_command"))
        .and_then(|value| value.as_str())
        .map(|command| command.trim().to_string())
        .unwrap_or_default();

    AppPreferences {
        startup_open,
        default_language_id,
        default_theme_id,
        appearance_themes,
        show_table_headers,
        writing,
        image_paste_behavior,
        image_upload_command,
        keybindings,
        status_bar,
        fonts,
        pdf_export: PdfExportOptions::from_toml(value.get("pdf_export")),
    }
}

fn appearance_themes_from_toml_value(value: &toml::Value) -> AppearanceThemes {
    let defaults = AppearanceThemes::default();
    let theme = value.get("theme");
    let theme_id = |key: &str, fallback: String| {
        theme
            .and_then(|theme| theme.get(key))
            .and_then(|id| id.as_str())
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_string)
            .unwrap_or(fallback)
    };
    AppearanceThemes {
        follow_system: theme
            .and_then(|theme| theme.get("follow_system_appearance"))
            .and_then(|value| value.as_bool())
            .unwrap_or(defaults.follow_system),
        light_theme_id: theme_id("light_theme_id", defaults.light_theme_id),
        dark_theme_id: theme_id("dark_theme_id", defaults.dark_theme_id),
    }
}

/// Maps preferred system locales to a supported built-in language.
fn detected_language_id_from_locales<I, S>(locales: I) -> &'static str
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    language_id_for_locale_preferences(locales)
}

/// Loads or repairs preferences using locale fallback, then saves the normalized result.
fn load_or_create_app_preferences_with_dirs_and_locales<I, S>(
    dirs: &VelotypeConfigDirs,
    locales: I,
) -> anyhow::Result<AppPreferences>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let detected_language_id = detected_language_id_from_locales(locales);
    let path = dirs.app_config_file();
    let preferences = match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str::<toml::Value>(&text)
            .map(|value| app_preferences_from_toml_value(&value, detected_language_id))
            .unwrap_or_else(|_| AppPreferences {
                default_language_id: detected_language_id.into(),
                ..AppPreferences::default()
            }),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => AppPreferences {
            default_language_id: detected_language_id.into(),
            ..AppPreferences::default()
        },
        Err(err) => {
            return Err(err).with_context(|| format!("failed to read '{}'", path.display()));
        }
    };
    save_app_preferences_with_dirs(&preferences, dirs)?;
    Ok(preferences)
}

/// Writes normalized preferences to the platform configuration directory.
pub(crate) fn save_app_preferences(preferences: &AppPreferences) -> anyhow::Result<()> {
    save_app_preferences_with_dirs(preferences, &VelotypeConfigDirs::from_system()?)
}

/// Creates the config directory as needed and writes pretty-printed TOML preferences.
pub(crate) fn save_app_preferences_with_dirs(
    preferences: &AppPreferences,
    dirs: &VelotypeConfigDirs,
) -> anyhow::Result<()> {
    let path = dirs.app_config_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create '{}'", parent.display()))?;
    }
    let text = toml::to_string_pretty(&PreferencesFile::from(preferences))?;
    std::fs::write(&path, text).with_context(|| format!("failed to write '{}'", path.display()))
}

/// Returns the first history path that is a file, without checking its extension.
/// Unreadable history yields no startup file.
pub(crate) fn first_existing_recent_markdown_file() -> Option<PathBuf> {
    let recent_files = read_recent_files().ok()?;
    recent_files.into_iter().find(|path| path.is_file())
}

/// Activates and persists a recognized language, returning whether its ID changed.
/// Unknown IDs return `false`; persistence errors occur after runtime selection.
pub(crate) fn apply_configured_language(cx: &mut App, language_id: &str) -> anyhow::Result<bool> {
    let mut applied = false;
    let changed = cx.update_global::<I18nManager, _>(|i18n_manager, _cx| {
        let changed = i18n_manager.set_language_by_id(language_id);
        applied = changed || i18n_manager.current_language_id() == language_id;
        changed
    });
    if !applied {
        return Ok(false);
    }
    update_app_preferences(|preferences| {
        preferences.default_language_id = language_id.into();
    })?;
    Ok(changed)
}

/// Activates and persists a theme, returning `true` for any recognized ID.
/// Persistence errors occur after runtime selection; unknown IDs return `false`.
pub(crate) fn apply_configured_theme(cx: &mut App, theme_id: &str) -> anyhow::Result<bool> {
    let mut applied = false;
    let dark = system_appearance_is_dark(cx.window_appearance());
    let mut appearance_themes = None;
    let changed = cx.update_global::<ThemeManager, _>(|theme_manager, _cx| {
        let changed = theme_manager.set_theme_by_id(theme_id);
        applied = changed || theme_manager.current_theme_id() == theme_id;
        if applied && theme_manager.follows_system_appearance() {
            // While following the system, picking a theme assigns it to the
            // slot of the appearance that is currently active.
            let mut appearance = theme_manager.appearance_themes().clone();
            if dark {
                appearance.dark_theme_id = theme_id.into();
            } else {
                appearance.light_theme_id = theme_id.into();
            }
            theme_manager.set_appearance_themes(appearance.clone());
            appearance_themes = Some(appearance);
        }
        changed
    });
    if !applied {
        return Ok(false);
    }
    update_app_preferences(|preferences| {
        preferences.default_theme_id = theme_id.into();
        if let Some(appearance_themes) = appearance_themes {
            preferences.appearance_themes = appearance_themes;
        }
    })?;
    Ok(changed)
}

/// True for the dark variants of the OS window appearance.
pub(crate) fn system_appearance_is_dark(appearance: WindowAppearance) -> bool {
    matches!(
        appearance,
        WindowAppearance::Dark | WindowAppearance::VibrantDark
    )
}

/// Installs the persisted light/dark theme pair and, when following is
/// enabled, activates the theme matching the current OS appearance.
pub(crate) fn init_appearance_themes(cx: &mut App, appearance_themes: AppearanceThemes) {
    let dark = system_appearance_is_dark(cx.window_appearance());
    cx.update_global::<ThemeManager, _>(|theme_manager, _cx| {
        theme_manager.set_appearance_themes(appearance_themes);
        theme_manager.apply_system_appearance(dark);
    });
}

/// Re-applies the appearance-bound theme after the OS appearance changed.
/// Returns true when the active theme changed and windows need a refresh.
pub(crate) fn sync_theme_with_system_appearance(
    cx: &mut App,
    appearance: WindowAppearance,
) -> bool {
    let dark = system_appearance_is_dark(appearance);
    let changed = cx.update_global::<ThemeManager, _>(|theme_manager, _cx| {
        theme_manager.apply_system_appearance(dark)
    });
    if changed {
        crate::app_menu::install_menus(cx);
        cx.refresh_windows();
    }
    changed
}

/// Toggles "follow system appearance". Turning it on seeds the slot of the
/// current appearance with the active theme, so nothing changes visually
/// until the OS appearance flips.
pub(crate) fn toggle_follow_system_appearance(cx: &mut App) -> anyhow::Result<()> {
    let dark = system_appearance_is_dark(cx.window_appearance());
    let appearance_themes = cx.update_global::<ThemeManager, _>(|theme_manager, _cx| {
        let mut appearance = theme_manager.appearance_themes().clone();
        appearance.follow_system = !appearance.follow_system;
        if appearance.follow_system {
            let current = theme_manager.current_theme_id().to_string();
            if dark {
                appearance.dark_theme_id = current;
            } else {
                appearance.light_theme_id = current;
            }
        }
        theme_manager.set_appearance_themes(appearance);
        theme_manager.apply_system_appearance(dark);
        theme_manager.appearance_themes().clone()
    });
    update_app_preferences(|preferences| {
        preferences.appearance_themes = appearance_themes;
    })?;
    Ok(())
}

/// Imports and activates a language pack, then persists its ID as the default.
pub(crate) fn import_language_config_and_select(
    cx: &mut App,
    path: impl AsRef<std::path::Path>,
) -> anyhow::Result<String> {
    let imported_id = cx.update_global::<I18nManager, _>(|i18n_manager, _cx| {
        i18n_manager.import_language_config(path)
    })?;
    update_app_preferences(|preferences| {
        preferences.default_language_id = imported_id.clone();
    })?;
    Ok(imported_id)
}

/// Imports and activates a theme pack, then persists its ID as the default.
pub(crate) fn import_theme_config_and_select(
    cx: &mut App,
    path: impl AsRef<std::path::Path>,
) -> anyhow::Result<String> {
    let imported_id = cx.update_global::<ThemeManager, _>(|theme_manager, _cx| {
        theme_manager.import_theme_config(path)
    })?;
    update_app_preferences(|preferences| {
        preferences.default_theme_id = imported_id.clone();
    })?;
    Ok(imported_id)
}

/// Persists editable window settings while retaining language and table-header settings.
pub(crate) fn save_preferences_from_window(
    startup_open: StartupOpenPreference,
    default_theme_id: &str,
    image_paste_behavior: ImagePasteBehavior,
    keybindings: BTreeMap<String, Vec<String>>,
    status_bar: &StatusBarPreferences,
    fonts: &FontPreferences,
) -> anyhow::Result<AppPreferences> {
    let dirs = VelotypeConfigDirs::from_system()?;
    save_preferences_from_window_with_dirs(
        startup_open,
        default_theme_id,
        image_paste_behavior,
        keybindings,
        status_bar,
        fonts,
        &dirs,
    )
}

/// Updates window-owned fields in the supplied config root and returns the saved values.
fn save_preferences_from_window_with_dirs(
    startup_open: StartupOpenPreference,
    default_theme_id: &str,
    image_paste_behavior: ImagePasteBehavior,
    keybindings: BTreeMap<String, Vec<String>>,
    status_bar: &StatusBarPreferences,
    fonts: &FontPreferences,
    dirs: &VelotypeConfigDirs,
) -> anyhow::Result<AppPreferences> {
    let mut preferences =
        load_or_create_app_preferences_with_dirs_and_locales(dirs, sys_locale::get_locales())?;
    preferences.startup_open = startup_open;
    preferences.default_theme_id = default_theme_id.into();
    preferences.image_paste_behavior = image_paste_behavior;
    preferences.keybindings = normalize_shortcut_config(&keybindings);
    preferences.status_bar = status_bar.clone();
    preferences.fonts = fonts.clone();
    save_app_preferences_with_dirs(&preferences, dirs)?;
    Ok(preferences)
}

/// Loads normalized preferences, applies one mutation, and saves the result.
fn update_app_preferences(
    update: impl FnOnce(&mut AppPreferences),
) -> anyhow::Result<AppPreferences> {
    let mut preferences = load_or_create_app_preferences()?;
    update(&mut preferences);
    save_app_preferences(&preferences)?;
    Ok(preferences)
}

/// Preferences page currently displayed in the navigation pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PreferencesNav {
    /// Startup document selection.
    File,
    /// Theme selection and font family lists.
    Theme,
    /// Clipboard image storage policy.
    Image,
    /// Keyboard shortcut overrides.
    Shortcuts,
    /// Status bar visibility controls.
    StatusBar,
    Export,
}

/// Checks body, code, and UI availability; syntax errors are handled separately.
fn font_uses_system_default(fonts: &FontPreferences, catalog: &FontCatalog) -> [bool; 3] {
    [
        FontSettings::stack_uses_system_default(&fonts.body_stack, catalog, std::env::consts::OS)
            .unwrap_or(false),
        FontSettings::stack_uses_system_default(&fonts.code_stack, catalog, std::env::consts::OS)
            .unwrap_or(false),
        FontSettings::stack_uses_system_default(&fonts.ui_stack, catalog, std::env::consts::OS)
            .unwrap_or(false),
    ]
}

/// Independent preferences window view.
pub(crate) struct PreferencesWindow {
    /// Active page in the navigation pane.
    nav: PreferencesNav,
    /// Draft startup document policy.
    startup_open: StartupOpenPreference,
    /// Draft theme selection.
    selected_theme_id: String,
    /// Draft clipboard image storage policy.
    image_paste_behavior: ImagePasteBehavior,
    /// Draft shortcut overrides.
    keybindings: BTreeMap<String, Vec<String>>,
    /// Startup policy at the last successful save.
    saved_startup_open: StartupOpenPreference,
    /// Theme ID at the last successful save.
    saved_theme_id: String,
    /// Image policy at the last successful save.
    saved_image_paste_behavior: ImagePasteBehavior,
    /// Shortcut overrides used as the dirty-state baseline.
    saved_keybindings: BTreeMap<String, Vec<String>>,
    /// Theme catalog snapshot captured when this window opens.
    theme_options: Vec<ThemeCatalogEntry>,
    /// Window-level focus target and shortcut recording receiver.
    focus_handle: FocusHandle,
    /// Whether startup policy options are expanded.
    startup_dropdown_open: bool,
    /// Whether theme options are expanded.
    theme_dropdown_open: bool,
    /// Whether image storage options are expanded.
    image_dropdown_open: bool,
    /// Command awaiting a replacement keystroke.
    recording_shortcut: Option<ShortcutCommand>,
    /// Localized invalid-key or conflict message for shortcut recording.
    shortcut_error: Option<String>,
    /// Draft visibility of the status bar.
    status_bar_enabled: bool,
    /// Draft visibility of word count.
    status_bar_show_word_count: bool,
    /// Draft visibility of cursor position.
    status_bar_show_cursor_position: bool,
    /// Draft visibility of the workspace toggle.
    status_bar_show_sidebar_toggle: bool,
    /// Draft visibility of the view-mode switch.
    status_bar_show_mode_switch: bool,
    /// Saved bar visibility for dirty-state comparison.
    saved_status_bar_enabled: bool,
    /// Saved word-count visibility for dirty-state comparison.
    saved_status_bar_show_word_count: bool,
    /// Saved cursor-position visibility for dirty-state comparison.
    saved_status_bar_show_cursor_position: bool,
    /// Saved workspace-toggle visibility for dirty-state comparison.
    saved_status_bar_show_sidebar_toggle: bool,
    /// Saved mode-switch visibility for dirty-state comparison.
    saved_status_bar_show_mode_switch: bool,
    /// Draft family lists synchronized from the text inputs.
    fonts: FontPreferences,
    /// Font family lists at the last successful save.
    saved_fonts: FontPreferences,
    /// Available family snapshot used to validate local availability.
    font_catalog: FontCatalog,
    /// Flags for stacks with no available requested family, in body, code, UI order.
    font_uses_system_default: [bool; 3],
    /// Editable body font family list.
    body_font_input: Entity<SingleLineInput>,
    /// Editable code font family list.
    code_font_input: Entity<SingleLineInput>,
    /// Editable application UI font family list.
    ui_font_input: Entity<SingleLineInput>,
    pdf_export: PdfExportOptions,
    saved_pdf_export: PdfExportOptions,
    upload_command: String,
    saved_upload_command: String,
    upload_command_input: Entity<SingleLineInput>,
}

impl PreferencesWindow {
    /// Builds the draft, saved baselines, and subscribed font inputs from preferences.
    fn new(
        preferences: AppPreferences,
        theme_options: Vec<ThemeCatalogEntry>,
        cx: &mut Context<Self>,
    ) -> Self {
        let selected_theme_id = if theme_options
            .iter()
            .any(|entry| entry.id == preferences.default_theme_id)
        {
            preferences.default_theme_id
        } else {
            DEFAULT_THEME_ID.into()
        };
        let startup_open = preferences.startup_open;
        let image_paste_behavior = preferences.image_paste_behavior;
        let keybindings = preferences.keybindings;
        let fonts = preferences.fonts.clone();
        let font_catalog = FontCatalog::current(cx);
        let font_uses_system_default = font_uses_system_default(&fonts, &font_catalog);
        let body_font_input = cx.new(|cx| {
            SingleLineInput::new(fonts.body_stack.clone(), "e.g. .SystemUIFont", cx)
                .with_overflow(SingleLineOverflow::Ellipsis)
        });
        let code_font_input = cx.new(|cx| {
            SingleLineInput::new(fonts.code_stack.clone(), "e.g. Consolas, monospace", cx)
                .with_overflow(SingleLineOverflow::Ellipsis)
        });
        let ui_font_input = cx.new(|cx| {
            SingleLineInput::new(fonts.ui_stack.clone(), "e.g. .SystemUIFont", cx)
                .with_overflow(SingleLineOverflow::Ellipsis)
        });
        cx.subscribe(&body_font_input, |this, input, _: &InputChanged, cx| {
            this.fonts.body_stack = input.read(cx).value().to_string();
            this.refresh_font_availability(0);
            cx.notify();
        })
        .detach();
        cx.subscribe(&code_font_input, |this, input, _: &InputChanged, cx| {
            this.fonts.code_stack = input.read(cx).value().to_string();
            this.refresh_font_availability(1);
            cx.notify();
        })
        .detach();
        let upload_command = preferences.image_upload_command.clone();
        let upload_command_input = cx.new(|cx| {
            SingleLineInput::new(upload_command.clone(), "e.g. picgo upload", cx)
                .with_overflow(SingleLineOverflow::Ellipsis)
        });
        cx.subscribe(
            &upload_command_input,
            |this, input, _: &InputChanged, cx| {
                this.upload_command = input.read(cx).value().to_string();
                cx.notify();
            },
        )
        .detach();
        cx.subscribe(&ui_font_input, |this, input, _: &InputChanged, cx| {
            this.fonts.ui_stack = input.read(cx).value().to_string();
            this.refresh_font_availability(2);
            cx.notify();
        })
        .detach();
        Self {
            nav: PreferencesNav::File,
            startup_open,
            selected_theme_id: selected_theme_id.clone(),
            image_paste_behavior,
            keybindings: keybindings.clone(),
            saved_startup_open: startup_open,
            saved_theme_id: selected_theme_id,
            saved_image_paste_behavior: image_paste_behavior,
            saved_keybindings: keybindings,
            theme_options,
            focus_handle: cx.focus_handle(),
            startup_dropdown_open: false,
            theme_dropdown_open: false,
            image_dropdown_open: false,
            recording_shortcut: None,
            shortcut_error: None,
            status_bar_enabled: preferences.status_bar.enabled,
            status_bar_show_word_count: preferences.status_bar.show_word_count,
            status_bar_show_cursor_position: preferences.status_bar.show_cursor_position,
            status_bar_show_sidebar_toggle: preferences.status_bar.show_sidebar_toggle,
            status_bar_show_mode_switch: preferences.status_bar.show_mode_switch,
            saved_status_bar_enabled: preferences.status_bar.enabled,
            saved_status_bar_show_word_count: preferences.status_bar.show_word_count,
            saved_status_bar_show_cursor_position: preferences.status_bar.show_cursor_position,
            saved_status_bar_show_sidebar_toggle: preferences.status_bar.show_sidebar_toggle,
            saved_status_bar_show_mode_switch: preferences.status_bar.show_mode_switch,
            fonts: fonts.clone(),
            saved_fonts: fonts,
            font_catalog,
            font_uses_system_default,
            body_font_input,
            code_font_input,
            ui_font_input,
            pdf_export: preferences.pdf_export.clone(),
            saved_pdf_export: preferences.pdf_export,
            saved_upload_command: upload_command.clone(),
            upload_command,
            upload_command_input,
        }
    }

    /// Returns the first localized font syntax error in body, code, UI order.
    fn font_validation_error(&self, strings: &I18nStrings) -> Option<String> {
        [
            (&strings.preferences_font_body, &self.fonts.body_stack),
            (&strings.preferences_font_code, &self.fonts.code_stack),
            (&strings.preferences_font_ui, &self.fonts.ui_stack),
        ]
        .into_iter()
        .find_map(|(label, value)| {
            crate::fonts::parse_font_stack(value).err().map(|error| {
                let message = match error {
                    FontStackParseError::Empty => &strings.preferences_font_error_empty,
                    FontStackParseError::UnterminatedQuote => {
                        &strings.preferences_font_error_unterminated_quote
                    }
                };
                format!("{label}: {message}")
            })
        })
    }

    /// Rechecks one indexed font stack; invalid syntax does not show an availability warning.
    fn refresh_font_availability(&mut self, index: usize) {
        let input = match index {
            0 => &self.fonts.body_stack,
            1 => &self.fonts.code_stack,
            2 => &self.fonts.ui_stack,
            _ => return,
        };
        self.font_uses_system_default[index] = FontSettings::stack_uses_system_default(
            input,
            &self.font_catalog,
            std::env::consts::OS,
        )
        .unwrap_or(false);
    }

    /// Looks up the draft theme's display name, falling back to `Velotype`.
    fn selected_theme_name(&self) -> String {
        self.theme_options
            .iter()
            .find(|entry| entry.id == self.selected_theme_id)
            .map(|entry| entry.name.clone())
            .unwrap_or_else(|| "Velotype".into())
    }

    /// Compares draft values with saved baselines, normalizing shortcut overrides.
    fn has_unsaved_changes(&self) -> bool {
        self.startup_open != self.saved_startup_open
            || self.selected_theme_id != self.saved_theme_id
            || self.image_paste_behavior != self.saved_image_paste_behavior
            || normalize_shortcut_config(&self.keybindings)
                != normalize_shortcut_config(&self.saved_keybindings)
            || self.status_bar_enabled != self.saved_status_bar_enabled
            || self.status_bar_show_word_count != self.saved_status_bar_show_word_count
            || self.status_bar_show_cursor_position != self.saved_status_bar_show_cursor_position
            || self.status_bar_show_sidebar_toggle != self.saved_status_bar_show_sidebar_toggle
            || self.status_bar_show_mode_switch != self.saved_status_bar_show_mode_switch
            || self.fonts != self.saved_fonts
            || self.pdf_export != self.saved_pdf_export
            || self.upload_command.trim() != self.saved_upload_command.trim()
    }

    /// Shows startup settings and closes dropdowns and shortcut recording.
    fn set_nav_file(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.nav = PreferencesNav::File;
        self.startup_dropdown_open = false;
        self.theme_dropdown_open = false;
        self.image_dropdown_open = false;
        self.recording_shortcut = None;
        cx.notify();
    }

    /// Shows theme/font settings and closes dropdowns and shortcut recording.
    fn set_nav_theme(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.nav = PreferencesNav::Theme;
        self.startup_dropdown_open = false;
        self.theme_dropdown_open = false;
        self.image_dropdown_open = false;
        self.recording_shortcut = None;
        cx.notify();
    }

    /// Shows image settings and closes dropdowns and shortcut recording.
    fn set_nav_image(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.nav = PreferencesNav::Image;
        self.startup_dropdown_open = false;
        self.theme_dropdown_open = false;
        self.image_dropdown_open = false;
        self.recording_shortcut = None;
        cx.notify();
    }

    /// Shows shortcut settings, closes dropdowns, and clears the recording error.
    fn set_nav_shortcuts(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.nav = PreferencesNav::Shortcuts;
        self.startup_dropdown_open = false;
        self.theme_dropdown_open = false;
        self.image_dropdown_open = false;
        self.shortcut_error = None;
        cx.notify();
    }

    /// Shows status bar settings and closes dropdowns and shortcut recording.
    fn set_nav_status_bar(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.nav = PreferencesNav::StatusBar;
        self.startup_dropdown_open = false;
        self.theme_dropdown_open = false;
        self.image_dropdown_open = false;
        self.recording_shortcut = None;
        cx.notify();
    }

    fn set_nav_export(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.nav = PreferencesNav::Export;
        self.startup_dropdown_open = false;
        self.theme_dropdown_open = false;
        self.image_dropdown_open = false;
        self.recording_shortcut = None;
        cx.notify();
    }

    /// Toggles startup options and closes the other dropdowns.
    fn toggle_startup_dropdown(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.startup_dropdown_open = !self.startup_dropdown_open;
        self.theme_dropdown_open = false;
        self.image_dropdown_open = false;
        cx.notify();
    }

    /// Toggles theme options and closes the other dropdowns.
    fn toggle_theme_dropdown(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.theme_dropdown_open = !self.theme_dropdown_open;
        self.startup_dropdown_open = false;
        self.image_dropdown_open = false;
        cx.notify();
    }

    /// Toggles image storage options and closes the other dropdowns.
    fn toggle_image_dropdown(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.image_dropdown_open = !self.image_dropdown_open;
        self.startup_dropdown_open = false;
        self.theme_dropdown_open = false;
        cx.notify();
    }

    /// Closes the window without persisting the draft.
    fn cancel(&mut self, _: &ClickEvent, window: &mut Window, _: &mut Context<Self>) {
        window.remove_window();
    }

    /// Closes without saving when the custom titlebar receives a standard click.
    fn on_titlebar_close(
        &mut self,
        event: &ClickEvent,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        if event.standard_click() {
            window.remove_window();
        }
    }

    /// Validates and saves a changed draft, prompting on write failure before applying it.
    fn save(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.has_unsaved_changes() {
            return;
        }

        let strings = cx.global::<I18nManager>().strings();
        if self.font_validation_error(strings).is_some() {
            cx.notify();
            return;
        }

        let preferences = match save_preferences_from_window(
            self.startup_open,
            &self.selected_theme_id,
            self.image_paste_behavior,
            self.keybindings.clone(),
            &StatusBarPreferences {
                enabled: self.status_bar_enabled,
                show_word_count: self.status_bar_show_word_count,
                show_cursor_position: self.status_bar_show_cursor_position,
                show_sidebar_toggle: self.status_bar_show_sidebar_toggle,
                show_mode_switch: self.status_bar_show_mode_switch,
                custom_buttons: Vec::new(),
            },
            &self.fonts,
        )
        .and_then(|mut preferences| {
            // PDF page setup is stored separately so the shared window save
            // signature stays unchanged.
            preferences.pdf_export = self.pdf_export.clone();
            preferences.image_upload_command = self.upload_command.trim().to_string();
            save_app_preferences(&preferences)?;
            Ok(preferences)
        }) {
            Ok(preferences) => preferences,
            Err(err) => {
                let strings = cx.global::<I18nManager>().strings().clone();
                let ok = strings.info_dialog_ok;
                let buttons = [ok.as_str()];
                let _ = window.prompt(
                    PromptLevel::Critical,
                    &strings.preferences_save_failed_title,
                    Some(&err.to_string()),
                    &buttons,
                    cx,
                );
                return;
            }
        };

        self.apply_saved_preferences(preferences, window, cx);
    }

    /// Applies saved theme, shortcuts, bar flags, and fonts, then resets dirty baselines.
    /// Keeps this window open, active, and focused.
    fn apply_saved_preferences(
        &mut self,
        preferences: AppPreferences,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let theme_changed = cx.update_global::<ThemeManager, _>(|theme_manager, _cx| {
            theme_manager.set_theme_by_id(&preferences.default_theme_id)
        });
        if !theme_changed {
            let _ = cx.update_global::<ThemeManager, _>(|theme_manager, _cx| {
                theme_manager.set_theme_by_id(DEFAULT_THEME_ID)
            });
        }
        if cx.global::<ThemeManager>().follows_system_appearance() {
            if self.selected_theme_id != self.saved_theme_id {
                // A newly picked theme becomes the current appearance's theme.
                if let Err(err) = apply_configured_theme(cx, &preferences.default_theme_id) {
                    eprintln!("failed to persist appearance theme: {err}");
                }
            } else {
                let dark = system_appearance_is_dark(window.appearance());
                cx.update_global::<ThemeManager, _>(|theme_manager, _cx| {
                    theme_manager.apply_system_appearance(dark)
                });
            }
        }
        cx.clear_key_bindings();
        install_keybindings(cx, &preferences.keybindings);
        crate::app_menu::install_menus(cx);
        cx.update_global::<EditorSettings, _>(|settings, _cx| {
            settings.status_bar_settings.status_bar_enabled = preferences.status_bar.enabled;
            settings.status_bar_settings.status_bar_show_word_count =
                preferences.status_bar.show_word_count;
            settings.status_bar_settings.status_bar_show_cursor_position =
                preferences.status_bar.show_cursor_position;
            settings.status_bar_settings.status_bar_show_sidebar_toggle =
                preferences.status_bar.show_sidebar_toggle;
            settings.status_bar_settings.status_bar_show_mode_switch =
                preferences.status_bar.show_mode_switch;
        });
        FontSettings::update(cx, preferences.fonts.clone())
            .expect("saved font stacks were validated");
        cx.refresh_windows();
        window.activate_window();
        self.focus_handle.focus(window);
        self.saved_startup_open = self.startup_open;
        self.saved_theme_id = self.selected_theme_id.clone();
        self.saved_image_paste_behavior = self.image_paste_behavior;
        self.saved_keybindings = normalize_shortcut_config(&self.keybindings);
        self.saved_status_bar_enabled = self.status_bar_enabled;
        self.saved_status_bar_show_word_count = self.status_bar_show_word_count;
        self.saved_status_bar_show_cursor_position = self.status_bar_show_cursor_position;
        self.saved_status_bar_show_sidebar_toggle = self.status_bar_show_sidebar_toggle;
        self.saved_status_bar_show_mode_switch = self.status_bar_show_mode_switch;
        self.saved_fonts = self.fonts.clone();
        self.saved_pdf_export = self.pdf_export.clone();
        self.saved_upload_command = self.upload_command.clone();
        cx.notify();
    }

    /// Builds a navigation button with selected-page styling and a click listener.
    fn nav_button(
        &self,
        id: &'static str,
        label: String,
        selected: bool,
        theme: &Theme,
        on_click: fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>),
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        div()
            .h(px(34.0))
            .w(px(156.0))
            .px(px(12.0))
            .flex()
            .items_center()
            .justify_end()
            .rounded(px(d.menu_item_radius))
            .cursor_pointer()
            .text_size(px(t.dialog_body_size))
            .font_weight(t.dialog_button_weight.to_font_weight())
            .text_color(if selected {
                c.dialog_primary_button_text
            } else {
                c.dialog_body
            })
            .bg(if selected {
                c.dialog_primary_button_bg
            } else {
                c.dialog_secondary_button_bg
            })
            .hover(move |this| {
                this.bg(if selected {
                    c.dialog_primary_button_hover
                } else {
                    c.dialog_secondary_button_hover
                })
            })
            .id(id)
            .child(label)
            .on_click(cx.listener(on_click))
    }

    /// Builds the current-value button that opens a preferences dropdown.
    fn dropdown_button(
        id: &'static str,
        label: String,
        theme: &Theme,
        on_click: fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>),
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        div()
            .w_full()
            .min_w(px(0.0))
            .min_h(px(36.0))
            .px(px(12.0))
            .flex()
            .items_center()
            .justify_between()
            .rounded(px(d.menu_item_radius))
            .border(px(d.dialog_border_width))
            .border_color(c.dialog_border)
            .bg(c.dialog_secondary_button_bg)
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .cursor_pointer()
            .text_size(px(t.dialog_body_size))
            .text_color(c.dialog_body)
            .id(id)
            .child(label)
            .child("v")
            .on_click(cx.listener(on_click))
    }

    /// Builds a selectable dropdown row with current-selection highlighting.
    fn dropdown_item(
        id: impl Into<ElementId>,
        label: String,
        selected: bool,
        theme: &Theme,
        on_click: impl Fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        div()
            .w_full()
            .min_w(px(0.0))
            .min_h(px(30.0))
            .px(px(12.0))
            .flex()
            .items_center()
            .rounded(px(d.menu_item_radius))
            .cursor_pointer()
            .bg(if selected {
                c.selection
            } else {
                c.dialog_surface
            })
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .text_size(px(t.dialog_body_size))
            .text_color(c.dialog_body)
            .id(id)
            .child(label)
            .on_click(cx.listener(on_click))
    }

    /// Stacks a field label above its full-width control.
    fn labeled_row(&self, label: &str, control: impl IntoElement, theme: &Theme) -> Div {
        let c = &theme.colors;
        let t = &theme.typography;
        div()
            .w_full()
            .min_w(px(0.0))
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(
                div()
                    .w_full()
                    .min_w(px(0.0))
                    .text_size(px(t.dialog_body_size))
                    .font_weight(t.dialog_button_weight.to_font_weight())
                    .text_color(c.dialog_title)
                    .child(SharedString::from(label.to_string())),
            )
            .child(div().w_full().min_w(px(0.0)).child(control))
    }

    /// Renders the draft startup document policy dropdown.
    fn render_startup_page(
        &self,
        theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        cx: &mut Context<Self>,
    ) -> Div {
        let selected = match self.startup_open {
            StartupOpenPreference::NewFile => strings.preferences_startup_new_file.clone(),
            StartupOpenPreference::LastOpenedFile => {
                strings.preferences_startup_last_opened_file.clone()
            }
        };
        let mut dropdown = div()
            .w_full()
            .min_w(px(0.0))
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(Self::dropdown_button(
                "preferences-startup-dropdown",
                selected,
                theme,
                Self::toggle_startup_dropdown,
                cx,
            ));
        if self.startup_dropdown_open {
            let new_file_label = strings.preferences_startup_new_file.clone();
            let last_file_label = strings.preferences_startup_last_opened_file.clone();
            dropdown = dropdown
                .child(Self::dropdown_item(
                    "preferences-startup-new-file",
                    new_file_label,
                    self.startup_open == StartupOpenPreference::NewFile,
                    theme,
                    |this, _, _, cx| {
                        this.startup_open = StartupOpenPreference::NewFile;
                        this.startup_dropdown_open = false;
                        cx.notify();
                    },
                    cx,
                ))
                .child(Self::dropdown_item(
                    "preferences-startup-last-opened-file",
                    last_file_label,
                    self.startup_open == StartupOpenPreference::LastOpenedFile,
                    theme,
                    |this, _, _, cx| {
                        this.startup_open = StartupOpenPreference::LastOpenedFile;
                        this.startup_dropdown_open = false;
                        cx.notify();
                    },
                    cx,
                ));
        }
        self.labeled_row(&strings.preferences_startup_option, dropdown, theme)
    }

    /// Renders theme selection, font inputs, syntax errors, and availability warnings.
    fn render_theme_page(
        &self,
        theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut dropdown = div()
            .w_full()
            .min_w(px(0.0))
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(Self::dropdown_button(
                "preferences-theme-dropdown",
                self.selected_theme_name(),
                theme,
                Self::toggle_theme_dropdown,
                cx,
            ));
        if self.theme_dropdown_open {
            let mut list = div()
                .id("preferences-theme-dropdown-list")
                .w_full()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .gap(px(4.0));

            for (index, entry) in self.theme_options.clone().into_iter().enumerate() {
                let selected = entry.id == self.selected_theme_id;
                list = list.child(Self::dropdown_item(
                    ("preferences-theme-option", index),
                    entry.name,
                    selected,
                    theme,
                    move |this, _, _, cx| {
                        this.selected_theme_id = entry.id.clone();
                        this.theme_dropdown_open = false;
                        cx.notify();
                    },
                    cx,
                ));
            }
            dropdown = dropdown.child(list);
        }
        let c = &theme.colors;
        let t = &theme.typography;
        let font_error = self.font_validation_error(strings);
        let warning = |uses_system_default: bool| {
            if !uses_system_default {
                div().into_any_element()
            } else {
                div()
                    .w_full()
                    .min_w(px(0.0))
                    .text_size(px((t.dialog_body_size - 2.).max(10.)))
                    .text_color(c.callout_warning_border)
                    .child(strings.preferences_font_unavailable_template.clone())
                    .into_any_element()
            }
        };
        let fields = div()
            .w_full()
            .min_w(px(0.0))
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(self.labeled_row(
                &strings.preferences_font_body,
                self.body_font_input.clone(),
                theme,
            ))
            .child(warning(
                font_error.is_none() && self.font_uses_system_default[0],
            ))
            .child(self.labeled_row(
                &strings.preferences_font_code,
                self.code_font_input.clone(),
                theme,
            ))
            .child(warning(
                font_error.is_none() && self.font_uses_system_default[1],
            ))
            .child(self.labeled_row(
                &strings.preferences_font_ui,
                self.ui_font_input.clone(),
                theme,
            ))
            .child(warning(
                font_error.is_none() && self.font_uses_system_default[2],
            ))
            .when_some(font_error, |this, error| {
                this.child(
                    div()
                        .w_full()
                        .min_w(px(0.0))
                        .text_size(px(t.dialog_body_size))
                        .text_color(c.dialog_danger_button_bg)
                        .child(error),
                )
            });
        div()
            .id("preferences-theme-scroll")
            .w_full()
            .h_full()
            .flex_1()
            .min_h(px(0.0))
            .min_w(px(0.0))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(18.))
            .pr(px(4.0))
            .child(self.labeled_row(&strings.preferences_local_theme, dropdown, theme))
            .child(
                div()
                    .w_full()
                    .min_w(px(0.0))
                    .text_size(px((t.dialog_body_size - 2.).max(10.)))
                    .text_color(c.dialog_muted)
                    .child(strings.preferences_font_help.clone()),
            )
            .child(fields)
    }

    /// Resolves the localized label for an image storage policy.
    fn image_paste_behavior_label(
        behavior: ImagePasteBehavior,
        strings: &crate::i18n::I18nStrings,
    ) -> String {
        match behavior {
            ImagePasteBehavior::None => strings.preferences_image_paste_none.clone(),
            ImagePasteBehavior::CopyToDocumentFolder => strings
                .preferences_image_paste_copy_to_document_folder
                .clone(),
            ImagePasteBehavior::CopyToAssetsFolder => strings
                .preferences_image_paste_copy_to_assets_folder
                .clone(),
            ImagePasteBehavior::CopyToNamedAssetsFolder => strings
                .preferences_image_paste_copy_to_named_assets_folder
                .clone(),
            ImagePasteBehavior::Upload => strings.preferences_image_paste_upload.clone(),
        }
    }

    /// Renders the clipboard image storage policy dropdown.
    fn render_image_page(
        &self,
        theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        cx: &mut Context<Self>,
    ) -> Div {
        let options = [
            ImagePasteBehavior::None,
            ImagePasteBehavior::CopyToDocumentFolder,
            ImagePasteBehavior::CopyToAssetsFolder,
            ImagePasteBehavior::CopyToNamedAssetsFolder,
            ImagePasteBehavior::Upload,
        ];
        let mut dropdown = div()
            .w_full()
            .min_w(px(0.0))
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(Self::dropdown_button(
                "preferences-image-dropdown",
                Self::image_paste_behavior_label(self.image_paste_behavior, strings),
                theme,
                Self::toggle_image_dropdown,
                cx,
            ));
        if self.image_dropdown_open {
            for (index, behavior) in options.into_iter().enumerate() {
                let selected = behavior == self.image_paste_behavior;
                let label = Self::image_paste_behavior_label(behavior, strings);
                dropdown = dropdown.child(Self::dropdown_item(
                    ("preferences-image-option", index),
                    label,
                    selected,
                    theme,
                    move |this, _, _, cx| {
                        this.image_paste_behavior = behavior;
                        this.image_dropdown_open = false;
                        cx.notify();
                    },
                    cx,
                ));
            }
        }
        let c = &theme.colors;
        let t = &theme.typography;
        div()
            .w_full()
            .min_w(px(0.0))
            .flex()
            .flex_col()
            .gap(px(18.))
            .child(self.labeled_row(&strings.preferences_image_insert_behavior, dropdown, theme))
            .child(self.labeled_row(
                &strings.preferences_image_upload_command,
                self.upload_command_input.clone(),
                theme,
            ))
            .child(
                div()
                    .w_full()
                    .min_w(px(0.0))
                    .text_size(px((t.dialog_body_size - 2.).max(10.)))
                    .text_color(c.dialog_muted)
                    .child(strings.preferences_image_upload_help.clone()),
            )
    }

    /// Resolves the localized heading for a shortcut category.
    pub(crate) fn shortcut_category_label(
        category: ShortcutCategory,
        strings: &crate::i18n::I18nStrings,
    ) -> String {
        match category {
            ShortcutCategory::File => strings.preferences_shortcuts_group_file.clone(),
            ShortcutCategory::Edit => strings.preferences_shortcuts_group_edit.clone(),
            ShortcutCategory::Navigation => strings.preferences_shortcuts_group_navigation.clone(),
            ShortcutCategory::Formatting => strings.preferences_shortcuts_group_formatting.clone(),
            ShortcutCategory::Block => strings.preferences_shortcuts_group_block.clone(),
            ShortcutCategory::Other => strings.preferences_shortcuts_group_other.clone(),
        }
    }

    /// Resolves the localized preferences label for a shortcut command.
    pub(crate) fn shortcut_command_label(
        command: ShortcutCommand,
        strings: &crate::i18n::I18nStrings,
    ) -> String {
        match command {
            ShortcutCommand::Newline => strings.preferences_shortcut_newline.clone(),
            ShortcutCommand::DeleteBack => strings.preferences_shortcut_delete_back.clone(),
            ShortcutCommand::Delete => strings.preferences_shortcut_delete.clone(),
            ShortcutCommand::WordDeleteBack => {
                strings.preferences_shortcut_word_delete_back.clone()
            }
            ShortcutCommand::WordDeleteForward => {
                strings.preferences_shortcut_word_delete_forward.clone()
            }
            ShortcutCommand::FocusPrev => strings.preferences_shortcut_focus_prev.clone(),
            ShortcutCommand::FocusNext => strings.preferences_shortcut_focus_next.clone(),
            ShortcutCommand::MoveLeft => strings.preferences_shortcut_move_left.clone(),
            ShortcutCommand::MoveRight => strings.preferences_shortcut_move_right.clone(),
            ShortcutCommand::WordMoveLeft => strings.preferences_shortcut_word_move_left.clone(),
            ShortcutCommand::WordMoveRight => strings.preferences_shortcut_word_move_right.clone(),
            ShortcutCommand::Home => strings.preferences_shortcut_home.clone(),
            ShortcutCommand::End => strings.preferences_shortcut_end.clone(),
            ShortcutCommand::BlockUp => strings.preferences_shortcut_block_up.clone(),
            ShortcutCommand::BlockDown => strings.preferences_shortcut_block_down.clone(),
            ShortcutCommand::PageUp => strings.preferences_shortcut_page_up.clone(),
            ShortcutCommand::PageDown => strings.preferences_shortcut_page_down.clone(),
            ShortcutCommand::JumpToTop => strings.preferences_shortcut_jump_to_top.clone(),
            ShortcutCommand::JumpToBottom => strings.preferences_shortcut_jump_to_bottom.clone(),
            ShortcutCommand::SelectLeft => strings.preferences_shortcut_select_left.clone(),
            ShortcutCommand::SelectRight => strings.preferences_shortcut_select_right.clone(),
            ShortcutCommand::WordSelectLeft => {
                strings.preferences_shortcut_word_select_left.clone()
            }
            ShortcutCommand::WordSelectRight => {
                strings.preferences_shortcut_word_select_right.clone()
            }
            ShortcutCommand::SelectHome => strings.preferences_shortcut_select_home.clone(),
            ShortcutCommand::SelectEnd => strings.preferences_shortcut_select_end.clone(),
            ShortcutCommand::SelectAll => strings.preferences_shortcut_select_all.clone(),
            ShortcutCommand::Copy => strings.preferences_shortcut_copy.clone(),
            ShortcutCommand::Cut => strings.preferences_shortcut_cut.clone(),
            ShortcutCommand::Paste => strings.preferences_shortcut_paste.clone(),
            ShortcutCommand::Undo => strings.preferences_shortcut_undo.clone(),
            ShortcutCommand::Redo => strings.preferences_shortcut_redo.clone(),
            ShortcutCommand::BoldSelection => strings.preferences_shortcut_bold_selection.clone(),
            ShortcutCommand::ItalicSelection => {
                strings.preferences_shortcut_italic_selection.clone()
            }
            ShortcutCommand::UnderlineSelection => {
                strings.preferences_shortcut_underline_selection.clone()
            }
            ShortcutCommand::CodeSelection => strings.preferences_shortcut_code_selection.clone(),
            ShortcutCommand::SetLineEndingCrlf => strings.menu_line_ending_crlf.clone(),
            ShortcutCommand::SetLineEndingLf => strings.menu_line_ending_lf.clone(),
            ShortcutCommand::ToggleFinalNewline => strings.menu_final_newline.clone(),
            ShortcutCommand::ShowFileTree => strings.workspace_tab_files.clone(),
            ShortcutCommand::ShowOutline => strings.workspace_tab_outline.clone(),
            ShortcutCommand::ShowArticles => strings.workspace_tab_articles.clone(),
            ShortcutCommand::ShowLinks => strings.workspace_tab_links.clone(),
            ShortcutCommand::OpenDailyNote => strings.menu_daily_note.clone(),
            ShortcutCommand::PrintDocument => strings.menu_print.clone(),
            ShortcutCommand::ToggleToolbar => strings.menu_toolbar.clone(),
            ShortcutCommand::ExportAgain => strings.menu_export_again.clone(),
            ShortcutCommand::CopyImage => strings.menu_copy_image.clone(),
            ShortcutCommand::SetImageRoot => strings.menu_set_image_root.clone(),
            ShortcutCommand::InsertImage => strings.menu_insert_image.clone(),
            ShortcutCommand::RevealImageFile => strings.menu_reveal_image_file.clone(),
            ShortcutCommand::ToggleImageSyntax => strings.menu_toggle_image_syntax.clone(),
            ShortcutCommand::CopyImageTo => strings.menu_copy_image_to.clone(),
            ShortcutCommand::MoveImageTo => strings.menu_move_image_to.clone(),
            ShortcutCommand::DeleteImageFile => strings.menu_delete_image_file.clone(),
            ShortcutCommand::MoveDocument => strings.menu_move_document.clone(),
            ShortcutCommand::ShowProperties => strings.menu_properties.clone(),
            ShortcutCommand::ExportHtmlPlain => strings.menu_export_html_plain.clone(),
            ShortcutCommand::ToggleAutosave => strings.menu_autosave.clone(),
            ShortcutCommand::ShowBackups => strings.menu_show_backups.clone(),
            ShortcutCommand::UploadImage => strings.menu_upload_image.clone(),
            ShortcutCommand::UploadAllImages => strings.menu_upload_all_images.clone(),
            ShortcutCommand::OpenEmojiPicker => strings.menu_emoji_picker.clone(),
            ShortcutCommand::SelectLine => strings.menu_select_line.clone(),
            ShortcutCommand::SelectStyledScope => strings.menu_select_styled_scope.clone(),
            ShortcutCommand::SelectWord => strings.menu_select_word.clone(),
            ShortcutCommand::SelectBlock => strings.menu_select_block.clone(),
            ShortcutCommand::JumpToSelection => strings.menu_jump_to_selection.clone(),
            ShortcutCommand::DeleteWord => strings.menu_delete_word.clone(),
            ShortcutCommand::DeleteLine => strings.menu_delete_line.clone(),
            ShortcutCommand::DeleteStyledScope => strings.menu_delete_styled_scope.clone(),
            ShortcutCommand::DeleteBlock => strings.menu_delete_block.clone(),
            ShortcutCommand::PasteAsPlainText => strings.menu_paste_as_plain_text.clone(),
            ShortcutCommand::CopyAsPlainText => strings.menu_copy_as_plain_text.clone(),
            ShortcutCommand::CopyAsHtml => strings.menu_copy_as_html.clone(),
            ShortcutCommand::IncreaseHeadingLevel => strings.menu_heading_increase.clone(),
            ShortcutCommand::DecreaseHeadingLevel => strings.menu_heading_decrease.clone(),
            ShortcutCommand::ToggleQuote => strings.menu_quote.clone(),
            ShortcutCommand::ToggleOrderedList => strings.menu_ordered_list.clone(),
            ShortcutCommand::ToggleBulletList => strings.menu_bullet_list.clone(),
            ShortcutCommand::ToggleTaskList => strings.menu_task_list.clone(),
            ShortcutCommand::ToggleTaskStatus => strings.menu_task_status.clone(),
            ShortcutCommand::InsertTable => strings.menu_insert_table.clone(),
            ShortcutCommand::InsertCodeBlock => strings.menu_code_block.clone(),
            ShortcutCommand::InsertMathBlock => strings.menu_math_block.clone(),
            ShortcutCommand::InsertHorizontalRule => strings.menu_horizontal_rule.clone(),
            ShortcutCommand::InsertTableOfContents => strings.menu_table_of_contents.clone(),
            ShortcutCommand::InsertFootnote => strings.menu_footnote.clone(),
            ShortcutCommand::InsertLinkReference => strings.menu_link_reference.clone(),
            ShortcutCommand::InsertFrontMatter => strings.menu_front_matter.clone(),
            ShortcutCommand::StrikethroughSelection => strings.menu_strikethrough.clone(),
            ShortcutCommand::HighlightSelection => strings.menu_highlight.clone(),
            ShortcutCommand::SuperscriptSelection => strings.menu_superscript.clone(),
            ShortcutCommand::SubscriptSelection => strings.menu_subscript.clone(),
            ShortcutCommand::InlineMathSelection => strings.menu_inline_math.clone(),
            ShortcutCommand::CommentSelection => strings.menu_comment.clone(),
            ShortcutCommand::InsertLink => strings.menu_link.clone(),
            ShortcutCommand::ClearFormat => strings.menu_clear_format.clone(),
            ShortcutCommand::IndentBlock => strings.preferences_shortcut_indent_block.clone(),
            ShortcutCommand::OutdentBlock => strings.preferences_shortcut_outdent_block.clone(),
            ShortcutCommand::ExitCodeBlock => strings.preferences_shortcut_exit_code_block.clone(),
            ShortcutCommand::SaveDocument => strings.preferences_shortcut_save_document.clone(),
            ShortcutCommand::SaveDocumentAs => {
                strings.preferences_shortcut_save_document_as.clone()
            }
            ShortcutCommand::NewWindow => strings.preferences_shortcut_new_window.clone(),
            ShortcutCommand::OpenFile => strings.preferences_shortcut_open_file.clone(),
            ShortcutCommand::QuitApplication => {
                strings.preferences_shortcut_quit_application.clone()
            }
            ShortcutCommand::CloseWindow => strings.preferences_shortcut_close_window.clone(),
            ShortcutCommand::ToggleAutoPair => strings.menu_auto_pair.clone(),
            ShortcutCommand::ToggleSmartQuotes => strings.menu_smart_quotes.clone(),
            ShortcutCommand::ToggleSmartDashes => strings.menu_smart_dashes.clone(),
            ShortcutCommand::ToggleFocusMode => strings.menu_focus_mode.clone(),
            ShortcutCommand::ToggleTypewriterMode => strings.menu_typewriter_mode.clone(),
            ShortcutCommand::ToggleStatusBar => strings.menu_status_bar.clone(),
            ShortcutCommand::ToggleFullScreen => strings.menu_toggle_full_screen.clone(),
            ShortcutCommand::ToggleAlwaysOnTop => strings.menu_always_on_top.clone(),
            ShortcutCommand::ResetZoom => strings.menu_actual_size.clone(),
            ShortcutCommand::ZoomIn => strings.menu_zoom_in.clone(),
            ShortcutCommand::ZoomOut => strings.menu_zoom_out.clone(),
            ShortcutCommand::ToggleReadOnly => strings.menu_read_only.clone(),
            ShortcutCommand::ToggleSpellcheck => strings.menu_spellcheck.clone(),
            ShortcutCommand::OpenFolder => strings.menu_open_folder.clone(),
            ShortcutCommand::CloseFolder => strings.menu_close_folder.clone(),
            ShortcutCommand::ReloadFromDisk => strings.menu_reload_from_disk.clone(),
            ShortcutCommand::SaveAll => strings.menu_save_all.clone(),
            ShortcutCommand::ReopenClosedTab => strings.menu_reopen_closed_tab.clone(),
            ShortcutCommand::RevealInFileManager => strings.menu_reveal_in_file_manager.clone(),
            ShortcutCommand::RevealInSidebar => strings.menu_reveal_in_sidebar.clone(),
            ShortcutCommand::DeleteCurrentFile => strings.menu_delete_file.clone(),
            ShortcutCommand::ClearRecentFiles => strings.menu_clear_recent.clone(),
            ShortcutCommand::QuickOpen => strings.preferences_shortcut_quick_open.clone(),
            ShortcutCommand::SearchInWorkspace => {
                strings.preferences_shortcut_search_workspace.clone()
            }
            ShortcutCommand::DismissTransientUi => {
                strings.preferences_shortcut_dismiss_transient_ui.clone()
            }
            ShortcutCommand::ToggleViewMode => {
                strings.preferences_shortcut_toggle_view_mode.clone()
            }
            ShortcutCommand::FindInDocument => strings.preferences_shortcut_find.clone(),
            ShortcutCommand::FindAndReplace => strings.preferences_shortcut_find_replace.clone(),
            ShortcutCommand::FindNext => strings.preferences_shortcut_find_next.clone(),
            ShortcutCommand::FindPrevious => strings.preferences_shortcut_find_previous.clone(),
            ShortcutCommand::ToggleWorkspace => {
                strings.preferences_shortcut_toggle_workspace.clone()
            }
            ShortcutCommand::SetParagraph => strings.preferences_shortcut_set_paragraph.clone(),
            ShortcutCommand::SetHeading1 => Self::heading_shortcut_label(strings, 1),
            ShortcutCommand::SetHeading2 => Self::heading_shortcut_label(strings, 2),
            ShortcutCommand::SetHeading3 => Self::heading_shortcut_label(strings, 3),
            ShortcutCommand::SetHeading4 => Self::heading_shortcut_label(strings, 4),
            ShortcutCommand::SetHeading5 => Self::heading_shortcut_label(strings, 5),
            ShortcutCommand::SetHeading6 => Self::heading_shortcut_label(strings, 6),
            ShortcutCommand::NewTab => strings.preferences_shortcut_new_tab.clone(),
            ShortcutCommand::CloseTab => strings.preferences_shortcut_close_tab.clone(),
            ShortcutCommand::NextTab => strings.preferences_shortcut_next_tab.clone(),
            ShortcutCommand::PrevTab => strings.preferences_shortcut_prev_tab.clone(),
        }
    }

    fn heading_shortcut_label(strings: &crate::i18n::I18nStrings, level: u8) -> String {
        Self::format_template(
            &strings.preferences_shortcut_set_heading,
            "{level}",
            &level.to_string(),
        )
    }

    /// Replaces every occurrence of one placeholder in a localized message.
    fn format_template(template: &str, key: &str, value: &str) -> String {
        template.replace(key, value)
    }

    /// Arms a command for key capture and focuses the window-level receiver.
    fn begin_recording_shortcut(
        &mut self,
        command: ShortcutCommand,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.recording_shortcut = Some(command);
        self.shortcut_error = None;
        window.focus(&self.focus_handle);
        cx.notify();
    }

    /// Removes a command override and clears any recording state for that command.
    fn reset_shortcut(
        &mut self,
        command: ShortcutCommand,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(definition) = shortcut_definitions()
            .iter()
            .find(|definition| definition.command == command)
        {
            self.keybindings.remove(definition.id);
        }
        if self.recording_shortcut == Some(command) {
            self.recording_shortcut = None;
        }
        self.shortcut_error = None;
        cx.notify();
    }

    /// Captures a nonrepeated key, rejecting invalid/conflicting bindings; Escape cancels.
    fn capture_shortcut_key(
        &mut self,
        event: &KeyDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(command) = self.recording_shortcut else {
            return;
        };
        cx.stop_propagation();
        if event.is_held {
            return;
        }

        let key = event.keystroke.unparse();
        if key == "escape" {
            self.recording_shortcut = None;
            self.shortcut_error = None;
            cx.notify();
            return;
        }

        let Some(keys) = normalize_shortcut_keys(std::slice::from_ref(&key)) else {
            let strings = cx.global::<I18nManager>().strings();
            self.shortcut_error = Some(Self::format_template(
                &strings.preferences_shortcut_invalid_template,
                "{shortcut}",
                &key,
            ));
            cx.notify();
            return;
        };

        if let Some(conflict) = shortcut_conflict_for(command, &keys, &self.keybindings) {
            let strings = cx.global::<I18nManager>().strings();
            let label = Self::shortcut_command_label(conflict.command, strings);
            self.shortcut_error = Some(Self::format_template(
                &strings.preferences_shortcut_conflict_template,
                "{command}",
                &label,
            ));
            cx.notify();
            return;
        }

        if let Some(definition) = shortcut_definitions()
            .iter()
            .find(|definition| definition.command == command)
        {
            let defaults = definition
                .default_keys
                .iter()
                .map(|key| key.to_string())
                .collect::<Vec<_>>();
            if keys == defaults {
                self.keybindings.remove(definition.id);
            } else {
                self.keybindings.insert(definition.id.to_string(), keys);
            }
        }
        self.recording_shortcut = None;
        self.shortcut_error = None;
        cx.notify();
    }

    /// Builds a compact shortcut-key or recording-state label.
    fn shortcut_chip(label: &str, theme: &Theme) -> impl IntoElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        div()
            .min_w(px(58.0))
            .h(px(24.0))
            .px(px(8.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px((d.menu_item_radius - 1.0).max(0.0)))
            .border(px(d.dialog_border_width))
            .border_color(c.dialog_border)
            .bg(c.code_bg)
            .text_size(px((t.dialog_body_size - 1.0).max(10.0)))
            .text_color(c.code_text)
            .child(SharedString::from(label.to_string()))
    }

    /// Builds a shortcut row's record or reset button.
    fn shortcut_action_button(
        id: impl Into<ElementId>,
        label: String,
        theme: &Theme,
        on_click: impl Fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        div()
            .id(id)
            .h(px(28.0))
            .px(px(10.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px((d.dialog_radius - 5.0).max(0.0)))
            .border(px(d.dialog_border_width))
            .border_color(c.dialog_border)
            .bg(c.dialog_secondary_button_bg)
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .cursor_pointer()
            .text_size(px((t.dialog_button_size - 1.0).max(10.0)))
            .font_weight(t.dialog_button_weight.to_font_weight())
            .text_color(c.dialog_secondary_button_text)
            .child(label)
            .on_click(cx.listener(on_click))
    }

    /// Renders one command's resolved keys and record/reset controls.
    fn render_shortcut_row(
        &self,
        definition: ShortcutDefinition,
        theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let is_recording = self.recording_shortcut == Some(definition.command);
        let keys = resolved_shortcut_keys(&self.keybindings, definition.command);
        let label = Self::shortcut_command_label(definition.command, strings);
        let command = definition.command;

        let mut chips = div().flex().flex_wrap().gap(px(6.0));
        if is_recording {
            chips = chips.child(Self::shortcut_chip(
                &strings.preferences_shortcut_recording,
                theme,
            ));
        } else {
            for key in keys {
                chips = chips.child(Self::shortcut_chip(&key, theme));
            }
        }

        div()
            .w_full()
            .min_h(px(42.0))
            .px(px(10.0))
            .py(px(6.0))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.0))
            .rounded(px(d.menu_item_radius))
            .bg(c.dialog_surface)
            .child(
                div()
                    .min_w(px(144.0))
                    .text_size(px(t.dialog_body_size))
                    .text_color(c.dialog_body)
                    .child(label),
            )
            .child(div().flex_1().child(chips))
            .child(
                div()
                    .flex()
                    .gap(px(6.0))
                    .child(Self::shortcut_action_button(
                        ("preferences-shortcut-record", definition.command as u32),
                        strings.preferences_shortcut_record.clone(),
                        theme,
                        move |this, event, window, cx| {
                            this.begin_recording_shortcut(command, event, window, cx)
                        },
                        cx,
                    ))
                    .child(Self::shortcut_action_button(
                        ("preferences-shortcut-reset", definition.command as u32),
                        strings.preferences_shortcut_reset.clone(),
                        theme,
                        move |this, event, window, cx| {
                            this.reset_shortcut(command, event, window, cx)
                        },
                        cx,
                    )),
            )
    }

    /// Renders categorized shortcut rows and the current validation error.
    fn render_shortcuts_page(
        &self,
        theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        cx: &mut Context<Self>,
    ) -> Div {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let mut content = div()
            .id("preferences-shortcuts-scroll")
            .w_full()
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(18.0))
            .pr(px(4.0));

        let categories = [
            ShortcutCategory::File,
            ShortcutCategory::Edit,
            ShortcutCategory::Navigation,
            ShortcutCategory::Formatting,
            ShortcutCategory::Block,
            ShortcutCategory::Other,
        ];

        for category in categories {
            let mut group = div().w_full().flex().flex_col().gap(px(8.0)).child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(
                        div()
                            .text_size(px(t.dialog_body_size))
                            .font_weight(t.dialog_button_weight.to_font_weight())
                            .text_color(c.dialog_title)
                            .child(Self::shortcut_category_label(category, strings)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .h(px(d.dialog_border_width.max(1.0)))
                            .bg(c.dialog_border),
                    ),
            );
            for definition in shortcut_definitions()
                .iter()
                .copied()
                .filter(|definition| definition.category == category)
            {
                group = group.child(self.render_shortcut_row(definition, theme, strings, cx));
            }
            content = content.child(group);
        }

        let mut page = div()
            .w_full()
            .h_full()
            .flex_1()
            .min_h(px(0.0))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(8.0));
        if let Some(error) = &self.shortcut_error {
            page = page.child(
                div()
                    .w_full()
                    .flex_shrink_0()
                    .text_size(px(t.dialog_body_size))
                    .text_color(c.dialog_danger_button_bg)
                    .child(error.clone()),
            );
        }
        page.child(content)
    }

    /// Renders the draft status bar and built-in item visibility switches.
    fn render_status_bar_page(
        &self,
        theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        cx: &mut Context<Self>,
    ) -> Div {
        let c = &theme.colors;
        let t = &theme.typography;

        let switch_row = |label: &str,
                          checked: bool,
                          on_click: fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>),
                          cx: &mut Context<Self>| {
            div()
                .w(px(280.0))
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(t.dialog_body_size))
                        .text_color(c.dialog_body)
                        .child(SharedString::from(label.to_string())),
                )
                .child(
                    Switch::new(ElementId::Name(
                        format!("preferences-toggle-{}", label).into(),
                    ))
                    .checked(checked)
                    .on_click(cx.listener(on_click)),
                )
        };

        let items = div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(switch_row(
                &strings.preferences_status_bar_enabled,
                self.status_bar_enabled,
                |this, _, _, cx| {
                    this.status_bar_enabled = !this.status_bar_enabled;
                    cx.notify();
                },
                cx,
            ))
            .child(switch_row(
                &strings.preferences_status_bar_show_word_count,
                self.status_bar_show_word_count,
                |this, _, _, cx| {
                    this.status_bar_show_word_count = !this.status_bar_show_word_count;
                    cx.notify();
                },
                cx,
            ))
            .child(switch_row(
                &strings.preferences_status_bar_show_cursor_position,
                self.status_bar_show_cursor_position,
                |this, _, _, cx| {
                    this.status_bar_show_cursor_position = !this.status_bar_show_cursor_position;
                    cx.notify();
                },
                cx,
            ))
            .child(switch_row(
                &strings.preferences_status_bar_show_sidebar_toggle,
                self.status_bar_show_sidebar_toggle,
                |this, _, _, cx| {
                    this.status_bar_show_sidebar_toggle = !this.status_bar_show_sidebar_toggle;
                    cx.notify();
                },
                cx,
            ))
            .child(switch_row(
                &strings.preferences_status_bar_show_mode_switch,
                self.status_bar_show_mode_switch,
                |this, _, _, cx| {
                    this.status_bar_show_mode_switch = !this.status_bar_show_mode_switch;
                    cx.notify();
                },
                cx,
            ));

        div()
            .w_full()
            .flex_1()
            .min_h(px(0.0))
            .flex()
            .items_center()
            .justify_center()
            .child(items)
    }

    /// Compact single-choice button used by the export page.
    fn choice_chip(
        id: impl Into<ElementId>,
        label: String,
        selected: bool,
        theme: &Theme,
        on_click: impl Fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        div()
            .id(id)
            .min_h(px(30.0))
            .px(px(12.0))
            .flex()
            .items_center()
            .rounded(px(d.menu_item_radius))
            .border(px(d.dialog_border_width))
            .border_color(c.dialog_border)
            .cursor_pointer()
            .text_size(px(t.dialog_body_size))
            .text_color(if selected {
                c.dialog_primary_button_text
            } else {
                c.dialog_body
            })
            .bg(if selected {
                c.dialog_primary_button_bg
            } else {
                c.dialog_secondary_button_bg
            })
            .hover(move |this| {
                this.bg(if selected {
                    c.dialog_primary_button_hover
                } else {
                    c.dialog_secondary_button_hover
                })
            })
            .child(label)
            .on_click(cx.listener(on_click))
    }

    fn render_export_page(
        &self,
        theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        cx: &mut Context<Self>,
    ) -> Div {
        let chips = || div().w_full().flex().flex_wrap().gap(px(6.0));

        let mut paper = chips();
        for (index, size) in PdfPaperSize::ALL.into_iter().enumerate() {
            paper = paper.child(Self::choice_chip(
                ("preferences-pdf-paper", index),
                size.label().to_string(),
                self.pdf_export.paper_size == size,
                theme,
                move |this, _, _, cx| {
                    this.pdf_export.paper_size = size;
                    cx.notify();
                },
                cx,
            ));
        }

        let mut orientation = chips();
        for (index, (value, label)) in [
            (
                PdfOrientation::Portrait,
                &strings.preferences_pdf_orientation_portrait,
            ),
            (
                PdfOrientation::Landscape,
                &strings.preferences_pdf_orientation_landscape,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            orientation = orientation.child(Self::choice_chip(
                ("preferences-pdf-orientation", index),
                label.clone(),
                self.pdf_export.orientation == value,
                theme,
                move |this, _, _, cx| {
                    this.pdf_export.orientation = value;
                    cx.notify();
                },
                cx,
            ));
        }

        // Presets cover common layouts; a custom value from config.toml stays
        // selectable alongside them.
        let mut margins = vec![5, 10, DEFAULT_PDF_MARGIN_MM, 20, 25];
        if !margins.contains(&self.saved_pdf_export.margin_mm) {
            margins.push(self.saved_pdf_export.margin_mm);
            margins.sort_unstable();
        }
        let mut margin = chips();
        for (index, value) in margins.into_iter().enumerate() {
            margin = margin.child(Self::choice_chip(
                ("preferences-pdf-margin", index),
                format!("{value} mm"),
                self.pdf_export.margin_mm == value,
                theme,
                move |this, _, _, cx| {
                    this.pdf_export.margin_mm = value;
                    cx.notify();
                },
                cx,
            ));
        }

        let c = &theme.colors;
        let t = &theme.typography;
        let page_numbers = div()
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_size(px(t.dialog_body_size))
                    .font_weight(t.dialog_button_weight.to_font_weight())
                    .text_color(c.dialog_title)
                    .child(strings.preferences_pdf_page_numbers.clone()),
            )
            .child(
                Switch::new("preferences-pdf-page-numbers")
                    .checked(self.pdf_export.page_numbers)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.pdf_export.page_numbers = !this.pdf_export.page_numbers;
                        cx.notify();
                    })),
            );

        div()
            .w_full()
            .min_w(px(0.0))
            .flex()
            .flex_col()
            .gap(px(18.0))
            .child(self.labeled_row(&strings.preferences_pdf_paper_size, paper, theme))
            .child(self.labeled_row(&strings.preferences_pdf_orientation, orientation, theme))
            .child(self.labeled_row(&strings.preferences_pdf_margin, margin, theme))
            .child(page_numbers)
    }
}

impl Render for PreferencesWindow {
    /// Renders navigation, the active page, and draft-aware save/cancel controls.
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<ThemeManager>().current().clone();
        let strings = cx.global::<I18nManager>().strings().clone();
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let can_save = self.has_unsaved_changes();
        let window_title =
            SharedString::from(format!("Velotype - {}", strings.preferences_window_title));
        window.set_window_title(window_title.as_ref());
        let titlebar_height = custom_titlebar_height(window, d);

        let content = div()
            .size_full()
            .pt(px(titlebar_height))
            .flex()
            .key_context("Preferences")
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::capture_shortcut_key))
            .bg(c.editor_background)
            .text_color(c.dialog_body)
            .font(FontSettings::ui_font(cx))
            .child(
                div()
                    .w(relative(0.3))
                    .h_full()
                    .pr(px(20.0))
                    .flex()
                    .items_center()
                    .justify_end()
                    .border_r(px(d.dialog_border_width))
                    .border_color(c.dialog_border)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.0))
                            .child(self.nav_button(
                                "preferences-nav-file",
                                strings.preferences_nav_file.clone(),
                                self.nav == PreferencesNav::File,
                                &theme,
                                Self::set_nav_file,
                                cx,
                            ))
                            .child(self.nav_button(
                                "preferences-nav-theme",
                                strings.preferences_nav_theme.clone(),
                                self.nav == PreferencesNav::Theme,
                                &theme,
                                Self::set_nav_theme,
                                cx,
                            ))
                            .child(self.nav_button(
                                "preferences-nav-image",
                                strings.preferences_nav_image.clone(),
                                self.nav == PreferencesNav::Image,
                                &theme,
                                Self::set_nav_image,
                                cx,
                            ))
                            .child(self.nav_button(
                                "preferences-nav-shortcuts",
                                strings.preferences_nav_shortcuts.clone(),
                                self.nav == PreferencesNav::Shortcuts,
                                &theme,
                                Self::set_nav_shortcuts,
                                cx,
                            ))
                            .child(self.nav_button(
                                "preferences-nav-status-bar",
                                strings.preferences_nav_status_bar.clone(),
                                self.nav == PreferencesNav::StatusBar,
                                &theme,
                                Self::set_nav_status_bar,
                                cx,
                            ))
                            .child(self.nav_button(
                                "preferences-nav-export",
                                strings.preferences_nav_export.clone(),
                                self.nav == PreferencesNav::Export,
                                &theme,
                                Self::set_nav_export,
                                cx,
                            )),
                    ),
            )
            .child(
                div()
                    .w(relative(0.7))
                    .h_full()
                    .p(px(d.dialog_padding))
                    .flex()
                    .flex_col()
                    .gap(px(d.dialog_gap))
                    .child(
                        div()
                            .w_full()
                            .flex_1()
                            .min_h(px(0.0))
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(d.dialog_gap * 1.5))
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .text_size(px(t.dialog_title_size))
                                    .font_weight(t.dialog_title_weight.to_font_weight())
                                    .text_color(c.dialog_title)
                                    .child(match self.nav {
                                        PreferencesNav::File => {
                                            strings.preferences_nav_file.clone()
                                        }
                                        PreferencesNav::Theme => {
                                            strings.preferences_nav_theme.clone()
                                        }
                                        PreferencesNav::Image => {
                                            strings.preferences_nav_image.clone()
                                        }
                                        PreferencesNav::Shortcuts => {
                                            strings.preferences_nav_shortcuts.clone()
                                        }
                                        PreferencesNav::StatusBar => {
                                            strings.preferences_nav_status_bar.clone()
                                        }
                                        PreferencesNav::Export => {
                                            strings.preferences_nav_export.clone()
                                        }
                                    }),
                            )
                            .child(match self.nav {
                                PreferencesNav::File => div()
                                    .id("preferences-file-scroll")
                                    .w_full()
                                    .h_full()
                                    .flex_1()
                                    .min_h(px(0.0))
                                    .min_w(px(0.0))
                                    .overflow_y_scroll()
                                    .pr(px(4.0))
                                    .child(self.render_startup_page(&theme, &strings, cx))
                                    .into_any_element(),
                                PreferencesNav::Theme => div()
                                    .w_full()
                                    .flex_1()
                                    .min_h(px(0.0))
                                    .child(self.render_theme_page(&theme, &strings, cx))
                                    .into_any_element(),
                                PreferencesNav::Image => div()
                                    .id("preferences-image-scroll")
                                    .w_full()
                                    .h_full()
                                    .flex_1()
                                    .min_h(px(0.0))
                                    .min_w(px(0.0))
                                    .overflow_y_scroll()
                                    .pr(px(4.0))
                                    .child(self.render_image_page(&theme, &strings, cx))
                                    .into_any_element(),
                                PreferencesNav::Shortcuts => div()
                                    .w_full()
                                    .flex_1()
                                    .min_h(px(0.0))
                                    .child(self.render_shortcuts_page(&theme, &strings, cx))
                                    .into_any_element(),
                                PreferencesNav::StatusBar => div()
                                    .w_full()
                                    .flex_1()
                                    .min_h(px(0.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(self.render_status_bar_page(&theme, &strings, cx))
                                    .into_any_element(),
                                PreferencesNav::Export => div()
                                    .id("preferences-export-scroll")
                                    .w_full()
                                    .h_full()
                                    .flex_1()
                                    .min_h(px(0.0))
                                    .min_w(px(0.0))
                                    .overflow_y_scroll()
                                    .pr(px(4.0))
                                    .child(self.render_export_page(&theme, &strings, cx))
                                    .into_any_element(),
                            }),
                    )
                    .child(
                        div()
                            .w_full()
                            .flex_shrink_0()
                            .flex()
                            .justify_end()
                            .gap(px(d.dialog_button_gap))
                            .child(
                                div()
                                    .id("preferences-cancel")
                                    .h(px(d.dialog_button_height))
                                    .px(px(d.dialog_button_padding_x))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px((d.dialog_radius - 4.0).max(0.0)))
                                    .border(px(d.dialog_border_width))
                                    .border_color(c.dialog_border)
                                    .bg(c.dialog_secondary_button_bg)
                                    .hover(|this| this.bg(c.dialog_secondary_button_hover))
                                    .cursor_pointer()
                                    .text_size(px(t.dialog_button_size))
                                    .font_weight(t.dialog_button_weight.to_font_weight())
                                    .text_color(c.dialog_secondary_button_text)
                                    .child(strings.preferences_cancel.clone())
                                    .on_click(cx.listener(Self::cancel)),
                            )
                            .child(
                                div()
                                    .id("preferences-save")
                                    .h(px(d.dialog_button_height))
                                    .px(px(d.dialog_button_padding_x))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px((d.dialog_radius - 4.0).max(0.0)))
                                    .border(px(if can_save { 0.0 } else { d.dialog_border_width }))
                                    .border_color(c.dialog_border)
                                    .bg(if can_save {
                                        c.dialog_primary_button_bg
                                    } else {
                                        c.dialog_secondary_button_bg
                                    })
                                    .hover(move |this| {
                                        if can_save {
                                            this.bg(c.dialog_primary_button_hover)
                                        } else {
                                            this.bg(c.dialog_secondary_button_bg)
                                        }
                                    })
                                    .when(can_save, |this| this.cursor_pointer())
                                    .text_size(px(t.dialog_button_size))
                                    .font_weight(t.dialog_button_weight.to_font_weight())
                                    .text_color(if can_save {
                                        c.dialog_primary_button_text
                                    } else {
                                        c.dialog_secondary_button_text
                                    })
                                    .child(strings.preferences_save.clone())
                                    .on_click(cx.listener(Self::save)),
                            ),
                    ),
            );

        let root = div()
            .size_full()
            .relative()
            .bg(c.editor_background)
            .font(FontSettings::ui_font(cx))
            .child(content);

        if let Some(titlebar) = render_custom_titlebar(
            "preferences-titlebar",
            window_title,
            &theme,
            window,
            cx,
            Self::on_titlebar_close,
        ) {
            root.child(titlebar)
        } else {
            root
        }
    }
}

/// Opens a preferences draft and installs the input filter before activation and focus.
/// Panics if the window cannot be opened or its initial update fails.
fn open_preferences_window_with_state(
    cx: &mut App,
    preferences: AppPreferences,
    theme_options: Vec<ThemeCatalogEntry>,
    title: String,
) -> WindowHandle<PreferencesWindow> {
    let bounds = Bounds::centered(None, size(px(720.0), px(480.0)), cx);
    let window_title = SharedString::from(format!("Velotype - {title}"));
    let handle = cx
        .open_window(
            velotype_window_options(window_title, bounds),
            move |_window, cx| {
                cx.new(move |cx| PreferencesWindow::new(preferences, theme_options, cx))
            },
        )
        .expect("preferences window should open");

    handle
        .update(cx, |preferences, window, _cx| {
            crate::ime::install_window_input_filter(window);
            window.activate_window();
            preferences.focus_handle.focus(window);
        })
        .expect("newly opened preferences window should be updateable");

    handle
}

/// Opens a new preferences window using persisted settings and current global catalogs.
pub(crate) fn open_preferences_window(cx: &mut App) -> WindowHandle<PreferencesWindow> {
    let preferences = match read_app_preferences() {
        Ok(preferences) => preferences,
        Err(err) => {
            eprintln!("failed to read app preferences: {err}");
            AppPreferences::default()
        }
    };
    let theme_options = cx.global::<ThemeManager>().available_themes().to_vec();
    let title = cx
        .global::<I18nManager>()
        .strings()
        .preferences_window_title
        .clone();
    open_preferences_window_with_state(cx, preferences, theme_options, title)
}

#[cfg(test)]
mod tests {
    #[test]
    fn zoom_steps_stop_at_the_ends() {
        use super::{ZOOM_STEPS, step_zoom};
        assert_eq!(step_zoom(100, true), 110);
        assert_eq!(step_zoom(100, false), 90);
        // Off-step values snap to the next step.
        assert_eq!(step_zoom(105, true), 110);
        assert_eq!(step_zoom(105, false), 100);
        assert_eq!(step_zoom(300, true), 300);
        assert_eq!(step_zoom(50, false), 50);
        assert!(ZOOM_STEPS.contains(&100));
    }

    use super::{
        AppPreferences, EditorSettings, ImagePasteBehavior, StartupOpenPreference,
        StatusBarPreferences, WritingPreferences,
        load_or_create_app_preferences_with_dirs_and_locales, open_preferences_window_with_state,
        read_app_preferences_with_dirs, save_app_preferences_with_dirs,
        save_preferences_from_window_with_dirs,
    };
    use super::{
        AppearanceThemes, DEFAULT_LANGUAGE_ID, app_preferences_from_toml_value,
        system_appearance_is_dark,
    };
    use crate::config::VelotypeConfigDirs;
    use crate::export::{PdfExportOptions, PdfOrientation, PdfPaperSize};
    use crate::fonts::FontPreferences;
    use crate::i18n::I18nManager;
    use crate::theme::{ThemeCatalogEntry, ThemeManager};
    use gpui::TestAppContext;
    use gpui::WindowAppearance;
    use std::collections::BTreeMap;

    fn init_preferences_test_app(cx: &mut TestAppContext) {
        cx.update(|cx| {
            I18nManager::init_with_language_id(cx, "en-US");
            ThemeManager::init_with_theme_id(cx, "velotype");
            crate::components::init(cx);
            EditorSettings::init(cx, true);
        });
    }

    fn default_theme_options() -> Vec<ThemeCatalogEntry> {
        vec![ThemeCatalogEntry {
            id: "velotype".into(),
            name: "Velotype".into(),
        }]
    }

    #[test]
    fn missing_preferences_file_returns_defaults() {
        let root = std::env::temp_dir().join(format!(
            "velotype-preferences-missing-{}",
            uuid::Uuid::new_v4()
        ));
        let dirs = VelotypeConfigDirs::from_root(&root);
        let preferences =
            read_app_preferences_with_dirs(&dirs).expect("missing preferences should load");
        assert_eq!(preferences, AppPreferences::default());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn partial_or_invalid_preferences_fall_back_by_field() {
        let root = std::env::temp_dir().join(format!(
            "velotype-preferences-partial-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).expect("temp root should exist");
        let dirs = VelotypeConfigDirs::from_root(&root);
        std::fs::write(
            dirs.app_config_file(),
            r#"
                [startup]
                open = "not-valid"

                [theme]
                default_theme_id = "velotype-light"
            "#,
        )
        .expect("preferences should be written");

        let preferences =
            read_app_preferences_with_dirs(&dirs).expect("partial preferences should load");
        assert_eq!(preferences.startup_open, StartupOpenPreference::NewFile);
        assert_eq!(preferences.default_language_id, "en-US");
        assert_eq!(preferences.default_theme_id, "velotype-light");
        assert_eq!(preferences.image_paste_behavior, ImagePasteBehavior::None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn invalid_font_stacks_fall_back_independently() {
        let root = std::env::temp_dir().join(format!(
            "velotype-preferences-invalid-font-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).expect("temp root should exist");
        let dirs = VelotypeConfigDirs::from_root(&root);
        std::fs::write(
            dirs.app_config_file(),
            r#"
                [fonts]
                body_stack = "Georgia, serif"
                code_stack = ""
                ui_stack = "Inter, sans-serif"
            "#,
        )
        .expect("preferences should be written");

        let preferences = read_app_preferences_with_dirs(&dirs).expect("preferences should load");
        assert_eq!(preferences.fonts.body_stack, "Georgia, serif");
        assert_eq!(
            preferences.fonts.code_stack,
            FontPreferences::default().code_stack
        );
        assert_eq!(preferences.fonts.ui_stack, "Inter, sans-serif");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn invalid_image_paste_behavior_falls_back_to_none() {
        let root = std::env::temp_dir().join(format!(
            "velotype-preferences-image-invalid-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).expect("temp root should exist");
        let dirs = VelotypeConfigDirs::from_root(&root);
        std::fs::write(
            dirs.app_config_file(),
            r#"
                [editor]
                image_paste_behavior = "somewhere-dangerous"
            "#,
        )
        .expect("preferences should be written");

        let preferences = read_app_preferences_with_dirs(&dirs).expect("preferences should load");
        assert_eq!(preferences.image_paste_behavior, ImagePasteBehavior::None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn damaged_preferences_file_returns_defaults() {
        let root = std::env::temp_dir().join(format!(
            "velotype-preferences-damaged-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).expect("temp root should exist");
        let dirs = VelotypeConfigDirs::from_root(&root);
        std::fs::write(dirs.app_config_file(), "not = [valid")
            .expect("preferences should be written");

        let preferences =
            read_app_preferences_with_dirs(&dirs).expect("damaged preferences should load");
        assert_eq!(preferences, AppPreferences::default());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn appearance_themes_default_off_and_tolerate_missing_keys() {
        let value =
            toml::from_str::<toml::Value>("[theme]\ndefault_theme_id = \"night\"\n").expect("toml");
        let preferences = app_preferences_from_toml_value(&value, DEFAULT_LANGUAGE_ID);
        assert_eq!(preferences.appearance_themes, AppearanceThemes::default());
        assert!(!preferences.appearance_themes.follow_system);

        let value = toml::from_str::<toml::Value>(
            "[theme]\nfollow_system_appearance = true\nlight_theme_id = \" \"\ndark_theme_id = \"night\"\n",
        )
        .expect("toml");
        let preferences = app_preferences_from_toml_value(&value, DEFAULT_LANGUAGE_ID);
        assert!(preferences.appearance_themes.follow_system);
        assert_eq!(
            preferences.appearance_themes.light_theme_id,
            AppearanceThemes::default().light_theme_id
        );
        assert_eq!(preferences.appearance_themes.dark_theme_id, "night");
    }

    #[test]
    fn dark_window_appearances_are_detected() {
        assert!(system_appearance_is_dark(WindowAppearance::Dark));
        assert!(system_appearance_is_dark(WindowAppearance::VibrantDark));
        assert!(!system_appearance_is_dark(WindowAppearance::Light));
        assert!(!system_appearance_is_dark(WindowAppearance::VibrantLight));
    }

    #[test]
    fn saves_and_reads_preferences() {
        let root = std::env::temp_dir().join(format!(
            "velotype-preferences-save-{}",
            uuid::Uuid::new_v4()
        ));
        let dirs = VelotypeConfigDirs::from_root(&root);
        let preferences = AppPreferences {
            startup_open: StartupOpenPreference::LastOpenedFile,
            default_language_id: "zh-CN".into(),
            default_theme_id: "velotype-light".into(),
            appearance_themes: AppearanceThemes {
                follow_system: true,
                light_theme_id: "github".into(),
                dark_theme_id: "night".into(),
            },
            show_table_headers: false,
            writing: WritingPreferences {
                auto_pair: false,
                focus_mode: true,
                typewriter_mode: true,
                spellcheck: false,
                spellcheck_language: "de-DE".into(),
                zoom_percent: 125,
                final_newline: true,
                smart_quotes: true,
                smart_dashes: true,
                autosave: true,
                toolbar: true,
            },
            image_paste_behavior: ImagePasteBehavior::CopyToAssetsFolder,
            image_upload_command: "picgo upload".into(),
            keybindings: BTreeMap::new(),
            status_bar: StatusBarPreferences::default(),
            fonts: FontPreferences {
                body_stack: "Segoe UI, sans-serif".into(),
                code_stack: "Consolas, monospace".into(),
                ui_stack: ".SystemUIFont".into(),
            },
            pdf_export: PdfExportOptions {
                paper_size: PdfPaperSize::Letter,
                orientation: PdfOrientation::Landscape,
                margin_mm: 22,
                page_numbers: true,
            },
        };

        save_app_preferences_with_dirs(&preferences, &dirs)
            .expect("preferences should save to config.toml");
        let loaded = read_app_preferences_with_dirs(&dirs).expect("preferences should read back");
        assert_eq!(loaded, preferences);

        let text =
            std::fs::read_to_string(dirs.app_config_file()).expect("config.toml should exist");
        assert!(text.contains("open = \"last_opened_file\""));
        assert!(text.contains("default_language_id = \"zh-CN\""));
        assert!(text.contains("default_theme_id = \"velotype-light\""));
        assert!(text.contains("follow_system_appearance = true"));
        assert!(text.contains("light_theme_id = \"github\""));
        assert!(text.contains("dark_theme_id = \"night\""));
        assert!(text.contains("show_table_headers = false"));
        assert!(text.contains("auto_pair = false"));
        assert!(text.contains("focus_mode = true"));
        assert!(text.contains("typewriter_mode = true"));
        assert!(text.contains("spellcheck_language = \"de-DE\""));
        assert!(text.contains("zoom_percent = 125"));
        assert!(text.contains("final_newline = true"));
        assert!(text.contains("smart_quotes = true"));
        assert!(text.contains("smart_dashes = true"));
        assert!(text.contains("autosave = true"));
        assert!(text.contains("toolbar = true"));
        assert!(text.contains("image_paste_behavior = \"copy_to_assets_folder\""));
        assert!(text.contains("image_upload_command = \"picgo upload\""));
        assert!(text.contains("[fonts]"));
        assert!(text.contains("body_stack = \"Segoe UI, sans-serif\""));
        assert!(text.contains("[pdf_export]"));
        assert!(text.contains("paper_size = \"letter\""));
        assert!(text.contains("orientation = \"landscape\""));
        assert!(text.contains("margin_mm = 22"));
        assert!(text.contains("page_numbers = true"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn missing_preferences_file_is_created_with_detected_language() {
        let root = std::env::temp_dir().join(format!(
            "velotype-preferences-create-{}",
            uuid::Uuid::new_v4()
        ));
        let dirs = VelotypeConfigDirs::from_root(&root);
        let preferences = load_or_create_app_preferences_with_dirs_and_locales(&dirs, ["zh-HK"])
            .expect("preferences should be created");
        assert_eq!(preferences.default_language_id, "zh-CN");
        assert!(dirs.app_config_file().exists());
        let text =
            std::fs::read_to_string(dirs.app_config_file()).expect("config.toml should exist");
        assert!(text.contains("[language]"));
        assert!(text.contains("default_language_id = \"zh-CN\""));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn legacy_preferences_are_normalized_with_language() {
        let root = std::env::temp_dir().join(format!(
            "velotype-preferences-legacy-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).expect("temp root should exist");
        let dirs = VelotypeConfigDirs::from_root(&root);
        std::fs::write(
            dirs.app_config_file(),
            r#"
                [startup]
                open = "last_opened_file"

                [theme]
                default_theme_id = "velotype-light"
            "#,
        )
        .expect("legacy preferences should be written");

        let preferences = load_or_create_app_preferences_with_dirs_and_locales(&dirs, ["en-GB"])
            .expect("legacy preferences should normalize");
        assert_eq!(
            preferences.startup_open,
            StartupOpenPreference::LastOpenedFile
        );
        assert_eq!(preferences.default_language_id, "en-US");
        assert_eq!(preferences.default_theme_id, "velotype-light");
        let text =
            std::fs::read_to_string(dirs.app_config_file()).expect("config.toml should exist");
        assert!(text.contains("[language]"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn saving_preferences_window_preserves_language() {
        let root = std::env::temp_dir().join(format!(
            "velotype-preferences-window-{}",
            uuid::Uuid::new_v4()
        ));
        let dirs = VelotypeConfigDirs::from_root(&root);
        let preferences = AppPreferences {
            startup_open: StartupOpenPreference::NewFile,
            default_language_id: "zh-CN".into(),
            default_theme_id: "velotype".into(),
            appearance_themes: AppearanceThemes::default(),
            show_table_headers: true,
            writing: WritingPreferences::default(),
            image_paste_behavior: ImagePasteBehavior::None,
            image_upload_command: String::new(),
            keybindings: BTreeMap::new(),
            status_bar: StatusBarPreferences::default(),
            fonts: FontPreferences::default(),
            pdf_export: PdfExportOptions::default(),
        };
        save_app_preferences_with_dirs(&preferences, &dirs)
            .expect("preferences should save to config.toml");

        let saved = save_preferences_from_window_with_dirs(
            StartupOpenPreference::LastOpenedFile,
            "velotype-light",
            ImagePasteBehavior::CopyToNamedAssetsFolder,
            BTreeMap::from([("save_document".to_string(), vec!["ctrl-alt-s".to_string()])]),
            &StatusBarPreferences::default(),
            &FontPreferences::default(),
            &dirs,
        )
        .expect("window preferences should save");
        assert_eq!(saved.default_language_id, "zh-CN");
        assert_eq!(saved.startup_open, StartupOpenPreference::LastOpenedFile);
        assert_eq!(saved.default_theme_id, "velotype-light");
        assert_eq!(
            saved.image_paste_behavior,
            ImagePasteBehavior::CopyToNamedAssetsFolder
        );
        assert_eq!(
            saved.keybindings.get("save_document"),
            Some(&vec!["ctrl-alt-s".to_string()])
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[gpui::test]
    async fn preferences_window_activates_and_focuses_on_open(cx: &mut TestAppContext) {
        init_preferences_test_app(cx);

        let handle = cx.update(|cx| {
            open_preferences_window_with_state(
                cx,
                AppPreferences::default(),
                default_theme_options(),
                "Preferences".into(),
            )
        });
        cx.run_until_parked();

        let active_window = cx.update(|cx| cx.active_window().expect("window should be active"));
        assert_eq!(active_window.window_id(), handle.window_id());
        assert!(
            handle
                .update(cx, |preferences, window, _cx| preferences
                    .focus_handle
                    .is_focused(window))
                .expect("preferences window should be updateable")
        );
        assert!(
            !handle
                .update(cx, |preferences, _window, _cx| preferences
                    .has_unsaved_changes())
                .expect("preferences window should be updateable")
        );
    }

    #[gpui::test]
    async fn preferences_dirty_state_tracks_draft_changes(cx: &mut TestAppContext) {
        init_preferences_test_app(cx);

        let handle = cx.update(|cx| {
            open_preferences_window_with_state(
                cx,
                AppPreferences::default(),
                default_theme_options(),
                "Preferences".into(),
            )
        });
        cx.run_until_parked();

        handle
            .update(cx, |preferences, _window, _cx| {
                assert!(!preferences.has_unsaved_changes());
                preferences.startup_open = StartupOpenPreference::LastOpenedFile;
                assert!(preferences.has_unsaved_changes());
                preferences.startup_open = StartupOpenPreference::NewFile;
                assert!(!preferences.has_unsaved_changes());

                preferences.image_paste_behavior = ImagePasteBehavior::CopyToAssetsFolder;
                assert!(preferences.has_unsaved_changes());
                preferences.image_paste_behavior = ImagePasteBehavior::None;
                assert!(!preferences.has_unsaved_changes());

                preferences
                    .keybindings
                    .insert("save_document".into(), vec!["ctrl-alt-s".into()]);
                assert!(preferences.has_unsaved_changes());
            })
            .expect("preferences window should be updateable");
    }

    #[gpui::test]
    async fn font_input_changes_update_the_preferences_draft(cx: &mut TestAppContext) {
        init_preferences_test_app(cx);
        let handle = cx.update(|cx| {
            open_preferences_window_with_state(
                cx,
                AppPreferences::default(),
                default_theme_options(),
                "Preferences".into(),
            )
        });
        cx.run_until_parked();
        let input = handle
            .update(cx, |preferences, _window, _cx| {
                preferences.body_font_input.clone()
            })
            .expect("preferences window should be updateable");
        input.update(cx, |input, cx| input.set_value("Georgia, serif", cx));
        cx.run_until_parked();
        handle
            .update(cx, |preferences, _window, _cx| {
                assert_eq!(preferences.fonts.body_stack, "Georgia, serif");
                assert!(preferences.has_unsaved_changes());
            })
            .expect("preferences window should be updateable");

        input.update(cx, |input, cx| input.set_value("", cx));
        cx.run_until_parked();
        handle
            .update(cx, |preferences, _window, cx| {
                let strings = cx.global::<I18nManager>().strings();
                assert_eq!(
                    preferences.font_validation_error(strings).as_deref(),
                    Some("Body font: Enter at least one font family.")
                );
            })
            .expect("preferences window should expose live font validation");
    }

    #[gpui::test]
    async fn pdf_export_draft_changes_mark_window_dirty(cx: &mut TestAppContext) {
        init_preferences_test_app(cx);

        let handle = cx.update(|cx| {
            open_preferences_window_with_state(
                cx,
                AppPreferences::default(),
                default_theme_options(),
                "Preferences".into(),
            )
        });
        cx.run_until_parked();

        handle
            .update(cx, |preferences, window, cx| {
                assert!(!preferences.has_unsaved_changes());
                preferences.pdf_export.page_numbers = true;
                preferences.pdf_export.paper_size = PdfPaperSize::A5;
                assert!(preferences.has_unsaved_changes());
                let saved = AppPreferences {
                    pdf_export: preferences.pdf_export.clone(),
                    ..AppPreferences::default()
                };
                preferences.apply_saved_preferences(saved, window, cx);
                assert!(!preferences.has_unsaved_changes());
            })
            .expect("preferences window should be updateable");
    }

    #[gpui::test]
    async fn applying_saved_preferences_keeps_window_open_and_focused(cx: &mut TestAppContext) {
        init_preferences_test_app(cx);

        let handle = cx.update(|cx| {
            open_preferences_window_with_state(
                cx,
                AppPreferences::default(),
                default_theme_options(),
                "Preferences".into(),
            )
        });
        cx.run_until_parked();

        handle
            .update(cx, |preferences, window, cx| {
                preferences.startup_open = StartupOpenPreference::LastOpenedFile;
                assert!(preferences.has_unsaved_changes());
                let saved = AppPreferences {
                    startup_open: StartupOpenPreference::LastOpenedFile,
                    ..AppPreferences::default()
                };
                preferences.apply_saved_preferences(saved, window, cx);
            })
            .expect("preferences window should be updateable");
        cx.run_until_parked();

        assert_eq!(cx.update(|cx| cx.windows().len()), 1);
        let active_window = cx.update(|cx| cx.active_window().expect("window should be active"));
        assert_eq!(active_window.window_id(), handle.window_id());
        assert!(
            handle
                .update(cx, |preferences, window, _cx| preferences
                    .focus_handle
                    .is_focused(window))
                .expect("preferences window should remain updateable")
        );
        assert!(
            !handle
                .update(cx, |preferences, _window, _cx| preferences
                    .has_unsaved_changes())
                .expect("preferences window should remain updateable")
        );
    }
}
