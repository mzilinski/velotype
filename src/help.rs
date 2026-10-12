//! Help > Quick Start, Markdown Reference and Keyboard Shortcuts. The guides
//! are bundled Markdown (German or English, by UI language); the shortcut
//! list is generated from the current key bindings. They open as read-only
//! tabs from a copy in the config folder (so the tab has a title).

use std::collections::BTreeMap;
use std::path::PathBuf;

use gpui::App;

use crate::components::{ShortcutCategory, resolved_shortcut_keys, shortcut_definitions};
use crate::i18n::I18nStrings;

const QUICK_START_DE: &str = include_str!("../assets/help/quick-start.de.md");
const QUICK_START_EN: &str = include_str!("../assets/help/quick-start.en.md");
const REFERENCE_DE: &str = include_str!("../assets/help/markdown-reference.de.md");
const REFERENCE_EN: &str = include_str!("../assets/help/markdown-reference.en.md");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HelpDocument {
    QuickStart,
    MarkdownReference,
    KeyboardShortcuts,
}

fn is_german(language_id: &str) -> bool {
    language_id.starts_with("de")
}

/// A key binding as shown to people: `ctrl-shift-e` → `Ctrl+Shift+E`
/// (`Strg` in German).
pub(crate) fn display_keystroke(keys: &str, german: bool) -> String {
    keys.split(' ')
        .map(|stroke| {
            // `ctrl--` ends with the key `-`.
            let (modifiers, key) = match stroke.strip_suffix("--") {
                Some(rest) => (rest, "-"),
                None => stroke.rsplit_once('-').unwrap_or(("", stroke)),
            };
            let mut parts = modifiers
                .split('-')
                .filter(|part| !part.is_empty())
                .map(|modifier| match modifier {
                    "ctrl" if german => "Strg".to_string(),
                    "ctrl" => "Ctrl".to_string(),
                    "cmd" => "Cmd".to_string(),
                    "alt" => "Alt".to_string(),
                    "shift" => "Shift".to_string(),
                    other => other.to_string(),
                })
                .collect::<Vec<_>>();
            let key = match key {
                "up" => "↑".to_string(),
                "down" => "↓".to_string(),
                "left" => "←".to_string(),
                "right" => "→".to_string(),
                "pageup" if german => "Bild↑".to_string(),
                "pagedown" if german => "Bild↓".to_string(),
                "pageup" => "PageUp".to_string(),
                "pagedown" => "PageDown".to_string(),
                "enter" => "Enter".to_string(),
                "tab" => "Tab".to_string(),
                "escape" => "Esc".to_string(),
                "backspace" => "Backspace".to_string(),
                "delete" => if german { "Entf" } else { "Delete" }.to_string(),
                "home" => if german { "Pos1" } else { "Home" }.to_string(),
                "end" => if german { "Ende" } else { "End" }.to_string(),
                "space" => if german { "Leertaste" } else { "Space" }.to_string(),
                key if key.len() == 1 => key.to_uppercase(),
                key => {
                    let mut chars = key.chars();
                    chars
                        .next()
                        .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                        .unwrap_or_default()
                }
            };
            parts.push(key);
            parts.join("+")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether a binding is meant for this platform (`cmd` only on macOS).
fn for_this_platform(keys: &str) -> bool {
    keys.contains("cmd-") == cfg!(target_os = "macos")
}

/// Markdown listing every command with its current shortcut, by category.
pub(crate) fn shortcuts_markdown(
    strings: &I18nStrings,
    language_id: &str,
    keybindings: &BTreeMap<String, Vec<String>>,
) -> String {
    let german = is_german(language_id);
    let categories = [
        ShortcutCategory::File,
        ShortcutCategory::Edit,
        ShortcutCategory::Formatting,
        ShortcutCategory::Block,
        ShortcutCategory::Navigation,
        ShortcutCategory::Other,
    ];
    let (command_header, keys_header, intro) = if german {
        (
            "Befehl",
            "Tastenkürzel",
            "Alle Befehle mit ihrer aktuellen Belegung. Ändern kannst du sie unter **Datei → Einstellungen → Tastenkürzel**.",
        )
    } else {
        (
            "Command",
            "Shortcut",
            "Every command with its current shortcut. Change them in **File → Preferences → Keyboard Shortcuts**.",
        )
    };
    let mut out = format!("# {}\n\n{intro}\n", strings.menu_keyboard_shortcuts);
    for category in categories {
        let mut rows = shortcut_definitions()
            .iter()
            .filter(|definition| definition.category == category)
            .map(|definition| {
                let keys = resolved_shortcut_keys(keybindings, definition.command)
                    .into_iter()
                    .filter(|keys| for_this_platform(keys))
                    .map(|keys| format!("`{}`", display_keystroke(&keys, german)))
                    .collect::<Vec<_>>()
                    .join(", ");
                let label = crate::config::preferences::PreferencesWindow::shortcut_command_label(
                    definition.command,
                    strings,
                );
                (
                    label,
                    if keys.is_empty() {
                        "–".to_string()
                    } else {
                        keys
                    },
                )
            })
            .collect::<Vec<_>>();
        if rows.is_empty() {
            continue;
        }
        rows.sort_by_key(|(label, _)| label.to_lowercase());
        let title = crate::config::preferences::PreferencesWindow::shortcut_category_label(
            category, strings,
        );
        out.push_str(&format!(
            "\n## {title}\n\n| {command_header} | {keys_header} |\n|---|---|\n"
        ));
        for (label, keys) in rows {
            out.push_str(&format!("| {} | {keys} |\n", label.replace('|', "\\|")));
        }
    }
    out
}

/// File name and Markdown of a help document in the UI language.
pub(crate) fn help_document(
    document: HelpDocument,
    strings: &I18nStrings,
    language_id: &str,
    keybindings: &BTreeMap<String, Vec<String>>,
) -> (String, String) {
    let german = is_german(language_id);
    let (name, markdown) = match document {
        HelpDocument::QuickStart => (
            strings.menu_quick_start.clone(),
            if german {
                QUICK_START_DE
            } else {
                QUICK_START_EN
            }
            .to_string(),
        ),
        HelpDocument::MarkdownReference => (
            strings.menu_markdown_reference.clone(),
            if german { REFERENCE_DE } else { REFERENCE_EN }.to_string(),
        ),
        HelpDocument::KeyboardShortcuts => (
            strings.menu_keyboard_shortcuts.clone(),
            shortcuts_markdown(strings, language_id, keybindings),
        ),
    };
    let file = name
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() || ch == ' ' || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    (format!("{}.md", file.trim()), markdown)
}

/// Opens a help document as a read-only tab.
pub(crate) fn open_help(document: HelpDocument, cx: &mut App) {
    let i18n = cx.global::<crate::i18n::I18nManager>();
    let strings = i18n.strings().clone();
    let language_id = i18n.current_language_id().to_string();
    let keybindings = crate::config::read_app_preferences()
        .map(|preferences| preferences.keybindings)
        .unwrap_or_default();
    let (file_name, markdown) = help_document(document, &strings, &language_id, &keybindings);
    let path = crate::config::VelotypeConfigDirs::from_system()
        .ok()
        .map(|dirs| dirs.help_dir().join(&file_name));
    let path: Option<PathBuf> = path.filter(|path| {
        path.parent()
            .is_some_and(|dir| std::fs::create_dir_all(dir).is_ok())
            && std::fs::write(path, &markdown).is_ok()
    });
    crate::app_menu::open_read_only_document(cx, markdown, path);
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{HelpDocument, display_keystroke, help_document};

    #[test]
    fn keystrokes_read_like_menus() {
        assert_eq!(display_keystroke("ctrl-shift-e", false), "Ctrl+Shift+E");
        assert_eq!(display_keystroke("ctrl-shift-e", true), "Strg+Shift+E");
        assert_eq!(display_keystroke("ctrl--", true), "Strg+-");
        assert_eq!(display_keystroke("alt-shift-up", false), "Alt+Shift+↑");
        assert_eq!(display_keystroke("f8", false), "F8");
        assert_eq!(display_keystroke("ctrl-pagedown", true), "Strg+Bild↓");
    }

    #[test]
    fn help_documents_follow_the_language() {
        let strings = crate::i18n::I18nManager::default().strings().clone();
        let keys = BTreeMap::new();
        let (_, de) = help_document(HelpDocument::QuickStart, &strings, "de-DE", &keys);
        assert!(de.starts_with("# Velotype – Schnellstart"));
        let (_, en) = help_document(HelpDocument::QuickStart, &strings, "zh-CN", &keys);
        assert!(en.starts_with("# Velotype – Quick Start"));
        let (name, reference) =
            help_document(HelpDocument::MarkdownReference, &strings, "en", &keys);
        assert!(name.ends_with(".md"));
        assert!(reference.contains("[TOC]"));
    }

    #[test]
    fn shortcut_list_shows_current_bindings() {
        let strings = crate::i18n::I18nManager::default().strings().clone();
        let mut keys = BTreeMap::new();
        keys.insert("save_document".to_string(), vec!["ctrl-alt-s".to_string()]);
        let (_, markdown) = help_document(HelpDocument::KeyboardShortcuts, &strings, "en", &keys);
        assert!(markdown.contains("| Command | Shortcut |"), "{markdown}");
        if !cfg!(target_os = "macos") {
            assert!(markdown.contains("`Ctrl+Alt+S`"), "{markdown}");
            assert!(markdown.contains("`Ctrl+B`"), "{markdown}");
        }
    }
}
