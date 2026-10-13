//! Native application menu, app-level actions, and window close routing.
//!
//! This module owns menu construction and the actions that operate on the
//! active editor window. The Quit action is routed to the current window so the
//! existing unsaved-changes dialog remains authoritative for that window.

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use gpui::*;

use crate::components::{
    AddLanguageConfig, AddThemeConfig, CheckForUpdates, ClearRecentFiles, CloseFolder, CloseTab,
    CloseWindow, DeleteCurrentFile, ExportHtml, ExportPdf, InstallCliTool, NewTab, NewWindow,
    NextTab, NoRecentFiles, OpenFile, OpenFolder, OpenPreferences, OpenRecentFile, PrevTab,
    QuitApplication, ReloadFromDisk, ReopenClosedTab, RevealInFileManager, RevealInSidebar,
    SaveAll, SaveDocument, SaveDocumentAs, SelectLanguage, SelectSpellcheckLanguage, SelectTheme,
    ShowAbout, ToggleAutoPair, ToggleFocusMode, ToggleFollowSystemAppearance, ToggleReadOnly,
    ToggleSpellcheck, ToggleTypewriterMode, ToggleWorkspace, UninstallCliTool,
};
use crate::components::{
    ResetZoom, ToggleAlwaysOnTop, ToggleFullScreen, ToggleSmartDashes, ToggleSmartQuotes,
    ToggleStatusBar, ZoomIn, ZoomOut,
};
use crate::config::{
    SessionDrafts, apply_configured_language, apply_configured_theme,
    import_language_config_and_select, import_theme_config_and_select, open_preferences_window,
    read_recent_files, record_recent_file, remove_recent_file,
};
use crate::editor::{Editor, EditorWindow, InfoDialogKind};
use crate::export::ExportFormat;
use crate::i18n::I18nManager;
use crate::theme::ThemeManager;
use crate::window_chrome::velotype_window_options;

/// Global app-menu state for platform menu lifecycle hooks.
#[derive(Default)]
pub(crate) struct AppMenuState {
    window_closed_subscription: Option<Subscription>,
}

impl Global for AppMenuState {}

fn window_title(file_path: Option<&Path>) -> SharedString {
    if let Some(path) = file_path {
        // OsStr::to_string_lossy returns Cow<str>; calling .to_string() on
        // it always allocates a fresh String, even for the valid-UTF-8 path
        // (the common case). Borrow the Cow directly into format! — its
        // Display impl writes the borrowed bytes straight into the output
        // String, no intermediate allocation.
        format!(
            "Velotype - {}",
            path.file_name()
                .map(|name| name.to_string_lossy())
                .unwrap_or_else(|| path.to_string_lossy())
        )
        .into()
    } else {
        SharedString::new("Velotype")
    }
}

/// Opens a new editor window whose first tab shows the given Markdown content
/// and optional path.
pub(crate) fn open_editor_window(
    cx: &mut App,
    markdown: String,
    file_path: Option<PathBuf>,
) -> WindowHandle<EditorWindow> {
    let bounds = Bounds::centered(None, size(px(1080.), px(720.)), cx);
    let title = window_title(file_path.as_deref());
    // The first editor window reopens the last opened folder, unless it was
    // started for a file outside of it.
    let first_window = !cx
        .windows()
        .iter()
        .any(|window| window.downcast::<EditorWindow>().is_some());
    let restored_folder = first_window
        .then(crate::config::read_last_workspace_folder)
        .flatten()
        .filter(|folder| {
            file_path
                .as_deref()
                .is_none_or(|path| path.starts_with(folder))
        });
    let handle = cx
        .open_window(velotype_window_options(title, bounds), move |window, cx| {
            let editor = cx.new(move |cx| Editor::from_markdown(cx, markdown, file_path));
            cx.new(|cx| EditorWindow::new(editor, window, cx))
        })
        .unwrap();

    handle
        .update(cx, |host, window, cx| {
            window.activate_window();
            host.force_install_close_guard(window, cx);
            if crate::window_pin::AlwaysOnTop::enabled(cx) {
                crate::window_pin::set_always_on_top(window, true);
            }
            if restored_folder.is_some() {
                host.set_workspace_folder(restored_folder, cx);
            }
        })
        .expect("newly opened editor window should be updateable");

    handle
}

/// The editor window that "open" requests should target: the active window
/// when it is an editor window, otherwise the most recent one.
fn current_editor_window(cx: &mut App) -> Option<WindowHandle<EditorWindow>> {
    current_window_candidates(cx)
        .into_iter()
        .find_map(|window| window.downcast::<EditorWindow>())
}

/// Shows the given document in a tab of the current editor window, opening a
/// new window only when there is none (or it cannot be updated right now).
pub(crate) fn open_markdown_in_current_window(
    cx: &mut App,
    markdown: String,
    file_path: Option<PathBuf>,
) {
    let mut document = Some((markdown, file_path));
    if let Some(handle) = current_editor_window(cx) {
        let _ = handle.update(cx, |host, window, cx| {
            if let Some((markdown, file_path)) = document.take() {
                window.activate_window();
                host.open_document(markdown, file_path, window, cx);
            }
        });
    }
    if let Some((markdown, file_path)) = document {
        open_editor_window(cx, markdown, file_path);
    }
}

/// Opens `markdown` as a read-only tab of the current editor window (Help
/// documents).
pub(crate) fn open_read_only_document(cx: &mut App, markdown: String, file_path: Option<PathBuf>) {
    let make_read_only = |editor: &gpui::Entity<Editor>, cx: &mut App| {
        editor.update(cx, |editor, cx| {
            if !editor.is_read_only() {
                editor.toggle_read_only(cx);
            }
        });
    };
    let mut document = Some((markdown, file_path));
    if let Some(handle) = current_editor_window(cx) {
        let _ = handle.update(cx, |host, window, cx| {
            if let Some((markdown, file_path)) = document.take() {
                window.activate_window();
                let editor = host.open_document(markdown, file_path, window, cx);
                make_read_only(&editor, cx);
            }
        });
    }
    if let Some((markdown, file_path)) = document {
        let handle = open_editor_window(cx, markdown, file_path);
        let _ = handle.update(cx, |host, _window, cx| {
            let editor = host.active_editor().clone();
            make_read_only(&editor, cx);
        });
    }
}

/// Reopens the untitled drafts kept when the previous session closed as
/// unsaved tabs of the current editor window (opening one if needed).
pub(crate) fn restore_session_drafts(cx: &mut App) {
    let Some(store) = SessionDrafts::get(cx) else {
        return;
    };
    let drafts = store.take_all();
    if drafts.is_empty() {
        return;
    }

    let handle = match current_editor_window(cx) {
        Some(handle) => handle,
        None => open_editor_window(cx, String::new(), None),
    };
    let _ = handle.update(cx, |host, window, cx| {
        for markdown in drafts {
            host.open_draft(markdown, window, cx);
        }
    });
}

/// Reads `path` and opens it in a tab of the current editor window.
pub(crate) fn open_file_in_tab(cx: &mut App, path: &Path) -> anyhow::Result<()> {
    if crate::text_file::is_import_document(path) {
        import_document_in_tab(cx, path.to_path_buf());
        return Ok(());
    }
    let read = crate::perf_log::Span::new(format!("Datei lesen: {}", path.display()));
    let markdown = crate::text_file::read_text_file(path)?;
    drop(read);
    crate::perf_log::note(format!("  {} Bytes", markdown.len()));
    let open = crate::perf_log::Span::new("Dokument in Tab öffnen");
    open_markdown_in_current_window(cx, markdown, Some(path.to_path_buf()));
    drop(open);
    record_recent_file_and_refresh(path, cx);
    Ok(())
}

/// Opens a Word, OpenDocument, RTF or EPUB file as a new untitled tab,
/// converted to Markdown by Pandoc in the background.
pub(crate) fn import_document_in_tab(cx: &mut App, path: PathBuf) {
    crate::perf_log::step(format!("Import mit Pandoc: {}", path.display()));
    let error_window = cx.active_window();
    let strings = cx.global::<I18nManager>().strings().clone();
    let Some(pandoc) = crate::export::pandoc::find_pandoc() else {
        show_window_prompt(
            error_window,
            &strings.pandoc_missing_title,
            &strings.pandoc_missing_message,
            cx,
        );
        return;
    };
    let import = cx.background_executor().spawn(async move {
        let media_dir = crate::export::pandoc::media_dir_for(&path);
        crate::export::pandoc::import(&pandoc, &path, &media_dir)
            .with_context(|| format!("failed to import '{}'", path.display()))
    });
    cx.spawn(async move |cx| {
        let result = import.await;
        let _ = cx.update(|cx| match result {
            Ok(markdown) => {
                crate::perf_log::note("  importiert");
                open_markdown_in_current_window(cx, markdown, None);
            }
            Err(err) => {
                crate::perf_log::note(format!("  Fehler: {err:#}"));
                show_window_prompt(
                    error_window,
                    &strings.open_failed_title,
                    &format!("{err:#}"),
                    cx,
                );
            }
        });
    })
    .detach();
}

/// The message for a file that could not be opened.
pub(crate) fn open_error_detail(err: &anyhow::Error, path: &Path, cx: &App) -> String {
    if err
        .chain()
        .any(|cause| cause.is::<crate::text_file::NotText>())
    {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        cx.global::<I18nManager>()
            .strings()
            .open_not_text_message
            .replace("{file}", &name)
    } else {
        format!("{err:#}")
    }
}

fn record_recent_file_and_refresh(path: &Path, cx: &mut App) {
    let record = crate::perf_log::Span::new("Zuletzt geöffnet: Liste schreiben");
    if let Err(err) = record_recent_file(path) {
        eprintln!("failed to update recent file history: {err}");
        return;
    }
    drop(record);
    let _menus = crate::perf_log::Span::new("Menüs neu aufbauen");
    install_menus(cx);
    cx.refresh_windows();
}

#[cfg(target_os = "macos")]
/// Check whether `/usr/local/bin/velotype` is correctly installed for this app.
///
/// Returns `true` only if the symlink exists **and** resolves (directly or via
/// one level of canonicalization) to the currently running executable.
fn is_cli_symlink_current_app() -> bool {
    let link = std::path::Path::new("/usr/local/bin/velotype");
    let Ok(target) = std::fs::read_link(link) else {
        return false; // does not exist or not a symlink
    };
    let resolved = if target.is_absolute() {
        // Canonicalize the target itself (may fail if dangling).
        std::fs::canonicalize(&target).unwrap_or(target)
    } else {
        // Relative — resolve from symlink's parent directory.
        link.parent()
            .unwrap_or(std::path::Path::new("/"))
            .join(&target)
            .canonicalize()
            .unwrap_or(target)
    };
    match std::env::current_exe() {
        Ok(exe) => resolved == exe,
        Err(_) => false,
    }
}

#[cfg(any(target_os = "macos", test))]
fn applescript_string_literal(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            _ => escaped.push(ch),
        }
    }
    escaped.push('"');
    escaped
}

#[cfg(target_os = "macos")]
pub(crate) fn install_cli_tool(cx: &mut App) {
    use std::process::Command;

    let bin_link = "/usr/local/bin/velotype";
    let strings = cx.global::<I18nManager>().strings();

    let current_exe = match std::env::current_exe() {
        Ok(path) => path,
        Err(err) => {
            show_install_cli_error(cx, &format!("Failed to get executable path: {err}"));
            return;
        }
    };

    // Only allow from a portable .app bundle (e.g. drag-installed to /Applications)
    if !current_exe
        .to_string_lossy()
        .contains(".app/Contents/MacOS/")
    {
        show_install_cli_error(
            cx,
            "Command-line tool installation requires running from an .app bundle.\n\n\
             If the app was installed via the `.pkg` installer,\n\
             the CLI command is configured automatically.",
        );
        return;
    }

    let exe_path = applescript_string_literal(&current_exe.to_string_lossy());
    let link_path = applescript_string_literal(bin_link);
    let script = format!(
        r#"set exePath to {exe_path}
set linkPath to {link_path}
do shell script "rm -f " & quoted form of linkPath & linefeed & "ln -s " & quoted form of exePath & space & quoted form of linkPath with administrator privileges"#
    );

    match Command::new("osascript").arg("-e").arg(&script).output() {
        Ok(output) => {
            if output.status.success() {
                let title = "CLI Command Installed";
                let detail = format!(
                    "Successfully installed! You can now use 'velotype' from the terminal:\n\n\
                     \x1b[1mvelotype README.md\x1b[0m\n\
                     \x1b[1mvelotype file1.md file2.md\x1b[0m\n\n\
                     Location: {bin_link}\n\n\
                     Note: If you move or delete Velotype.app,\n\
                     the 'velotype' command will stop working\n\
                     automatically (no cleanup needed)."
                );
                if let Some(window) = cx.active_window() {
                    let ok = strings.info_dialog_ok.clone();
                    let _ = window.update(cx, |_view, window, cx| {
                        let _ = window.prompt(
                            PromptLevel::Info,
                            &title,
                            Some(&detail),
                            &[ok.as_str()],
                            cx,
                        );
                    });
                }
            } else {
                // User pressed Cancel on the admin password dialog
                // or the link creation failed for another reason.
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let detail = if stderr.contains("User canceled") || stderr.contains("(-128)") {
                    "Installation cancelled.".to_string()
                } else {
                    format!("Installation failed: {stderr}")
                };
                show_install_cli_error(cx, &detail);
            }
        }
        Err(err) => {
            show_install_cli_error(cx, &format!("Failed to run installer: {err}"));
        }
    }
    // Refresh menus so the label changes between Install -> Uninstall.
    install_menus(cx);
}

#[cfg(target_os = "macos")]
pub(crate) fn uninstall_cli_tool(cx: &mut App) {
    use std::process::Command;

    let bin_link = "/usr/local/bin/velotype";
    let strings = cx.global::<I18nManager>().strings();

    if !is_cli_symlink_current_app() {
        show_install_cli_error(cx, "CLI command is not installed for this app.");
        return;
    }

    let link_path = applescript_string_literal(bin_link);
    let script = format!(
        r#"set linkPath to {link_path}
do shell script "rm -f " & quoted form of linkPath with administrator privileges"#
    );

    match Command::new("osascript").arg("-e").arg(&script).output() {
        Ok(output) => {
            if output.status.success() {
                let title = "CLI Command Uninstalled";
                let detail = "CLI command has been removed successfully.".to_string();
                if let Some(window) = cx.active_window() {
                    let ok = strings.info_dialog_ok.clone();
                    let _ = window.update(cx, |_view, window, cx| {
                        let _ = window.prompt(
                            PromptLevel::Info,
                            &title,
                            Some(&detail),
                            &[ok.as_str()],
                            cx,
                        );
                    });
                }
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let detail = if stderr.contains("User canceled") || stderr.contains("(-128)") {
                    "Uninstall cancelled.".to_string()
                } else {
                    format!("Uninstall failed: {stderr}")
                };
                show_install_cli_error(cx, &detail);
            }
        }
        Err(err) => {
            show_install_cli_error(cx, &format!("Failed to run uninstaller: {err}"));
        }
    }
    // Refresh menus so the label changes between Install -> Uninstall.
    install_menus(cx);
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn install_cli_tool(cx: &mut App) {
    show_install_cli_error(
        cx,
        "Command-line tool installation is only available on macOS.",
    );
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn uninstall_cli_tool(cx: &mut App) {
    show_install_cli_error(
        cx,
        "Command-line tool uninstallation is only available on macOS.",
    );
}

fn show_install_cli_error(cx: &mut App, detail: &str) {
    let strings = cx.global::<I18nManager>().strings();
    let title = "Install Command-Line Tool Failed";

    if let Some(window) = cx.active_window() {
        let ok = strings.info_dialog_ok.clone();
        let _ = window.update(cx, |_view, window, cx| {
            let _ = window.prompt(
                PromptLevel::Critical,
                title,
                Some(detail),
                &[ok.as_str()],
                cx,
            );
        });
    } else {
        eprintln!("{title}: {detail}");
    }
}

pub(crate) fn record_recent_file_from_editor(path: &Path, cx: &mut App) {
    record_recent_file_and_refresh(path, cx);
}

fn show_window_prompt(window: Option<AnyWindowHandle>, title: &str, detail: &str, cx: &mut App) {
    if let Some(window) = window {
        let ok = cx.global::<I18nManager>().strings().info_dialog_ok.clone();
        let shown = window.update(cx, |_view, window, cx| {
            let buttons = [ok.as_str()];
            let _ = window.prompt(PromptLevel::Critical, title, Some(detail), &buttons, cx);
        });
        crate::perf_log::note(format!(
            "  Fehlermeldung {}: {title}",
            if shown.is_ok() {
                "angezeigt"
            } else {
                "NICHT angezeigt (Fenster belegt)"
            }
        ));
    } else {
        crate::perf_log::note(format!("  Fehlermeldung ohne Fenster: {title}: {detail}"));
        eprintln!("{title}: {detail}");
    }
}

/// Flips one writing aid, persists it, and repaints windows and menus.
/// Applies a document zoom level (View > Zoom) to every window and persists it.
fn set_zoom(cx: &mut App, percent: u16) {
    crate::config::EditorSettings::update_writing(cx, |writing| writing.zoom_percent = percent);
    cx.global_mut::<ThemeManager>()
        .set_zoom(f32::from(percent) / 100.0);
    install_menus(cx);
    cx.refresh_windows();
}

fn toggle_writing_aid(cx: &mut App, update: impl FnOnce(&mut crate::config::WritingPreferences)) {
    crate::config::EditorSettings::update_writing(cx, update);
    install_menus(cx);
    cx.refresh_windows();
}

fn with_active_editor<R>(
    cx: &mut App,
    update: impl FnOnce(&mut Editor, &mut Window, &mut Context<Editor>) -> R,
) -> Option<R> {
    let window = cx.active_window()?;
    if let Some(host) = window.downcast::<EditorWindow>() {
        return host
            .update(cx, |host, window, cx| {
                host.active_editor()
                    .clone()
                    .update(cx, |editor, cx| update(editor, window, cx))
            })
            .ok();
    }
    let window = window.downcast::<Editor>()?;
    window.update(cx, update).ok()
}

fn with_active_editor_window<R>(
    cx: &mut App,
    update: impl FnOnce(&mut EditorWindow, &mut Window, &mut Context<EditorWindow>) -> R,
) -> Option<R> {
    let window = cx.active_window()?.downcast::<EditorWindow>()?;
    window.update(cx, update).ok()
}

fn show_info_dialog_on_active_editor(cx: &mut App, kind: InfoDialogKind) {
    let _ = with_active_editor(cx, move |editor, _window, cx| {
        editor.show_info_dialog(kind, cx);
    });
}

fn request_update_check_on_active_editor(cx: &mut App) {
    let _ = with_active_editor(cx, |editor, window, cx| {
        editor.request_check_updates(window, cx);
    });
}

fn recent_files_for_menu() -> Vec<PathBuf> {
    match read_recent_files() {
        Ok(paths) => paths,
        Err(err) => {
            eprintln!("failed to read recent file history: {err}");
            Vec::new()
        }
    }
}

fn open_recent_file(cx: &mut App, path: PathBuf) {
    let error_window = cx.active_window();
    open_recent_file_with_error_window(cx, path, error_window);
}

fn open_recent_file_with_error_window(
    cx: &mut App,
    path: PathBuf,
    error_window: Option<AnyWindowHandle>,
) {
    if !path.is_file() {
        if let Err(err) = remove_recent_file(&path) {
            eprintln!("failed to remove missing recent file: {err}");
        }
        install_menus(cx);
        cx.refresh_windows();
        let strings = cx.global::<I18nManager>().strings().clone();
        let detail = strings
            .recent_file_missing_message_template
            .replace("{path}", &path.to_string_lossy());
        show_window_prompt(
            error_window,
            &strings.recent_file_missing_title,
            &detail,
            cx,
        );
        return;
    }

    if let Err(err) = open_file_in_tab(cx, &path) {
        let title = cx
            .global::<I18nManager>()
            .strings()
            .open_failed_title
            .clone();
        show_window_prompt(error_window, &title, &err.to_string(), cx);
    }
}

fn is_editor_scoped_menu_action(action: &dyn Action) -> bool {
    action.as_any().is::<SaveDocument>()
        || action.as_any().is::<SaveDocumentAs>()
        || action.as_any().is::<ExportHtml>()
        || action.as_any().is::<ExportPdf>()
        || action.as_any().is::<QuitApplication>()
        || action.as_any().is::<CloseWindow>()
        || action.as_any().is::<CheckForUpdates>()
        || action.as_any().is::<ShowAbout>()
        || action.as_any().is::<InstallCliTool>()
        || action.as_any().is::<UninstallCliTool>()
        || action.as_any().is::<ToggleWorkspace>()
        || action.as_any().is::<OpenFolder>()
        || action.as_any().is::<CloseFolder>()
        || action.as_any().is::<ReloadFromDisk>()
        || action.as_any().is::<SaveAll>()
        || action.as_any().is::<RevealInFileManager>()
        || action.as_any().is::<RevealInSidebar>()
        || action.as_any().is::<DeleteCurrentFile>()
}

fn is_window_context_menu_action(action: &dyn Action) -> bool {
    action.as_any().is::<NewWindow>()
        || action.as_any().is::<OpenFile>()
        || action.as_any().is::<OpenPreferences>()
        || action.as_any().is::<OpenRecentFile>()
        || action.as_any().is::<NoRecentFiles>()
        || action.as_any().is::<AddLanguageConfig>()
        || action.as_any().is::<AddThemeConfig>()
        || action.as_any().is::<InstallCliTool>()
        || action.as_any().is::<UninstallCliTool>()
        || is_editor_scoped_menu_action(action)
}

fn current_window_candidates(cx: &mut App) -> Vec<AnyWindowHandle> {
    let mut candidates = Vec::new();
    let mut push_unique = |window: AnyWindowHandle| {
        if candidates
            .iter()
            .all(|candidate: &AnyWindowHandle| candidate.window_id() != window.window_id())
        {
            candidates.push(window);
        }
    };

    if let Some(window) = cx.active_window() {
        push_unique(window);
    }
    if let Some(windows) = cx.window_stack() {
        for window in windows {
            push_unique(window);
        }
    }
    for window in cx.windows() {
        push_unique(window);
    }

    candidates
}

fn request_close_editor_window(window: AnyWindowHandle, cx: &mut App) -> bool {
    if let Some(host) = window.downcast::<EditorWindow>() {
        return host
            .update(cx, |host, window, cx| host.request_close_window(window, cx))
            .is_ok();
    }
    let Some(window) = window.downcast::<Editor>() else {
        return false;
    };

    window
        .update(cx, |editor, window, cx| {
            editor.request_close_current_window(window, cx);
        })
        .is_ok()
}

fn request_close_current_editor_window(cx: &mut App) {
    let candidates = current_window_candidates(cx);
    if candidates.is_empty() {
        cx.quit();
        return;
    }

    for window in candidates {
        if request_close_editor_window(window, cx) {
            return;
        }
    }
}

pub(crate) fn request_quit_application(cx: &mut App) {
    let candidates = current_window_candidates(cx);
    if candidates.is_empty() {
        cx.quit();
        return;
    }

    // Ask for unsaved-changes decisions before any window keeps its drafts,
    // so an aborted quit never leaves drafts stored for windows still open.
    for window in &candidates {
        let Some(host) = window.downcast::<EditorWindow>() else {
            continue;
        };
        let needs_prompt = host
            .update(cx, |host, _window, cx| host.needs_close_prompt(cx))
            .unwrap_or(false);
        if needs_prompt {
            let _ = host.update(cx, |host, window, cx| {
                window.activate_window();
                host.on_window_should_close(window, cx)
            });
            return;
        }
    }

    for window in candidates {
        let should_close = if let Some(host) = window.downcast::<EditorWindow>() {
            host.update(cx, |host, window, cx| {
                host.on_window_should_close(window, cx)
            })
            .unwrap_or(false)
        } else if let Some(window) = window.downcast::<Editor>() {
            window
                .update(cx, |editor, window, cx| {
                    editor.on_window_should_close(window, cx)
                })
                .unwrap_or(false)
        } else {
            continue;
        };
        if !should_close {
            return;
        }
    }

    cx.quit();
}

/// Executes one of the app-menu actions against the current application state.
pub(crate) fn dispatch_menu_action(action: &dyn Action, cx: &mut App) {
    if action.as_any().is::<NewWindow>() {
        open_editor_window(cx, String::new(), None);
    } else if action.as_any().is::<OpenFile>() {
        prompt_and_open_files(cx);
    } else if action.as_any().is::<OpenPreferences>() {
        open_preferences_window(cx);
    } else if let Some(action) = action.as_any().downcast_ref::<OpenRecentFile>() {
        open_recent_file(cx, PathBuf::from(&action.path));
    } else if action.as_any().is::<NoRecentFiles>() {
    } else if action.as_any().is::<AddLanguageConfig>() {
        prompt_and_import_language_config(cx);
    } else if action.as_any().is::<AddThemeConfig>() {
        prompt_and_import_theme_config(cx);
    } else if action.as_any().is::<crate::components::OpenQuickStart>() {
        crate::help::open_help(crate::help::HelpDocument::QuickStart, cx);
    } else if action
        .as_any()
        .is::<crate::components::OpenMarkdownReference>()
    {
        crate::help::open_help(crate::help::HelpDocument::MarkdownReference, cx);
    } else if action
        .as_any()
        .is::<crate::components::OpenKeyboardShortcuts>()
    {
        crate::help::open_help(crate::help::HelpDocument::KeyboardShortcuts, cx);
    } else if action.as_any().is::<crate::components::OpenThemeFolder>() {
        // Typora CSS themes and Velotype JSON themes placed here are listed in
        // the Theme menu after a restart (or right away when imported).
        match crate::config::VelotypeConfigDirs::from_system() {
            Ok(dirs) => {
                let dir = dirs.themes_dir();
                let _ = std::fs::create_dir_all(&dir);
                cx.open_with_system(&dir);
            }
            Err(err) => eprintln!("failed to locate the theme folder: {err}"),
        }
    } else if action.as_any().is::<SaveDocument>() {
        let _ = with_active_editor(cx, |editor, window, cx| editor.save_document(window, cx));
    } else if action.as_any().is::<SaveDocumentAs>() {
        let _ = with_active_editor(cx, |editor, window, cx| editor.save_document_as(window, cx));
    } else if action.as_any().is::<ExportHtml>() {
        let _ = with_active_editor(cx, |editor, window, cx| {
            editor.export_document_via_prompt(ExportFormat::Html, window, cx)
        });
    } else if action.as_any().is::<ExportPdf>() {
        let _ = with_active_editor(cx, |editor, window, cx| {
            editor.export_document_via_prompt(ExportFormat::Pdf, window, cx)
        });
    } else if action.as_any().is::<ToggleAutoPair>() {
        toggle_writing_aid(cx, |writing| writing.auto_pair = !writing.auto_pair);
    } else if action.as_any().is::<crate::components::ToggleToolbar>() {
        toggle_writing_aid(cx, |writing| writing.toolbar = !writing.toolbar);
    } else if action.as_any().is::<crate::components::ToggleAutosave>() {
        toggle_writing_aid(cx, |writing| writing.autosave = !writing.autosave);
    } else if action.as_any().is::<ToggleSmartQuotes>() {
        toggle_writing_aid(cx, |writing| writing.smart_quotes = !writing.smart_quotes);
    } else if action.as_any().is::<ToggleSmartDashes>() {
        toggle_writing_aid(cx, |writing| writing.smart_dashes = !writing.smart_dashes);
    } else if action.as_any().is::<ToggleFocusMode>() {
        toggle_writing_aid(cx, |writing| writing.focus_mode = !writing.focus_mode);
    } else if action.as_any().is::<ToggleTypewriterMode>() {
        toggle_writing_aid(cx, |writing| {
            writing.typewriter_mode = !writing.typewriter_mode
        });
    } else if action.as_any().is::<ToggleStatusBar>() {
        let enabled = crate::config::EditorSettings::status_bar_preferences(cx).enabled;
        crate::config::EditorSettings::set_status_bar_enabled(cx, !enabled);
        install_menus(cx);
        cx.refresh_windows();
    } else if action.as_any().is::<ToggleFullScreen>() {
        if let Some(window) = cx.active_window() {
            let _ = window.update(cx, |_, window, _| window.toggle_fullscreen());
        }
    } else if action.as_any().is::<ToggleAlwaysOnTop>() {
        let on_top = !crate::window_pin::AlwaysOnTop::enabled(cx);
        cx.set_global(crate::window_pin::AlwaysOnTop(on_top));
        for window in cx.windows() {
            let _ = window.update(cx, |_, window, _| {
                crate::window_pin::set_always_on_top(window, on_top)
            });
        }
        install_menus(cx);
    } else if action.as_any().is::<ResetZoom>() {
        set_zoom(cx, 100);
    } else if action.as_any().is::<ZoomIn>() {
        let current = crate::config::EditorSettings::writing(cx).zoom_percent;
        set_zoom(cx, crate::config::step_zoom(current, true));
    } else if action.as_any().is::<ZoomOut>() {
        let current = crate::config::EditorSettings::writing(cx).zoom_percent;
        set_zoom(cx, crate::config::step_zoom(current, false));
    } else if action.as_any().is::<ToggleSpellcheck>() {
        toggle_writing_aid(cx, |writing| writing.spellcheck = !writing.spellcheck);
    } else if let Some(action) = action.as_any().downcast_ref::<SelectSpellcheckLanguage>() {
        let failed_title = cx
            .global::<I18nManager>()
            .strings()
            .spelling_download_failed_title
            .clone();
        crate::spellcheck::SpellChecker::toggle_language(
            cx,
            action.language.clone(),
            move |cx, result| match result {
                Ok(languages) => {
                    let joined = languages.join(",");
                    toggle_writing_aid(cx, move |writing| writing.spellcheck_language = joined);
                }
                Err(err) => {
                    show_window_prompt(cx.active_window(), &failed_title, &err.to_string(), cx)
                }
            },
        );
    } else if action.as_any().is::<ToggleReadOnly>() {
        let _ = with_active_editor(cx, |editor, _window, cx| editor.toggle_read_only(cx));
    } else if action.as_any().is::<ToggleFollowSystemAppearance>() {
        match crate::config::toggle_follow_system_appearance(cx) {
            Ok(()) => {
                install_menus(cx);
                cx.refresh_windows();
            }
            Err(err) => {
                let title = cx
                    .global::<I18nManager>()
                    .strings()
                    .preferences_save_failed_title
                    .clone();
                show_window_prompt(cx.active_window(), &title, &err.to_string(), cx);
            }
        }
    } else if let Some(action) = action.as_any().downcast_ref::<SelectTheme>() {
        match apply_configured_theme(cx, &action.theme_id) {
            Ok(changed) => {
                if changed {
                    install_menus(cx);
                    cx.refresh_windows();
                }
            }
            Err(err) => {
                let title = cx
                    .global::<I18nManager>()
                    .strings()
                    .preferences_save_failed_title
                    .clone();
                show_window_prompt(cx.active_window(), &title, &err.to_string(), cx);
            }
        }
    } else if let Some(action) = action.as_any().downcast_ref::<SelectLanguage>() {
        match apply_configured_language(cx, &action.language_id) {
            Ok(changed) => {
                if changed {
                    install_menus(cx);
                    cx.refresh_windows();
                }
            }
            Err(err) => {
                let title = cx
                    .global::<I18nManager>()
                    .strings()
                    .preferences_save_failed_title
                    .clone();
                show_window_prompt(cx.active_window(), &title, &err.to_string(), cx);
            }
        }
    } else if action.as_any().is::<CheckForUpdates>() {
        request_update_check_on_active_editor(cx);
    } else if action.as_any().is::<ShowAbout>() {
        show_info_dialog_on_active_editor(cx, InfoDialogKind::About);
    } else if action.as_any().is::<InstallCliTool>() {
        install_cli_tool(cx);
    } else if action.as_any().is::<UninstallCliTool>() {
        uninstall_cli_tool(cx);
    } else if action.as_any().is::<ToggleWorkspace>() {
        let _ = with_active_editor(cx, |editor, window, cx| {
            editor.toggle_workspace_drawer(window, cx);
        });
    } else if action.as_any().is::<OpenFolder>() {
        let _ = with_active_editor(cx, |editor, _, cx| editor.prompt_open_folder(cx));
    } else if action.as_any().is::<CloseFolder>() {
        let _ = with_active_editor(cx, |editor, _, cx| editor.apply_workspace_folder(None, cx));
    } else if action.as_any().is::<ReloadFromDisk>() {
        let _ = with_active_editor(cx, |editor, window, cx| editor.reload_from_disk(window, cx));
    } else if action.as_any().is::<SaveAll>() {
        let _ = with_active_editor(cx, |editor, window, cx| editor.save_all(window, cx));
    } else if action.as_any().is::<RevealInFileManager>() {
        let _ = with_active_editor(cx, |editor, _, cx| editor.reveal_in_file_manager(cx));
    } else if action.as_any().is::<RevealInSidebar>() {
        let _ = with_active_editor(cx, |editor, _, cx| editor.reveal_in_sidebar(cx));
    } else if action.as_any().is::<DeleteCurrentFile>() {
        let _ = with_active_editor(cx, |editor, window, cx| {
            editor.delete_current_file(window, cx)
        });
    } else if action.as_any().is::<ReopenClosedTab>() {
        let _ = with_active_editor_window(cx, |host, window, cx| {
            host.reopen_closed_tab(window, cx);
        });
    } else if action.as_any().is::<ClearRecentFiles>() {
        if let Err(err) = crate::config::clear_recent_files() {
            eprintln!("failed to clear recent file history: {err}");
        }
        install_menus(cx);
        cx.refresh_windows();
    } else if action.as_any().is::<QuitApplication>() {
        request_quit_application(cx);
    } else if action.as_any().is::<CloseWindow>() {
        request_close_current_editor_window(cx);
    } else if action.as_any().is::<NewTab>() {
        if with_active_editor_window(cx, |host, window, cx| host.new_tab(window, cx)).is_none() {
            open_markdown_in_current_window(cx, String::new(), None);
        }
    } else if action.as_any().is::<CloseTab>() {
        let _ = with_active_editor_window(cx, |host, window, cx| {
            host.request_close_tab(host.active_index(), window, cx);
        });
    } else if action.as_any().is::<NextTab>() {
        let _ = with_active_editor_window(cx, |host, window, cx| {
            host.activate_next_tab(window, cx);
        });
    } else if action.as_any().is::<PrevTab>() {
        let _ = with_active_editor_window(cx, |host, window, cx| {
            host.activate_previous_tab(window, cx);
        });
    }
}

/// Edit menu: history, clipboard (with the plain-text and HTML variants),
/// selection and find.
fn edit_menu(
    strings: &crate::i18n::I18nStrings,
    view: ViewMenuState,
    writing: &crate::config::WritingPreferences,
) -> Menu {
    use crate::components::{SetLineEndingCrlf, SetLineEndingLf, ToggleFinalNewline};
    let checked = |label: &str, on: bool| {
        if on {
            format!("\u{2713} {label}")
        } else {
            label.to_string()
        }
    };
    use crate::components::{
        Copy, CopyAsHtml, CopyAsPlainText, Cut, FindAndReplace, FindInDocument, Paste,
        PasteAsPlainText, Redo, SearchInWorkspace, SelectAll, Undo,
    };
    use crate::components::{
        DeleteBlock, DeleteLine, DeleteStyledScope, DeleteWord, JumpToBottom, JumpToSelection,
        JumpToTop, SelectBlock, SelectLine, SelectStyledScope, SelectWord,
    };
    Menu {
        name: strings.menu_edit.clone().into(),
        items: vec![
            MenuItem::action(strings.preferences_shortcut_undo.clone(), Undo),
            MenuItem::action(strings.preferences_shortcut_redo.clone(), Redo),
            MenuItem::separator(),
            MenuItem::action(strings.preferences_shortcut_cut.clone(), Cut),
            MenuItem::action(strings.preferences_shortcut_copy.clone(), Copy),
            MenuItem::action(strings.preferences_shortcut_paste.clone(), Paste),
            MenuItem::separator(),
            MenuItem::action(strings.menu_copy_as_plain_text.clone(), CopyAsPlainText),
            MenuItem::action(strings.menu_copy_as_html.clone(), CopyAsHtml),
            MenuItem::action(strings.menu_paste_as_plain_text.clone(), PasteAsPlainText),
            MenuItem::separator(),
            MenuItem::action(strings.preferences_shortcut_select_all.clone(), SelectAll),
            MenuItem::submenu(Menu {
                name: strings.menu_selection.clone().into(),
                items: vec![
                    MenuItem::action(strings.menu_select_block.clone(), SelectBlock),
                    MenuItem::action(strings.menu_select_line.clone(), SelectLine),
                    MenuItem::action(strings.menu_select_styled_scope.clone(), SelectStyledScope),
                    MenuItem::action(strings.menu_select_word.clone(), SelectWord),
                    MenuItem::separator(),
                    MenuItem::action(strings.preferences_shortcut_jump_to_top.clone(), JumpToTop),
                    MenuItem::action(strings.menu_jump_to_selection.clone(), JumpToSelection),
                    MenuItem::action(
                        strings.preferences_shortcut_jump_to_bottom.clone(),
                        JumpToBottom,
                    ),
                ],
            }),
            MenuItem::submenu(Menu {
                name: strings.menu_delete.clone().into(),
                items: vec![
                    MenuItem::action(strings.menu_delete_block.clone(), DeleteBlock),
                    MenuItem::action(strings.menu_delete_line.clone(), DeleteLine),
                    MenuItem::action(strings.menu_delete_styled_scope.clone(), DeleteStyledScope),
                    MenuItem::action(strings.menu_delete_word.clone(), DeleteWord),
                ],
            }),
            MenuItem::separator(),
            MenuItem::submenu(Menu {
                name: strings.menu_smart_punctuation.clone().into(),
                items: vec![
                    MenuItem::action(
                        checked(&strings.menu_smart_quotes, writing.smart_quotes),
                        ToggleSmartQuotes,
                    ),
                    MenuItem::action(
                        checked(&strings.menu_smart_dashes, writing.smart_dashes),
                        ToggleSmartDashes,
                    ),
                ],
            }),
            MenuItem::submenu(Menu {
                name: strings.menu_line_endings.clone().into(),
                items: vec![
                    MenuItem::action(
                        checked(&strings.menu_line_ending_crlf, view.crlf),
                        SetLineEndingCrlf,
                    ),
                    MenuItem::action(
                        checked(&strings.menu_line_ending_lf, !view.crlf),
                        SetLineEndingLf,
                    ),
                    MenuItem::separator(),
                    MenuItem::action(
                        checked(&strings.menu_final_newline, writing.final_newline),
                        ToggleFinalNewline,
                    ),
                ],
            }),
            MenuItem::separator(),
            MenuItem::action(strings.preferences_shortcut_find.clone(), FindInDocument),
            MenuItem::action(
                strings.preferences_shortcut_find_replace.clone(),
                FindAndReplace,
            ),
            MenuItem::action(
                strings.preferences_shortcut_search_workspace.clone(),
                SearchInWorkspace,
            ),
            MenuItem::separator(),
            MenuItem::action(
                strings.menu_emoji_picker.clone(),
                crate::components::OpenEmojiPicker,
            ),
        ],
    }
}

/// Paragraph menu in the layout of Typora.
pub(crate) fn paragraph_menu(strings: &crate::i18n::I18nStrings) -> Menu {
    use crate::components::{
        DecreaseHeadingLevel, IncreaseHeadingLevel, InsertCallout, InsertCodeBlock, InsertFootnote,
        InsertFrontMatter, InsertHorizontalRule, InsertLinkReference, InsertMathBlock, InsertTable,
        InsertTableOfContents, SetHeading1, SetHeading2, SetHeading3, SetHeading4, SetHeading5,
        SetHeading6, SetParagraph, ToggleBulletList, ToggleOrderedList, ToggleQuote,
        ToggleTaskList, ToggleTaskStatus,
    };
    let heading = |level: u8| {
        strings
            .preferences_shortcut_set_heading
            .replace("{level}", &level.to_string())
    };
    let callouts = [
        ("note", &strings.menu_callout_note),
        ("tip", &strings.menu_callout_tip),
        ("important", &strings.menu_callout_important),
        ("warning", &strings.menu_callout_warning),
        ("caution", &strings.menu_callout_caution),
    ];
    Menu {
        name: strings.menu_paragraph.clone().into(),
        items: vec![
            MenuItem::action(heading(1), SetHeading1),
            MenuItem::action(heading(2), SetHeading2),
            MenuItem::action(heading(3), SetHeading3),
            MenuItem::action(heading(4), SetHeading4),
            MenuItem::action(heading(5), SetHeading5),
            MenuItem::action(heading(6), SetHeading6),
            MenuItem::separator(),
            MenuItem::action(
                strings.preferences_shortcut_set_paragraph.clone(),
                SetParagraph,
            ),
            MenuItem::separator(),
            MenuItem::action(strings.menu_heading_increase.clone(), IncreaseHeadingLevel),
            MenuItem::action(strings.menu_heading_decrease.clone(), DecreaseHeadingLevel),
            MenuItem::separator(),
            MenuItem::action(strings.menu_insert_table.clone(), InsertTable),
            MenuItem::action(strings.menu_math_block.clone(), InsertMathBlock),
            MenuItem::action(strings.menu_code_block.clone(), InsertCodeBlock),
            MenuItem::separator(),
            MenuItem::action(strings.menu_quote.clone(), ToggleQuote),
            MenuItem::action(strings.menu_ordered_list.clone(), ToggleOrderedList),
            MenuItem::action(strings.menu_bullet_list.clone(), ToggleBulletList),
            MenuItem::action(strings.menu_task_list.clone(), ToggleTaskList),
            MenuItem::action(strings.menu_task_status.clone(), ToggleTaskStatus),
            MenuItem::submenu(Menu {
                name: strings.menu_callout.clone().into(),
                items: callouts
                    .into_iter()
                    .map(|(kind, label)| {
                        MenuItem::action(
                            label.clone(),
                            InsertCallout {
                                kind: kind.to_string(),
                            },
                        )
                    })
                    .collect(),
            }),
            MenuItem::separator(),
            MenuItem::action(strings.menu_link_reference.clone(), InsertLinkReference),
            MenuItem::action(strings.menu_footnote.clone(), InsertFootnote),
            MenuItem::separator(),
            MenuItem::action(strings.menu_horizontal_rule.clone(), InsertHorizontalRule),
            MenuItem::action(
                strings.menu_table_of_contents.clone(),
                InsertTableOfContents,
            ),
            MenuItem::action(strings.menu_front_matter.clone(), InsertFrontMatter),
        ],
    }
}

/// Format menu in the layout of Typora.
pub(crate) fn format_menu(strings: &crate::i18n::I18nStrings) -> Menu {
    use crate::components::{
        BoldSelection, ClearFormat, CodeSelection, CommentSelection, HighlightSelection,
        InlineMathSelection, InsertLink, ItalicSelection, StrikethroughSelection,
        SubscriptSelection, SuperscriptSelection, UnderlineSelection,
    };
    Menu {
        name: strings.menu_format.clone().into(),
        items: vec![
            MenuItem::action(
                strings.preferences_shortcut_bold_selection.clone(),
                BoldSelection,
            ),
            MenuItem::action(
                strings.preferences_shortcut_italic_selection.clone(),
                ItalicSelection,
            ),
            MenuItem::action(
                strings.preferences_shortcut_underline_selection.clone(),
                UnderlineSelection,
            ),
            MenuItem::action(
                strings.preferences_shortcut_code_selection.clone(),
                CodeSelection,
            ),
            MenuItem::separator(),
            MenuItem::action(strings.menu_inline_math.clone(), InlineMathSelection),
            MenuItem::action(strings.menu_strikethrough.clone(), StrikethroughSelection),
            MenuItem::action(strings.menu_highlight.clone(), HighlightSelection),
            MenuItem::action(strings.menu_superscript.clone(), SuperscriptSelection),
            MenuItem::action(strings.menu_subscript.clone(), SubscriptSelection),
            MenuItem::action(strings.menu_comment.clone(), CommentSelection),
            MenuItem::separator(),
            MenuItem::action(strings.menu_link.clone(), InsertLink),
            MenuItem::separator(),
            MenuItem::action(strings.menu_clear_format.clone(), ClearFormat),
            MenuItem::separator(),
            MenuItem::submenu(Menu {
                name: strings.menu_image.clone().into(),
                items: vec![
                    MenuItem::action(
                        strings.menu_insert_image.clone(),
                        crate::components::InsertImage,
                    ),
                    MenuItem::separator(),
                    MenuItem::action(
                        strings.menu_reveal_image_file.clone(),
                        crate::components::RevealImageFile,
                    ),
                    MenuItem::submenu(Menu {
                        name: strings.menu_zoom_image.clone().into(),
                        items: crate::editor::IMAGE_ZOOM_LEVELS
                            .iter()
                            .map(|percent| {
                                MenuItem::action(
                                    format!("{percent} %"),
                                    crate::components::SetImageZoom { percent: *percent },
                                )
                            })
                            .collect(),
                    }),
                    MenuItem::action(
                        strings.menu_toggle_image_syntax.clone(),
                        crate::components::ToggleImageSyntax,
                    ),
                    MenuItem::separator(),
                    MenuItem::action(
                        strings.menu_copy_image.clone(),
                        crate::components::CopyImage,
                    ),
                    MenuItem::action(
                        strings.menu_copy_image_to.clone(),
                        crate::components::CopyImageTo,
                    ),
                    MenuItem::action(
                        strings.menu_move_image_to.clone(),
                        crate::components::MoveImageTo,
                    ),
                    MenuItem::action(
                        strings.menu_delete_image_file.clone(),
                        crate::components::DeleteImageFile,
                    ),
                    MenuItem::action(
                        strings.menu_set_image_root.clone(),
                        crate::components::SetImageRoot,
                    ),
                    MenuItem::separator(),
                    MenuItem::action(
                        strings.menu_upload_image.clone(),
                        crate::components::UploadImage,
                    ),
                    MenuItem::action(
                        strings.menu_upload_all_images.clone(),
                        crate::components::UploadAllImages,
                    ),
                ],
            }),
        ],
    }
}

/// Paragraph and Format commands act on the focused block, so a menu click
/// re-dispatches them through the window focus path (block, then editor).
/// Help > Quick Start, Markdown Reference, Keyboard Shortcuts.
fn help_document_items(strings: &crate::i18n::I18nStrings) -> Vec<MenuItem> {
    vec![
        MenuItem::action(
            strings.menu_quick_start.clone(),
            crate::components::OpenQuickStart,
        ),
        MenuItem::action(
            strings.menu_markdown_reference.clone(),
            crate::components::OpenMarkdownReference,
        ),
        MenuItem::action(
            strings.menu_keyboard_shortcuts.clone(),
            crate::components::OpenKeyboardShortcuts,
        ),
        MenuItem::separator(),
    ]
}

fn is_document_command_action(action: &dyn Action) -> bool {
    use crate::components::{
        BoldSelection, ClearFormat, CodeSelection, CommentSelection, DecreaseHeadingLevel,
        HighlightSelection, IncreaseHeadingLevel, InlineMathSelection, InsertCallout,
        InsertCodeBlock, InsertFootnote, InsertFrontMatter, InsertHorizontalRule, InsertLink,
        InsertLinkReference, InsertMathBlock, InsertTable, InsertTableOfContents, ItalicSelection,
        SetHeading1, SetHeading2, SetHeading3, SetHeading4, SetHeading5, SetHeading6, SetParagraph,
        StrikethroughSelection, SubscriptSelection, SuperscriptSelection, ToggleBulletList,
        ToggleOrderedList, ToggleQuote, ToggleTaskList, ToggleTaskStatus, UnderlineSelection,
    };
    use crate::components::{
        Copy, CopyAsHtml, CopyAsPlainText, Cut, FindAndReplace, FindInDocument, Paste,
        PasteAsPlainText, Redo, SearchInWorkspace, SelectAll, Undo,
    };
    use crate::components::{
        DeleteBlock, DeleteLine, DeleteStyledScope, DeleteWord, JumpToBottom, JumpToSelection,
        JumpToTop, SelectBlock, SelectLine, SelectStyledScope, SelectWord,
    };
    use crate::components::{SetLineEndingCrlf, SetLineEndingLf, ToggleFinalNewline};
    let any = action.as_any();
    any.is::<crate::components::OpenEmojiPicker>()
        || any.is::<crate::components::ShowFileTree>()
        || any.is::<crate::components::ShowOutline>()
        || any.is::<crate::components::ShowArticles>()
        || any.is::<crate::components::ToggleViewMode>()
        || any.is::<crate::components::ExportWithPandoc>()
        || any.is::<crate::components::ImportWithPandoc>()
        || any.is::<crate::components::PrintDocument>()
        || any.is::<crate::components::ExportAgain>()
        || any.is::<crate::components::SetImageZoom>()
        || any.is::<crate::components::CopyImage>()
        || any.is::<crate::components::SetImageRoot>()
        || any.is::<crate::components::InsertImage>()
        || any.is::<crate::components::RevealImageFile>()
        || any.is::<crate::components::ToggleImageSyntax>()
        || any.is::<crate::components::CopyImageTo>()
        || any.is::<crate::components::MoveImageTo>()
        || any.is::<crate::components::DeleteImageFile>()
        || any.is::<crate::components::MoveDocument>()
        || any.is::<crate::components::ShowProperties>()
        || any.is::<crate::components::ExportHtmlPlain>()
        || any.is::<crate::components::ShowBackups>()
        || any.is::<crate::components::UploadImage>()
        || any.is::<crate::components::UploadAllImages>()
        || any.is::<SetLineEndingCrlf>()
        || any.is::<SetLineEndingLf>()
        || any.is::<ToggleFinalNewline>()
        || any.is::<SelectBlock>()
        || any.is::<SelectLine>()
        || any.is::<SelectStyledScope>()
        || any.is::<SelectWord>()
        || any.is::<JumpToTop>()
        || any.is::<JumpToSelection>()
        || any.is::<JumpToBottom>()
        || any.is::<DeleteBlock>()
        || any.is::<DeleteLine>()
        || any.is::<DeleteStyledScope>()
        || any.is::<DeleteWord>()
        || any.is::<Undo>()
        || any.is::<Redo>()
        || any.is::<Cut>()
        || any.is::<Copy>()
        || any.is::<Paste>()
        || any.is::<CopyAsPlainText>()
        || any.is::<CopyAsHtml>()
        || any.is::<PasteAsPlainText>()
        || any.is::<SelectAll>()
        || any.is::<FindInDocument>()
        || any.is::<FindAndReplace>()
        || any.is::<SearchInWorkspace>()
        || any.is::<SetHeading1>()
        || any.is::<SetHeading2>()
        || any.is::<SetHeading3>()
        || any.is::<SetHeading4>()
        || any.is::<SetHeading5>()
        || any.is::<SetHeading6>()
        || any.is::<SetParagraph>()
        || any.is::<IncreaseHeadingLevel>()
        || any.is::<DecreaseHeadingLevel>()
        || any.is::<InsertTable>()
        || any.is::<InsertMathBlock>()
        || any.is::<InsertCodeBlock>()
        || any.is::<ToggleQuote>()
        || any.is::<ToggleOrderedList>()
        || any.is::<ToggleBulletList>()
        || any.is::<ToggleTaskList>()
        || any.is::<ToggleTaskStatus>()
        || any.is::<InsertCallout>()
        || any.is::<InsertLinkReference>()
        || any.is::<InsertFootnote>()
        || any.is::<InsertHorizontalRule>()
        || any.is::<InsertTableOfContents>()
        || any.is::<InsertFrontMatter>()
        || any.is::<BoldSelection>()
        || any.is::<ItalicSelection>()
        || any.is::<UnderlineSelection>()
        || any.is::<CodeSelection>()
        || any.is::<InlineMathSelection>()
        || any.is::<StrikethroughSelection>()
        || any.is::<HighlightSelection>()
        || any.is::<SuperscriptSelection>()
        || any.is::<SubscriptSelection>()
        || any.is::<CommentSelection>()
        || any.is::<InsertLink>()
        || any.is::<ClearFormat>()
}

/// Executes a menu action against a specific editor when the action is
/// editor-scoped, falling back to app-wide behavior for global actions.
pub(crate) fn dispatch_menu_action_for_editor(
    action: &dyn Action,
    target: &WeakEntity<Editor>,
    window: &mut Window,
    cx: &mut App,
) {
    if is_document_command_action(action) {
        let action = action.boxed_clone();
        window.defer(cx, move |window, cx| window.dispatch_action(action, cx));
        return;
    }
    if !is_window_context_menu_action(action) {
        let deferred_action = action.boxed_clone();
        cx.defer(move |cx| {
            dispatch_menu_action(deferred_action.as_ref(), cx);
        });
        return;
    }

    window.activate_window();
    let current_window = Some(window.window_handle());

    if action.as_any().is::<NewWindow>() {
        open_editor_window(cx, String::new(), None);
    } else if action.as_any().is::<OpenFile>() {
        prompt_and_open_files_with_error_window(cx, current_window);
    } else if action.as_any().is::<OpenPreferences>() {
        open_preferences_window(cx);
    } else if let Some(action) = action.as_any().downcast_ref::<OpenRecentFile>() {
        // The file opens as a tab of this window, which is mid-update while the
        // in-window menu click is dispatched; finish that update first.
        let path = PathBuf::from(&action.path);
        cx.defer(move |cx| open_recent_file_with_error_window(cx, path, current_window));
    } else if action.as_any().is::<NoRecentFiles>() {
    } else if action.as_any().is::<AddLanguageConfig>() {
        prompt_and_import_language_config_with_error_window(cx, current_window);
    } else if action.as_any().is::<AddThemeConfig>() {
        prompt_and_import_theme_config_with_error_window(cx, current_window);
    } else if action.as_any().is::<SaveDocument>() {
        let _ = target.update(cx, |editor, cx| editor.request_save_document(cx));
    } else if action.as_any().is::<SaveDocumentAs>() {
        let _ = target.update(cx, |editor, cx| editor.request_save_document_as(cx));
    } else if action.as_any().is::<ExportHtml>() {
        let _ = target.update(cx, |editor, cx| {
            editor.export_document_via_prompt(ExportFormat::Html, window, cx);
        });
    } else if action.as_any().is::<ExportPdf>() {
        let _ = target.update(cx, |editor, cx| {
            editor.export_document_via_prompt(ExportFormat::Pdf, window, cx);
        });
    } else if action.as_any().is::<QuitApplication>() {
        request_quit_application(cx);
    } else if action.as_any().is::<CloseWindow>() {
        let _ = target.update(cx, |editor, cx| {
            editor.request_close_current_window(window, cx);
        });
    } else if action.as_any().is::<CheckForUpdates>() {
        let _ = target.update(cx, |editor, cx| {
            editor.request_check_updates(window, cx);
        });
    } else if action.as_any().is::<ShowAbout>() {
        let _ = target.update(cx, |editor, cx| {
            editor.show_info_dialog(InfoDialogKind::About, cx)
        });
    } else if action.as_any().is::<InstallCliTool>() {
        install_cli_tool(cx);
    } else if action.as_any().is::<UninstallCliTool>() {
        uninstall_cli_tool(cx);
    } else if action.as_any().is::<ToggleWorkspace>() {
        let _ = target.update(cx, |editor, cx| {
            editor.toggle_workspace_drawer(window, cx);
        });
    } else if action.as_any().is::<OpenFolder>() {
        let _ = target.update(cx, |editor, cx| editor.prompt_open_folder(cx));
    } else if action.as_any().is::<CloseFolder>() {
        let _ = target.update(cx, |editor, cx| editor.apply_workspace_folder(None, cx));
    } else if action.as_any().is::<ReloadFromDisk>() {
        let _ = target.update(cx, |editor, cx| editor.reload_from_disk(window, cx));
    } else if action.as_any().is::<SaveAll>() {
        let _ = target.update(cx, |editor, cx| editor.save_all(window, cx));
    } else if action.as_any().is::<RevealInFileManager>() {
        let _ = target.update(cx, |editor, cx| editor.reveal_in_file_manager(cx));
    } else if action.as_any().is::<RevealInSidebar>() {
        let _ = target.update(cx, |editor, cx| editor.reveal_in_sidebar(cx));
    } else if action.as_any().is::<DeleteCurrentFile>() {
        let _ = target.update(cx, |editor, cx| editor.delete_current_file(window, cx));
    }
}

/// Window and document state shown as check marks in the menus.
#[derive(Clone, Copy)]
struct ViewMenuState {
    status_bar: bool,
    always_on_top: bool,
    /// Line endings of the active document.
    crlf: bool,
    /// The active document shows its Markdown source.
    source_mode: bool,
}

impl Default for ViewMenuState {
    fn default() -> Self {
        Self {
            status_bar: true,
            always_on_top: false,
            crlf: false,
            source_mode: false,
        }
    }
}

#[cfg(test)]
fn build_menus(
    theme_manager: &ThemeManager,
    i18n_manager: &I18nManager,
    recent_files: &[PathBuf],
    writing: crate::config::WritingPreferences,
    active_spell_languages: &[String],
) -> Vec<Menu> {
    build_menus_with_view(
        theme_manager,
        i18n_manager,
        recent_files,
        writing,
        active_spell_languages,
        ViewMenuState::default(),
    )
}

fn build_menus_with_view(
    theme_manager: &ThemeManager,
    i18n_manager: &I18nManager,
    recent_files: &[PathBuf],
    writing: crate::config::WritingPreferences,
    active_spell_languages: &[String],
    view: ViewMenuState,
) -> Vec<Menu> {
    let current_theme_id = theme_manager.current_theme_id().to_string();
    let current_language_id = i18n_manager.current_language_id().to_string();
    let strings = i18n_manager.strings().clone();
    let mut theme_items = theme_manager
        .available_themes()
        .iter()
        .map(|entry| {
            let label = if entry.id.as_str() == current_theme_id {
                format!("\u{2713} {}", entry.name)
            } else {
                entry.name.to_string()
            };
            MenuItem::action(
                label,
                SelectTheme {
                    theme_id: entry.id.to_string(),
                },
            )
        })
        .collect::<Vec<_>>();
    theme_items.push(MenuItem::separator());
    let follow_label = if theme_manager.follows_system_appearance() {
        format!("\u{2713} {}", strings.menu_follow_system_appearance)
    } else {
        strings.menu_follow_system_appearance.clone()
    };
    theme_items.push(MenuItem::action(follow_label, ToggleFollowSystemAppearance));
    theme_items.push(MenuItem::action(
        strings.menu_add_theme_config.clone(),
        AddThemeConfig,
    ));
    theme_items.push(MenuItem::action(
        strings.menu_open_theme_folder.clone(),
        crate::components::OpenThemeFolder,
    ));

    let mut language_items = i18n_manager
        .available_languages()
        .iter()
        .map(|entry| {
            let name = entry.name.to_string();
            let label = if entry.id.as_str() == current_language_id {
                format!("\u{2713} {name}")
            } else {
                name
            };
            MenuItem::action(
                label,
                SelectLanguage {
                    language_id: entry.id.to_string(),
                },
            )
        })
        .collect::<Vec<_>>();
    language_items.push(MenuItem::separator());
    language_items.push(MenuItem::action(
        strings.menu_add_language_config.clone(),
        AddLanguageConfig,
    ));

    let recent_items = if recent_files.is_empty() {
        vec![MenuItem::action(
            strings.menu_no_recent_files.clone(),
            NoRecentFiles,
        )]
    } else {
        recent_files
            .iter()
            .map(|path| {
                // into_owned on a Cow<str> reuses the Cow::Owned variant
                // (no copy) when the OS string is valid UTF-8 — the common
                // case — and only allocates for the lossy fallback. The
                // previous .to_string_lossy().to_string() always allocated.
                let label = path.to_string_lossy().into_owned();
                MenuItem::action(label.clone(), OpenRecentFile { path: label })
            })
            .chain([
                MenuItem::separator(),
                MenuItem::action(strings.menu_clear_recent.clone(), ClearRecentFiles),
            ])
            .collect()
    };

    // Built before the File menu moves strings out.
    // Built before the File menu moves strings out of `strings`.
    let help_documents = help_document_items(&strings);
    let document_menus = [
        edit_menu(&strings, view, &writing),
        paragraph_menu(&strings),
        format_menu(&strings),
    ];
    #[cfg(target_os = "macos")]
    let initial_menus = {
        // On macOS, the first menu is the app menu (macOS overrides its title
        // with the app name). File operations go in a separate "File" menu to
        // match standard macOS conventions.
        vec![
            Menu {
                name: "Velotype".into(),
                items: vec![
                    MenuItem::action(strings.menu_preferences.clone(), OpenPreferences),
                    MenuItem::separator(),
                    MenuItem::action(strings.menu_quit.clone(), QuitApplication),
                ],
            },
            Menu {
                name: strings.menu_file.into(),
                items: vec![
                    MenuItem::action(strings.menu_new_window.clone(), NewWindow),
                    MenuItem::action(strings.menu_close_window.clone(), CloseWindow),
                    MenuItem::action(strings.menu_open_file.clone(), OpenFile),
                    MenuItem::action(strings.menu_open_folder.clone(), OpenFolder),
                    MenuItem::action(strings.menu_close_folder.clone(), CloseFolder),
                    MenuItem::submenu(Menu {
                        name: strings.menu_open_recent_file.clone().into(),
                        items: recent_items,
                    }),
                    MenuItem::separator(),
                    MenuItem::action(strings.menu_new_tab.clone(), NewTab),
                    MenuItem::action(strings.menu_close_tab.clone(), CloseTab),
                    MenuItem::action(strings.menu_reopen_closed_tab.clone(), ReopenClosedTab),
                    MenuItem::separator(),
                    MenuItem::action(strings.menu_save.clone(), SaveDocument),
                    MenuItem::action(strings.menu_save_as.clone(), SaveDocumentAs),
                    MenuItem::action(strings.menu_save_all.clone(), SaveAll),
                    MenuItem::action(
                        strings.menu_move_document.clone(),
                        crate::components::MoveDocument,
                    ),
                    MenuItem::action(
                        if writing.autosave {
                            format!("✓ {}", strings.menu_autosave)
                        } else {
                            strings.menu_autosave.clone()
                        },
                        crate::components::ToggleAutosave,
                    ),
                    MenuItem::separator(),
                    MenuItem::action(strings.menu_reload_from_disk.clone(), ReloadFromDisk),
                    MenuItem::action(
                        strings.menu_show_backups.clone(),
                        crate::components::ShowBackups,
                    ),
                    MenuItem::action(
                        strings.menu_reveal_in_file_manager.clone(),
                        RevealInFileManager,
                    ),
                    MenuItem::action(strings.menu_reveal_in_sidebar.clone(), RevealInSidebar),
                    MenuItem::action(strings.menu_delete_file.clone(), DeleteCurrentFile),
                    MenuItem::action(
                        strings.menu_properties.clone(),
                        crate::components::ShowProperties,
                    ),
                    MenuItem::separator(),
                    MenuItem::action(
                        strings.menu_import.clone(),
                        crate::components::ImportWithPandoc,
                    ),
                    MenuItem::action(strings.menu_print.clone(), crate::components::PrintDocument),
                ],
            },
        ]
    };

    #[cfg(not(target_os = "macos"))]
    let initial_menus = {
        vec![Menu {
            name: strings.menu_file.into(),
            items: vec![
                MenuItem::action(strings.menu_new_window.clone(), NewWindow),
                MenuItem::action(strings.menu_close_window.clone(), CloseWindow),
                MenuItem::action(strings.menu_open_file.clone(), OpenFile),
                MenuItem::action(strings.menu_open_folder.clone(), OpenFolder),
                MenuItem::action(strings.menu_close_folder.clone(), CloseFolder),
                MenuItem::submenu(Menu {
                    name: strings.menu_open_recent_file.clone().into(),
                    items: recent_items,
                }),
                MenuItem::action(strings.menu_preferences.clone(), OpenPreferences),
                MenuItem::separator(),
                MenuItem::action(strings.menu_new_tab.clone(), NewTab),
                MenuItem::action(strings.menu_close_tab.clone(), CloseTab),
                MenuItem::action(strings.menu_reopen_closed_tab.clone(), ReopenClosedTab),
                MenuItem::separator(),
                MenuItem::action(strings.menu_save.clone(), SaveDocument),
                MenuItem::action(strings.menu_save_as.clone(), SaveDocumentAs),
                MenuItem::action(strings.menu_save_all.clone(), SaveAll),
                MenuItem::action(
                    strings.menu_move_document.clone(),
                    crate::components::MoveDocument,
                ),
                MenuItem::action(
                    if writing.autosave {
                        format!("✓ {}", strings.menu_autosave)
                    } else {
                        strings.menu_autosave.clone()
                    },
                    crate::components::ToggleAutosave,
                ),
                MenuItem::separator(),
                MenuItem::action(strings.menu_reload_from_disk.clone(), ReloadFromDisk),
                MenuItem::action(
                    strings.menu_show_backups.clone(),
                    crate::components::ShowBackups,
                ),
                MenuItem::action(
                    strings.menu_reveal_in_file_manager.clone(),
                    RevealInFileManager,
                ),
                MenuItem::action(strings.menu_reveal_in_sidebar.clone(), RevealInSidebar),
                MenuItem::action(strings.menu_delete_file.clone(), DeleteCurrentFile),
                MenuItem::action(
                    strings.menu_properties.clone(),
                    crate::components::ShowProperties,
                ),
                MenuItem::separator(),
                MenuItem::action(
                    strings.menu_import.clone(),
                    crate::components::ImportWithPandoc,
                ),
                MenuItem::action(strings.menu_print.clone(), crate::components::PrintDocument),
                MenuItem::separator(),
                MenuItem::action(strings.menu_quit.clone(), QuitApplication),
            ],
        }]
    };

    #[cfg(target_os = "macos")]
    let help_items = {
        // Show different menu item depending on whether CLI is already
        // installed pointing to the current app.  Only portable
        // installations (drag-installed .app bundles) need this —
        // pkg-installed apps manage the symlink via postinstall.
        let cli_installed = is_cli_symlink_current_app();
        let mut items = help_documents;
        items.extend([
            MenuItem::action(strings.menu_check_updates.clone(), CheckForUpdates),
            MenuItem::separator(),
        ]);
        if cli_installed {
            items.push(MenuItem::action(
                SharedString::new(strings.menu_uninstall_cli_tool.as_str()),
                UninstallCliTool,
            ));
        } else {
            items.push(MenuItem::action(
                SharedString::new(strings.menu_install_cli_tool.as_str()),
                InstallCliTool,
            ));
        }
        items.push(MenuItem::separator());
        items.push(MenuItem::action(strings.menu_about.clone(), ShowAbout));
        items
    };
    #[cfg(not(target_os = "macos"))]
    let help_items = help_documents
        .into_iter()
        .chain([
            MenuItem::action(strings.menu_check_updates.clone(), CheckForUpdates),
            MenuItem::separator(),
            MenuItem::action(strings.menu_about.clone(), ShowAbout),
        ])
        .collect::<Vec<_>>();

    let mut menus = initial_menus;
    menus.extend(document_menus);
    menus.extend([
        Menu {
            name: strings.menu_export.into(),
            items: vec![
                MenuItem::action(strings.menu_export_html.clone(), ExportHtml),
                MenuItem::action(strings.menu_export_pdf.clone(), ExportPdf),
                MenuItem::action(
                    strings.menu_export_html_plain.clone(),
                    crate::components::ExportHtmlPlain,
                ),
                MenuItem::separator(),
            ]
            .into_iter()
            .chain(crate::export::pandoc::EXPORT_FORMATS.iter().map(|format| {
                MenuItem::action(
                    format.label,
                    crate::components::ExportWithPandoc {
                        format: format.id.to_string(),
                    },
                )
            }))
            .chain([
                MenuItem::separator(),
                MenuItem::action(
                    strings.menu_export_again.clone(),
                    crate::components::ExportAgain,
                ),
            ])
            .collect(),
        },
        Menu {
            name: strings.menu_language.into(),
            items: language_items,
        },
        Menu {
            name: strings.menu_theme.into(),
            items: theme_items,
        },
        Menu {
            name: strings.menu_workspace.into(),
            items: vec![MenuItem::action(
                strings.menu_toggle_workspace.clone(),
                ToggleWorkspace,
            )],
        },
        Menu {
            name: strings.menu_view.clone().into(),
            items: {
                let checked = |label: &str, on: bool| {
                    if on {
                        format!("\u{2713} {label}")
                    } else {
                        label.to_string()
                    }
                };
                vec![
                    MenuItem::action(
                        checked(&strings.menu_source_mode, view.source_mode),
                        crate::components::ToggleViewMode,
                    ),
                    MenuItem::separator(),
                    MenuItem::action(
                        strings.workspace_tab_outline.clone(),
                        crate::components::ShowOutline,
                    ),
                    MenuItem::action(
                        strings.workspace_tab_articles.clone(),
                        crate::components::ShowArticles,
                    ),
                    MenuItem::action(
                        strings.workspace_tab_files.clone(),
                        crate::components::ShowFileTree,
                    ),
                    MenuItem::separator(),
                    MenuItem::action(
                        checked(&strings.menu_focus_mode, writing.focus_mode),
                        ToggleFocusMode,
                    ),
                    MenuItem::action(
                        checked(&strings.menu_typewriter_mode, writing.typewriter_mode),
                        ToggleTypewriterMode,
                    ),
                    MenuItem::action(
                        checked(&strings.menu_auto_pair, writing.auto_pair),
                        ToggleAutoPair,
                    ),
                    MenuItem::action(
                        checked(&strings.menu_status_bar, view.status_bar),
                        ToggleStatusBar,
                    ),
                    MenuItem::action(
                        checked(&strings.menu_toolbar, writing.toolbar),
                        crate::components::ToggleToolbar,
                    ),
                    MenuItem::separator(),
                    MenuItem::action(strings.menu_toggle_full_screen.clone(), ToggleFullScreen),
                ]
                .into_iter()
                .chain(crate::window_pin::SUPPORTED.then(|| {
                    MenuItem::action(
                        checked(&strings.menu_always_on_top, view.always_on_top),
                        ToggleAlwaysOnTop,
                    )
                }))
                .chain([
                    MenuItem::separator(),
                    MenuItem::action(
                        checked(
                            &format!("{} ({} %)", strings.menu_actual_size, writing.zoom_percent),
                            writing.zoom_percent == 100,
                        ),
                        ResetZoom,
                    ),
                    MenuItem::action(strings.menu_zoom_in.clone(), ZoomIn),
                    MenuItem::action(strings.menu_zoom_out.clone(), ZoomOut),
                    MenuItem::separator(),
                    MenuItem::action(
                        checked(&strings.menu_spellcheck, writing.spellcheck),
                        ToggleSpellcheck,
                    ),
                    MenuItem::submenu(Menu {
                        name: strings.menu_spellcheck_language.clone().into(),
                        // Every downloadable dictionary; ✓ marks active ones and ⤓
                        // ones that are downloaded when first picked.
                        items: crate::spellcheck::hunspell::KNOWN_DICTIONARIES
                            .iter()
                            .map(|(tag, _, name)| {
                                let active = active_spell_languages.iter().any(|lang| lang == tag);
                                let label = if active {
                                    format!("\u{2713} {name}")
                                } else if crate::spellcheck::hunspell::is_installed(tag) {
                                    name.to_string()
                                } else {
                                    format!("\u{2913} {name}")
                                };
                                MenuItem::action(
                                    label,
                                    SelectSpellcheckLanguage {
                                        language: tag.to_string(),
                                    },
                                )
                            })
                            .collect(),
                    }),
                    MenuItem::separator(),
                    MenuItem::action(strings.menu_read_only.clone(), ToggleReadOnly),
                ])
                .collect()
            },
        },
        Menu {
            name: strings.menu_help.into(),
            items: help_items,
        },
    ]);
    menus
}

pub(crate) fn install_menus(cx: &mut App) {
    let recent_files = recent_files_for_menu();
    let view = ViewMenuState {
        status_bar: crate::config::EditorSettings::status_bar_preferences(cx).enabled,
        always_on_top: crate::window_pin::AlwaysOnTop::enabled(cx),
        crlf: crate::editor::menu_line_ending_is_crlf(cx),
        source_mode: crate::editor::menu_source_mode(cx),
    };
    let menus = build_menus_with_view(
        cx.global::<ThemeManager>(),
        cx.global::<I18nManager>(),
        &recent_files,
        crate::config::EditorSettings::writing(cx),
        &crate::spellcheck::SpellChecker::languages(cx),
        view,
    );
    cx.set_menus(menus);
}

fn prompt_and_open_files(cx: &mut App) {
    let error_window = cx.active_window();
    prompt_and_open_files_with_error_window(cx, error_window);
}

fn prompt_and_open_files_with_error_window(cx: &mut App, error_window: Option<AnyWindowHandle>) {
    let prompt_title = cx
        .global::<I18nManager>()
        .strings()
        .open_markdown_files_prompt
        .clone();
    crate::perf_log::step("Öffnen-Dialog angefordert");
    let prompt = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: true,
        prompt: Some(prompt_title.into()),
    });
    crate::perf_log::step("Öffnen-Dialog offen");

    cx.spawn(async move |cx| match prompt.await {
        Ok(Ok(Some(paths))) => {
            crate::perf_log::step(format!(
                "Öffnen-Dialog geschlossen: {} Datei(en)",
                paths.len()
            ));
            let updated = cx.update(move |cx| {
                for path in paths {
                    crate::perf_log::step(format!("Datei öffnen: {}", path.display()));
                    if let Err(err) = open_file_in_tab(cx, &path) {
                        crate::perf_log::note(format!("  Fehler: {err:#}"));
                        let title = cx
                            .global::<I18nManager>()
                            .strings()
                            .open_failed_title
                            .clone();
                        let detail = open_error_detail(&err, &path, cx);
                        show_window_prompt(error_window, &title, &detail, cx);
                    } else {
                        crate::perf_log::note("  geöffnet");
                    }
                }
            });
            if let Err(err) = updated {
                crate::perf_log::note(format!("  App-Update fehlgeschlagen: {err:#}"));
            }
        }
        Ok(Err(err)) => {
            let detail = err.to_string();
            let _ = cx.update(move |cx| {
                let title = cx
                    .global::<I18nManager>()
                    .strings()
                    .open_failed_title
                    .clone();
                show_window_prompt(error_window, &title, &detail, cx);
            });
        }
        Ok(Ok(None)) => crate::perf_log::note("Öffnen-Dialog abgebrochen"),
        Err(_) => crate::perf_log::note("Öffnen-Dialog: keine Antwort (Kanal geschlossen)"),
    })
    .detach();
}

fn prompt_and_import_language_config(cx: &mut App) {
    let error_window = cx.active_window();
    prompt_and_import_language_config_with_error_window(cx, error_window);
}

fn prompt_and_import_language_config_with_error_window(
    cx: &mut App,
    error_window: Option<AnyWindowHandle>,
) {
    let prompt_title = cx
        .global::<I18nManager>()
        .strings()
        .add_language_config_prompt
        .clone();
    let prompt = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: Some(prompt_title.into()),
    });

    cx.spawn(async move |cx| match prompt.await {
        Ok(Ok(Some(paths))) => {
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = cx.update(move |cx| {
                let result = import_language_config_and_select(cx, &path);
                match result {
                    Ok(_) => {
                        install_menus(cx);
                        cx.refresh_windows();
                    }
                    Err(err) => {
                        let title = cx
                            .global::<I18nManager>()
                            .strings()
                            .config_import_failed_title
                            .clone();
                        show_window_prompt(error_window, &title, &err.to_string(), cx);
                    }
                }
            });
        }
        Ok(Err(err)) => {
            let detail = err.to_string();
            let _ = cx.update(move |cx| {
                let title = cx
                    .global::<I18nManager>()
                    .strings()
                    .config_import_failed_title
                    .clone();
                show_window_prompt(error_window, &title, &detail, cx);
            });
        }
        Ok(Ok(None)) | Err(_) => {}
    })
    .detach();
}

fn prompt_and_import_theme_config(cx: &mut App) {
    let error_window = cx.active_window();
    prompt_and_import_theme_config_with_error_window(cx, error_window);
}

fn prompt_and_import_theme_config_with_error_window(
    cx: &mut App,
    error_window: Option<AnyWindowHandle>,
) {
    let prompt_title = cx
        .global::<I18nManager>()
        .strings()
        .add_theme_config_prompt
        .clone();
    let prompt = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: Some(prompt_title.into()),
    });

    cx.spawn(async move |cx| match prompt.await {
        Ok(Ok(Some(paths))) => {
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = cx.update(move |cx| {
                let result = import_theme_config_and_select(cx, &path);
                match result {
                    Ok(_) => {
                        install_menus(cx);
                        cx.refresh_windows();
                    }
                    Err(err) => {
                        let title = cx
                            .global::<I18nManager>()
                            .strings()
                            .config_import_failed_title
                            .clone();
                        show_window_prompt(error_window, &title, &err.to_string(), cx);
                    }
                }
            });
        }
        Ok(Err(err)) => {
            let detail = err.to_string();
            let _ = cx.update(move |cx| {
                let title = cx
                    .global::<I18nManager>()
                    .strings()
                    .config_import_failed_title
                    .clone();
                show_window_prompt(error_window, &title, &detail, cx);
            });
        }
        Ok(Ok(None)) | Err(_) => {}
    })
    .detach();
}

fn handle_window_closed(cx: &mut App) {
    if cx.windows().is_empty() {
        cx.quit();
    }
}

/// Installs menu state, action handlers, and the native menu bar.
pub(crate) fn init(cx: &mut App) {
    cx.set_global(AppMenuState::default());
    let subscription = cx.on_window_closed(handle_window_closed);
    cx.global_mut::<AppMenuState>().window_closed_subscription = Some(subscription);

    cx.on_action(|_: &NewWindow, cx| {
        dispatch_menu_action(&NewWindow, cx);
    });
    cx.on_action(|_: &OpenFile, cx| {
        dispatch_menu_action(&OpenFile, cx);
    });
    cx.on_action(|_: &OpenPreferences, cx| {
        dispatch_menu_action(&OpenPreferences, cx);
    });
    cx.on_action(|action: &OpenRecentFile, cx| {
        dispatch_menu_action(action, cx);
    });
    cx.on_action(|_: &NoRecentFiles, cx| {
        dispatch_menu_action(&NoRecentFiles, cx);
    });
    cx.on_action(|_: &AddLanguageConfig, cx| {
        dispatch_menu_action(&AddLanguageConfig, cx);
    });
    cx.on_action(|_: &AddThemeConfig, cx| {
        dispatch_menu_action(&AddThemeConfig, cx);
    });
    cx.on_action(|_: &SaveDocument, cx| {
        dispatch_menu_action(&SaveDocument, cx);
    });
    cx.on_action(|_: &SaveDocumentAs, cx| {
        dispatch_menu_action(&SaveDocumentAs, cx);
    });
    cx.on_action(|_: &ExportHtml, cx| {
        dispatch_menu_action(&ExportHtml, cx);
    });
    cx.on_action(|_: &ExportPdf, cx| {
        dispatch_menu_action(&ExportPdf, cx);
    });
    cx.on_action(|action: &SelectTheme, cx| {
        dispatch_menu_action(action, cx);
    });
    cx.on_action(|_: &ToggleFollowSystemAppearance, cx| {
        dispatch_menu_action(&ToggleFollowSystemAppearance, cx);
    });
    cx.on_action(|action: &SelectLanguage, cx| {
        dispatch_menu_action(action, cx);
    });
    cx.on_action(|_: &CheckForUpdates, cx| {
        dispatch_menu_action(&CheckForUpdates, cx);
    });
    cx.on_action(|_: &ShowAbout, cx| {
        dispatch_menu_action(&ShowAbout, cx);
    });
    cx.on_action(|_: &ToggleWorkspace, cx| {
        dispatch_menu_action(&ToggleWorkspace, cx);
    });
    cx.on_action(|_: &OpenFolder, cx| dispatch_menu_action(&OpenFolder, cx));
    cx.on_action(|_: &ReloadFromDisk, cx| dispatch_menu_action(&ReloadFromDisk, cx));
    cx.on_action(|_: &SaveAll, cx| dispatch_menu_action(&SaveAll, cx));
    cx.on_action(|_: &ReopenClosedTab, cx| dispatch_menu_action(&ReopenClosedTab, cx));
    cx.on_action(|_: &RevealInFileManager, cx| dispatch_menu_action(&RevealInFileManager, cx));
    cx.on_action(|_: &RevealInSidebar, cx| dispatch_menu_action(&RevealInSidebar, cx));
    cx.on_action(|_: &DeleteCurrentFile, cx| dispatch_menu_action(&DeleteCurrentFile, cx));
    cx.on_action(|_: &ClearRecentFiles, cx| dispatch_menu_action(&ClearRecentFiles, cx));
    cx.on_action(|_: &CloseFolder, cx| dispatch_menu_action(&CloseFolder, cx));
    cx.on_action(|_: &ToggleAutoPair, cx| dispatch_menu_action(&ToggleAutoPair, cx));
    cx.on_action(|_: &crate::components::ToggleAutosave, cx| {
        dispatch_menu_action(&crate::components::ToggleAutosave, cx)
    });
    cx.on_action(|_: &crate::components::ToggleToolbar, cx| {
        dispatch_menu_action(&crate::components::ToggleToolbar, cx)
    });
    cx.on_action(|_: &crate::components::OpenThemeFolder, cx| {
        dispatch_menu_action(&crate::components::OpenThemeFolder, cx)
    });
    cx.on_action(|_: &crate::components::OpenQuickStart, cx| {
        dispatch_menu_action(&crate::components::OpenQuickStart, cx)
    });
    cx.on_action(|_: &crate::components::OpenMarkdownReference, cx| {
        dispatch_menu_action(&crate::components::OpenMarkdownReference, cx)
    });
    cx.on_action(|_: &crate::components::OpenKeyboardShortcuts, cx| {
        dispatch_menu_action(&crate::components::OpenKeyboardShortcuts, cx)
    });
    cx.on_action(|_: &ToggleSmartQuotes, cx| dispatch_menu_action(&ToggleSmartQuotes, cx));
    cx.on_action(|_: &ToggleSmartDashes, cx| dispatch_menu_action(&ToggleSmartDashes, cx));
    cx.on_action(|_: &ToggleFocusMode, cx| dispatch_menu_action(&ToggleFocusMode, cx));
    cx.on_action(|_: &ToggleTypewriterMode, cx| dispatch_menu_action(&ToggleTypewriterMode, cx));
    cx.on_action(|_: &ToggleReadOnly, cx| dispatch_menu_action(&ToggleReadOnly, cx));
    cx.on_action(|_: &ToggleSpellcheck, cx| dispatch_menu_action(&ToggleSpellcheck, cx));
    cx.on_action(|_: &ToggleStatusBar, cx| dispatch_menu_action(&ToggleStatusBar, cx));
    cx.on_action(|_: &ToggleFullScreen, cx| dispatch_menu_action(&ToggleFullScreen, cx));
    cx.on_action(|_: &ToggleAlwaysOnTop, cx| dispatch_menu_action(&ToggleAlwaysOnTop, cx));
    cx.on_action(|_: &ResetZoom, cx| dispatch_menu_action(&ResetZoom, cx));
    cx.on_action(|_: &ZoomIn, cx| dispatch_menu_action(&ZoomIn, cx));
    cx.on_action(|_: &ZoomOut, cx| dispatch_menu_action(&ZoomOut, cx));
    cx.on_action(|action: &SelectSpellcheckLanguage, cx| dispatch_menu_action(action, cx));
    cx.on_action(|_: &QuitApplication, cx| {
        dispatch_menu_action(&QuitApplication, cx);
    });
    cx.on_action(|_: &CloseWindow, cx| {
        dispatch_menu_action(&CloseWindow, cx);
    });
    cx.on_action(|_: &NewTab, cx| {
        dispatch_menu_action(&NewTab, cx);
    });
    cx.on_action(|_: &CloseTab, cx| {
        dispatch_menu_action(&CloseTab, cx);
    });
    cx.on_action(|_: &NextTab, cx| {
        dispatch_menu_action(&NextTab, cx);
    });
    cx.on_action(|_: &PrevTab, cx| {
        dispatch_menu_action(&PrevTab, cx);
    });

    install_menus(cx);
    cx.activate(true);
}

#[cfg(test)]
mod tests {
    use super::{applescript_string_literal, build_menus};
    use crate::components::{
        AddLanguageConfig, AddThemeConfig, CheckForUpdates, CloseWindow, ExportHtml, ExportPdf,
        NewWindow, NoRecentFiles, OpenFile, OpenPreferences, OpenRecentFile, QuitApplication,
        SaveDocument, SelectLanguage, SelectTheme, ShowAbout, ToggleFollowSystemAppearance,
    };
    use crate::i18n::I18nManager;
    use crate::theme::ThemeManager;
    use gpui::MenuItem;
    use std::path::PathBuf;

    fn action_name(item: &MenuItem) -> &str {
        match item {
            MenuItem::Action { name, .. } => name.as_ref(),
            _ => panic!("expected action menu item"),
        }
    }

    fn submenu(item: &MenuItem) -> &gpui::Menu {
        match item {
            MenuItem::Submenu(menu) => menu,
            _ => panic!("expected submenu item"),
        }
    }

    #[test]
    fn applescript_string_literal_escapes_special_characters() {
        assert_eq!(
            applescript_string_literal(
                r#"/Applications/Velotype "Test".app/Contents/MacOS/velotype"#
            ),
            r#""/Applications/Velotype \"Test\".app/Contents/MacOS/velotype""#
        );
        assert_eq!(
            applescript_string_literal(r#"/Applications/O'Brien\Velotype.app"#),
            r#""/Applications/O'Brien\\Velotype.app""#
        );
    }

    // On macOS the menu bar is: [Velotype app menu, File, Export, Language, Theme, Workspace, Help]
    // On other platforms:       [File, Export, Language, Theme, Workspace, Help]
    #[cfg(target_os = "macos")]
    const EXPORT_IDX: usize = 5;
    #[cfg(not(target_os = "macos"))]
    const EXPORT_IDX: usize = 4;

    #[cfg(target_os = "macos")]
    const LANGUAGE_IDX: usize = 6;
    #[cfg(not(target_os = "macos"))]
    const LANGUAGE_IDX: usize = 5;

    #[cfg(target_os = "macos")]
    const THEME_IDX: usize = 7;
    #[cfg(not(target_os = "macos"))]
    const THEME_IDX: usize = 6;

    #[cfg(target_os = "macos")]
    const WORKSPACE_IDX: usize = 8;
    #[cfg(not(target_os = "macos"))]
    const WORKSPACE_IDX: usize = 7;

    #[cfg(target_os = "macos")]
    const HELP_IDX: usize = 10;
    #[cfg(not(target_os = "macos"))]
    const HELP_IDX: usize = 9;

    #[test]
    fn view_menu_has_status_bar_full_screen_and_zoom() {
        let theme_manager = ThemeManager::default();
        let i18n_manager = I18nManager::default();
        let writing = crate::config::WritingPreferences {
            zoom_percent: 125,
            ..Default::default()
        };
        let view = super::ViewMenuState {
            status_bar: false,
            always_on_top: true,
            crlf: true,
            source_mode: true,
        };
        let menus =
            super::build_menus_with_view(&theme_manager, &i18n_manager, &[], writing, &[], view);
        let view_menu = menus
            .iter()
            .find(|menu| menu.name.as_ref() == "View")
            .expect("view menu");
        let names = view_menu
            .items
            .iter()
            .filter_map(|item| match item {
                MenuItem::Action { name, .. } => Some(name.to_string()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(names.contains(&"✓ Source Code Mode".to_string()));
        assert!(names.contains(&"Status Bar".to_string()));
        assert!(names.contains(&"Toggle Fullscreen".to_string()));
        assert!(names.contains(&"Actual Size (125 %)".to_string()));
        assert!(names.contains(&"Zoom In".to_string()));
        assert!(names.contains(&"Zoom Out".to_string()));
        assert_eq!(
            names.contains(&"\u{2713} Always on Top".to_string()),
            crate::window_pin::SUPPORTED
        );

        let edit_menu = menus
            .iter()
            .find(|menu| menu.name.as_ref() == "Edit")
            .expect("edit menu");
        let line_endings = edit_menu
            .items
            .iter()
            .find_map(|item| match item {
                MenuItem::Submenu(menu) if menu.name.as_ref() == "Line Endings" => Some(menu),
                _ => None,
            })
            .expect("line endings submenu");
        let names = line_endings
            .items
            .iter()
            .filter_map(|item| match item {
                MenuItem::Action { name, .. } => Some(name.to_string()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                "\u{2713} Windows (CRLF)",
                "Unix (LF)",
                "Insert Final Newline on Save"
            ]
        );
    }

    #[test]
    fn build_menus_uses_english_fallback_by_default() {
        let theme_manager = ThemeManager::default();
        let i18n_manager = I18nManager::default();
        let menus = build_menus(&theme_manager, &i18n_manager, &[], Default::default(), &[]);

        let menu_names = menus
            .iter()
            .map(|menu| menu.name.to_string())
            .collect::<Vec<_>>();

        #[cfg(target_os = "macos")]
        assert_eq!(
            menu_names,
            vec![
                "Velotype",
                "File",
                "Edit",
                "Paragraph",
                "Format",
                "Export",
                "Language",
                "Theme",
                "Workspace",
                "View",
                "Help"
            ]
        );
        #[cfg(not(target_os = "macos"))]
        assert_eq!(
            menu_names,
            vec![
                "File",
                "Edit",
                "Paragraph",
                "Format",
                "Export",
                "Language",
                "Theme",
                "Workspace",
                "View",
                "Help"
            ]
        );

        // New Window belongs with file operations on macOS and remains the
        // first File menu item on other platforms.
        #[cfg(target_os = "macos")]
        assert_eq!(action_name(&menus[1].items[0]), "New Window");
        #[cfg(not(target_os = "macos"))]
        assert_eq!(action_name(&menus[0].items[0]), "New Window");

        // Open Recent File submenu location differs by platform.
        #[cfg(target_os = "macos")]
        assert_eq!(
            submenu(&menus[1].items[5]).name.to_string(),
            "Open Recent File"
        );
        #[cfg(not(target_os = "macos"))]
        assert_eq!(
            submenu(&menus[0].items[5]).name.to_string(),
            "Open Recent File"
        );

        // Close Window is colocated with New Window in the File menu.
        #[cfg(target_os = "macos")]
        assert_eq!(action_name(&menus[1].items[1]), "Close Window");
        #[cfg(not(target_os = "macos"))]
        assert_eq!(action_name(&menus[0].items[1]), "Close Window");

        // Preferences location differs by platform.
        #[cfg(target_os = "macos")]
        assert_eq!(action_name(&menus[0].items[0]), "Preferences");
        #[cfg(not(target_os = "macos"))]
        assert_eq!(action_name(&menus[0].items[6]), "Preferences");

        assert_eq!(action_name(&menus[EXPORT_IDX].items[0]), "HTML");
        assert_eq!(action_name(&menus[EXPORT_IDX].items[1]), "PDF");
        assert_eq!(action_name(&menus[LANGUAGE_IDX].items[0]), "简体中文");
        assert_eq!(
            action_name(&menus[LANGUAGE_IDX].items[1]),
            "\u{2713} English"
        );
        assert_eq!(
            action_name(&menus[WORKSPACE_IDX].items[0]),
            "Toggle Workspace"
        );
    }

    #[test]
    fn build_menus_uses_chinese_language_when_selected() {
        let theme_manager = ThemeManager::default();
        let i18n_manager = I18nManager::new_with_language_id("zh-CN");
        let menus = build_menus(&theme_manager, &i18n_manager, &[], Default::default(), &[]);

        #[cfg(target_os = "macos")]
        assert_eq!(
            submenu(&menus[1].items[5]).name.to_string(),
            i18n_manager.strings().menu_open_recent_file.as_str()
        );
        #[cfg(not(target_os = "macos"))]
        assert_eq!(
            submenu(&menus[0].items[5]).name.to_string(),
            i18n_manager.strings().menu_open_recent_file.as_str()
        );

        let menu_names = menus
            .iter()
            .map(|menu| menu.name.to_string())
            .collect::<Vec<_>>();

        #[cfg(target_os = "macos")]
        assert_eq!(
            menu_names,
            vec![
                "Velotype",
                "文件",
                "编辑",
                "段落",
                "格式",
                "导出",
                "语言",
                "主题",
                "工作区",
                "视图",
                "帮助"
            ]
        );
        #[cfg(not(target_os = "macos"))]
        assert_eq!(
            menu_names,
            vec![
                "文件",
                "编辑",
                "段落",
                "格式",
                "导出",
                "语言",
                "主题",
                "工作区",
                "视图",
                "帮助"
            ]
        );

        #[cfg(target_os = "macos")]
        assert_eq!(action_name(&menus[1].items[0]), "新建窗口");
        #[cfg(not(target_os = "macos"))]
        assert_eq!(action_name(&menus[0].items[0]), "新建窗口");
        assert_eq!(action_name(&menus[EXPORT_IDX].items[0]), "HTML");
        assert_eq!(action_name(&menus[EXPORT_IDX].items[1]), "PDF");
        assert_eq!(
            action_name(&menus[LANGUAGE_IDX].items[0]),
            "\u{2713} 简体中文"
        );
        assert_eq!(action_name(&menus[LANGUAGE_IDX].items[1]), "English");
        assert_eq!(action_name(&menus[WORKSPACE_IDX].items[0]), "切换工作区");
    }

    #[test]
    fn export_menu_items_dispatch_export_actions() {
        let theme_manager = ThemeManager::default();
        let i18n_manager = I18nManager::default();
        let menus = build_menus(&theme_manager, &i18n_manager, &[], Default::default(), &[]);

        match &menus[EXPORT_IDX].items[0] {
            MenuItem::Action { action, .. } => {
                assert!(action.as_any().is::<ExportHtml>());
            }
            _ => panic!("expected export html action item"),
        }

        match &menus[EXPORT_IDX].items[1] {
            MenuItem::Action { action, .. } => {
                assert!(action.as_any().is::<ExportPdf>());
            }
            _ => panic!("expected export pdf action item"),
        }

        match &menus[EXPORT_IDX].items[4] {
            MenuItem::Action { name, action, .. } => {
                assert_eq!(name.as_ref(), "Word (.docx)");
                let action = action
                    .as_any()
                    .downcast_ref::<crate::components::ExportWithPandoc>()
                    .expect("pandoc export action");
                assert_eq!(action.format, "docx");
            }
            _ => panic!("expected pandoc export action item"),
        }
        assert_eq!(
            menus[EXPORT_IDX].items.len(),
            6 + crate::export::pandoc::EXPORT_FORMATS.len()
        );
        assert!(super::is_document_command_action(
            &crate::components::ImportWithPandoc
        ));
    }

    #[test]
    fn language_menu_items_dispatch_select_language_actions() {
        let theme_manager = ThemeManager::default();
        let i18n_manager = I18nManager::default();
        let menus = build_menus(&theme_manager, &i18n_manager, &[], Default::default(), &[]);

        match &menus[LANGUAGE_IDX].items[0] {
            MenuItem::Action { action, .. } => {
                let action = action
                    .as_any()
                    .downcast_ref::<SelectLanguage>()
                    .expect("language item should dispatch SelectLanguage");
                assert_eq!(action.language_id, "zh-CN");
            }
            _ => panic!("expected language action item"),
        }
    }

    #[test]
    fn recent_files_submenu_uses_empty_state_when_history_is_empty() {
        let theme_manager = ThemeManager::default();
        let i18n_manager = I18nManager::default();
        let menus = build_menus(&theme_manager, &i18n_manager, &[], Default::default(), &[]);

        // On macOS: File menu is index 1, Open Recent is item 3 within it.
        // On other platforms: File menu is index 0, Open Recent is item 3.
        #[cfg(target_os = "macos")]
        let recent_menu = submenu(&menus[1].items[5]);
        #[cfg(not(target_os = "macos"))]
        let recent_menu = submenu(&menus[0].items[5]);

        assert_eq!(recent_menu.name.to_string(), "Open Recent File");
        assert_eq!(recent_menu.items.len(), 1);
        assert_eq!(action_name(&recent_menu.items[0]), "No Recent Files");
        match &recent_menu.items[0] {
            MenuItem::Action { action, .. } => {
                assert!(action.as_any().is::<NoRecentFiles>());
            }
            _ => panic!("expected empty recent-file action item"),
        }
    }

    #[test]
    fn recent_files_submenu_dispatches_path_actions() {
        let theme_manager = ThemeManager::default();
        let i18n_manager = I18nManager::default();
        let recent_files = vec![
            PathBuf::from(r"C:\docs\one.md"),
            PathBuf::from(r"D:\notes\two.markdown"),
        ];
        let menus = build_menus(
            &theme_manager,
            &i18n_manager,
            &recent_files,
            Default::default(),
            &[],
        );

        #[cfg(target_os = "macos")]
        let recent_menu = submenu(&menus[1].items[5]);
        #[cfg(not(target_os = "macos"))]
        let recent_menu = submenu(&menus[0].items[5]);

        // Two files, a separator and Clear Items.
        assert_eq!(recent_menu.items.len(), 4);
        assert_eq!(action_name(&recent_menu.items[3]), "Clear Items");
        assert_eq!(action_name(&recent_menu.items[0]), r"C:\docs\one.md");
        match &recent_menu.items[0] {
            MenuItem::Action { action, .. } => {
                let action = action
                    .as_any()
                    .downcast_ref::<OpenRecentFile>()
                    .expect("recent file should dispatch OpenRecentFile");
                assert_eq!(action.path, r"C:\docs\one.md");
            }
            _ => panic!("expected recent-file action item"),
        }
    }

    #[test]
    fn fallback_menu_routes_window_context_actions_without_app_defer() {
        assert!(super::is_window_context_menu_action(&NewWindow));
        assert!(super::is_window_context_menu_action(&OpenFile));
        assert!(super::is_window_context_menu_action(&OpenPreferences));
        assert!(super::is_window_context_menu_action(&OpenRecentFile {
            path: "notes.md".into(),
        }));
        assert!(super::is_window_context_menu_action(&NoRecentFiles));
        assert!(super::is_window_context_menu_action(&AddLanguageConfig));
        assert!(super::is_window_context_menu_action(&AddThemeConfig));
        assert!(super::is_window_context_menu_action(&SaveDocument));
        assert!(super::is_window_context_menu_action(&QuitApplication));
        assert!(super::is_window_context_menu_action(&CloseWindow));
        assert!(!super::is_window_context_menu_action(&SelectTheme {
            theme_id: "velotype".into(),
        }));
        assert!(!super::is_window_context_menu_action(&SelectLanguage {
            language_id: "en-US".into(),
        }));
    }

    #[test]
    fn config_import_items_are_bottom_menu_actions() {
        let theme_manager = ThemeManager::default();
        let i18n_manager = I18nManager::default();
        let menus = build_menus(&theme_manager, &i18n_manager, &[], Default::default(), &[]);

        let language_items = &menus[LANGUAGE_IDX].items;
        assert!(matches!(
            language_items[language_items.len() - 2],
            MenuItem::Separator
        ));
        assert_eq!(
            action_name(&language_items[language_items.len() - 1]),
            "Add Language Config"
        );
        match &language_items[language_items.len() - 1] {
            MenuItem::Action { action, .. } => {
                assert!(action.as_any().is::<AddLanguageConfig>());
            }
            _ => panic!("expected add language config action item"),
        }

        let theme_items = &menus[THEME_IDX].items;
        assert_eq!(action_name(&theme_items[0]), "\u{2713} Velotype");
        assert_eq!(action_name(&theme_items[1]), "Velotype Light");
        assert!(matches!(
            theme_items[theme_items.len() - 4],
            MenuItem::Separator
        ));
        assert_eq!(
            action_name(&theme_items[theme_items.len() - 3]),
            "Follow System Appearance"
        );
        assert_eq!(
            action_name(&theme_items[theme_items.len() - 2]),
            "Add Theme Config"
        );
        assert_eq!(
            action_name(&theme_items[theme_items.len() - 1]),
            "Open Theme Folder"
        );
        match &theme_items[0] {
            MenuItem::Action { action, .. } => {
                assert!(action.as_any().is::<SelectTheme>());
            }
            _ => panic!("expected select theme action item"),
        }
        match &theme_items[theme_items.len() - 2] {
            MenuItem::Action { action, .. } => {
                assert!(action.as_any().is::<AddThemeConfig>());
            }
            _ => panic!("expected add theme config action item"),
        }
    }

    #[test]
    fn theme_menu_marks_follow_system_appearance_when_enabled() {
        let mut theme_manager = ThemeManager::default();
        theme_manager.set_appearance_themes(crate::theme::AppearanceThemes {
            follow_system: true,
            ..Default::default()
        });
        let i18n_manager = I18nManager::default();
        let menus = build_menus(&theme_manager, &i18n_manager, &[], Default::default(), &[]);
        let theme_items = &menus[THEME_IDX].items;
        let follow = &theme_items[theme_items.len() - 3];

        assert_eq!(action_name(follow), "\u{2713} Follow System Appearance");
        match follow {
            MenuItem::Action { action, .. } => {
                assert!(action.as_any().is::<ToggleFollowSystemAppearance>());
            }
            _ => panic!("expected follow system appearance action item"),
        }
    }

    #[test]
    fn theme_menu_marks_selected_builtin_light_theme() {
        let mut theme_manager = ThemeManager::default();
        assert!(theme_manager.set_theme_by_id("velotype-light"));
        let i18n_manager = I18nManager::default();
        let menus = build_menus(&theme_manager, &i18n_manager, &[], Default::default(), &[]);
        let theme_items = &menus[THEME_IDX].items;

        assert_eq!(action_name(&theme_items[0]), "Velotype");
        assert_eq!(action_name(&theme_items[1]), "\u{2713} Velotype Light");
        match &theme_items[1] {
            MenuItem::Action { action, .. } => {
                let action = action
                    .as_any()
                    .downcast_ref::<SelectTheme>()
                    .expect("light theme item should dispatch SelectTheme");
                assert_eq!(action.theme_id, "velotype-light");
            }
            _ => panic!("expected light theme action item"),
        }
    }

    #[test]
    fn help_menu_first_item_is_check_for_updates() {
        let theme_manager = ThemeManager::default();
        let i18n_manager = I18nManager::default();
        let menus = build_menus(&theme_manager, &i18n_manager, &[], Default::default(), &[]);
        let help_items = &menus[HELP_IDX].items;

        // Quick Start, Markdown Reference, Keyboard Shortcuts, separator.
        match &help_items[0] {
            MenuItem::Action { action, .. } => {
                assert!(action.as_any().is::<crate::components::OpenQuickStart>());
            }
            _ => panic!("expected quick start action item"),
        }
        assert!(matches!(help_items[3], MenuItem::Separator));
        match &help_items[4] {
            MenuItem::Action { action, .. } => {
                assert!(action.as_any().is::<CheckForUpdates>());
            }
            _ => panic!("expected check updates action item"),
        }
        assert!(matches!(help_items[5], MenuItem::Separator));
    }

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn help_menu_contains_update_and_about_only() {
        let theme_manager = ThemeManager::default();
        let i18n_manager = I18nManager::default();
        let menus = build_menus(&theme_manager, &i18n_manager, &[], Default::default(), &[]);
        let help_items = &menus[HELP_IDX].items;

        assert_eq!(help_items.len(), 7);
        match &help_items[6] {
            MenuItem::Action { action, .. } => {
                assert!(action.as_any().is::<ShowAbout>());
            }
            _ => panic!("expected about action item"),
        }
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn help_menu_contains_cli_and_about_on_macos() {
        let theme_manager = ThemeManager::default();
        let i18n_manager = I18nManager::default();
        let menus = build_menus(&theme_manager, &i18n_manager, &[], Default::default(), &[]);
        let help_items = &menus[HELP_IDX].items;

        // Help documents (3 + separator), CheckForUpdates, separator,
        // Install/Uninstall CLI, separator, About
        assert_eq!(help_items.len(), 9);
        assert!(matches!(help_items[7], MenuItem::Separator));
        match &help_items[8] {
            MenuItem::Action { action, .. } => {
                assert!(action.as_any().is::<ShowAbout>());
            }
            _ => panic!("expected about action item"),
        }
    }
}
