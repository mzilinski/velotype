//! Velotype - a block-based Markdown editor built with GPUI.
//!
//! Reads file paths from command-line arguments and opens them as tabs of one
//! GPUI window. With no arguments, a single empty window is created.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::borrow::Cow;
use std::path::PathBuf;
#[cfg(target_os = "macos")]
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[cfg(target_os = "macos")]
use futures::{StreamExt, channel::mpsc};
use gpui::*;

mod app_identity;
mod app_menu;
mod clipboard;
mod components;
mod config;
mod editor;
mod export;
#[cfg(any(target_os = "macos", test))]
mod file_url;
mod fonts;
mod help;
mod i18n;
mod net;
mod perf_log;
mod spellcheck;
mod text_file;
mod theme;
mod window_chrome;
mod window_pin;

use app_menu::{init as init_app_menu, open_editor_window};
use components::init_with_keybindings as init_editor;
#[cfg(target_os = "macos")]
use file_url::parse_file_url;
use i18n::I18nManager;
use theme::ThemeManager;

struct VelotypeAssets;

fn open_startup_window(cx: &mut App, startup_open: config::StartupOpenPreference) {
    open_startup_document(cx, startup_open);
    app_menu::restore_session_drafts(cx);
}

fn open_startup_document(cx: &mut App, startup_open: config::StartupOpenPreference) {
    if startup_open == config::StartupOpenPreference::LastOpenedFile
        && let Some(path) = config::first_existing_recent_markdown_file()
    {
        match text_file::read_text_file(&path) {
            Ok(markdown) => {
                open_editor_window(cx, markdown, Some(path));
                return;
            }
            Err(err) => {
                eprintln!(
                    "failed to read last opened file '{}': {err}",
                    path.display()
                );
            }
        }
    }

    open_editor_window(cx, String::new(), None);
}

impl AssetSource for VelotypeAssets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        match path {
            "icon/workspace/folder.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/folder.svg"
            )))),
            "icon/workspace/markdown.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/markdown.svg"
            )))),
            "icon/titlebar/chrome-close.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/titlebar/chrome-close.svg"
            )))),
            "icon/titlebar/chrome-minimize.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/titlebar/chrome-minimize.svg"
            )))),
            "icon/titlebar/chrome-maximize.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/titlebar/chrome-maximize.svg"
            )))),
            "icon/titlebar/chrome-restore.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/titlebar/chrome-restore.svg"
            )))),
            _ => Ok(None),
        }
    }

    fn list(&self, _path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Parse command-line arguments
    let mut detach = false;
    let mut input_paths = Vec::new();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--version" | "-v" => {
                println!("velotype {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--help" | "-h" => {
                println!(
                    "velotype {} - A block-based Markdown editor",
                    env!("CARGO_PKG_VERSION")
                );
                println!();
                println!("USAGE:");
                println!("    velotype [OPTIONS] [FILES...]");
                println!();
                println!("OPTIONS:");
                println!("    -v, --version    Print version information");
                println!("    -h, --help       Print this help message");
                println!("    -d, --detach     Launch in background (non-blocking)");
                println!();
                println!("FILES:");
                println!("    One or more markdown files to open. If no files are specified,");
                println!("    opens an empty document.");
                return;
            }
            "--detach" | "-d" => {
                detach = true;
            }
            option if option.starts_with('-') => {
                eprintln!("Unknown option: {}", option);
                std::process::exit(1);
            }
            path => {
                input_paths.push(PathBuf::from(path));
            }
        }
        i += 1;
    }

    #[cfg(not(target_os = "macos"))]
    let _ = detach;

    // On macOS, detach from terminal if requested
    // TODO: Other platforms may also need to be adapted
    #[cfg(target_os = "macos")]
    if detach {
        use std::process::Command;

        // Re-launch the application in the background without the --detach flag
        let exe_path = std::env::current_exe().expect("Failed to get executable path");
        let non_detach_args: Vec<String> = args
            .iter()
            .filter(|arg| *arg != "--detach" && *arg != "-d")
            .cloned()
            .collect();

        Command::new(exe_path)
            .args(&non_detach_args[1..])
            .spawn()
            .expect("Failed to detach process");

        return;
    }

    #[cfg(target_os = "macos")]
    let (open_file_tx, mut open_file_rx) = mpsc::unbounded::<PathBuf>();
    #[cfg(target_os = "macos")]
    let open_file_requested = Arc::new(AtomicBool::new(false));

    let app = Application::new().with_assets(VelotypeAssets);

    #[cfg(target_os = "macos")]
    {
        let open_file_requested_for_callback = open_file_requested.clone();
        app.on_open_urls(move |urls| {
            for url in urls {
                let Some(path) = parse_file_url(&url) else {
                    continue;
                };
                open_file_requested_for_callback.store(true, Ordering::SeqCst);
                let _ = open_file_tx.unbounded_send(path);
            }
        });
    }

    app.run(move |cx: &mut App| {
        perf_log::start(cx);
        let startup = perf_log::Span::new("Start: Einstellungen, Themes, Wörterbuch, Menüs");
        let preferences = config::load_or_create_app_preferences().unwrap_or_else(|err| {
            eprintln!("failed to initialize app preferences: {err}");
            Default::default()
        });
        I18nManager::init_with_language_id(cx, &preferences.default_language_id);
        ThemeManager::init_with_theme_id(cx, &preferences.default_theme_id);
        config::init_appearance_themes(cx, preferences.appearance_themes.clone());
        components::svg_cache::spawn_svg_cache_cleanup();
        fonts::FontSettings::init(cx, preferences.fonts.clone());
        config::EditorSettings::init(cx, preferences.show_table_headers);
        cx.global_mut::<ThemeManager>()
            .set_zoom(f32::from(preferences.writing.zoom_percent) / 100.0);
        spellcheck::SpellChecker::init(cx, &preferences.writing.spellcheck_language);
        config::SessionDrafts::init(cx);
        net::install_http_client(cx);
        init_editor(cx, &preferences.keybindings);
        init_app_menu(cx);
        drop(startup);

        #[cfg(target_os = "macos")]
        cx.spawn(async move |cx| {
            while let Some(path) = open_file_rx.next().await {
                let _ = cx.update(move |cx| {
                    if let Err(err) = app_menu::open_file_in_tab(cx, &path) {
                        eprintln!("failed to open '{}': {err}", path.display());
                    }
                });
            }
        })
        .detach();

        if input_paths.is_empty() {
            #[cfg(target_os = "macos")]
            {
                let startup_open = preferences.startup_open;
                let open_file_requested = open_file_requested.clone();
                cx.spawn(async move |cx| {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(150))
                        .await;
                    if !open_file_requested.load(Ordering::SeqCst) {
                        let _ = cx.update(move |cx| open_startup_window(cx, startup_open));
                    }
                })
                .detach();
            }

            #[cfg(not(target_os = "macos"))]
            open_startup_window(cx, preferences.startup_open);

            return;
        }

        let mut startup_window: Option<WindowHandle<editor::EditorWindow>> = None;
        let mut imports = Vec::new();
        for path in &input_paths {
            let absolute_path = if path.is_absolute() {
                path.clone()
            } else {
                match std::env::current_dir() {
                    Ok(cwd) => cwd.join(path),
                    Err(_) => path.clone(),
                }
            };

            if text_file::is_import_document(&absolute_path) {
                imports.push(absolute_path);
                continue;
            }
            let markdown = match text_file::read_text_file(&absolute_path) {
                Ok(content) => {
                    if let Err(err) = config::record_recent_file(&absolute_path) {
                        eprintln!("failed to update recent file history: {err}");
                    }
                    content
                }
                // A file that does not exist yet opens empty and is created
                // on save.
                Err(err) if !absolute_path.exists() => {
                    eprintln!(
                        "failed to read '{}': {err:#}. opened as empty document.",
                        absolute_path.display()
                    );
                    String::new()
                }
                // Never bind an existing file that could not be read to an
                // empty document: saving it would overwrite the file.
                Err(err) => {
                    eprintln!("failed to read '{}': {err:#}", absolute_path.display());
                    perf_log::note(format!("Start: {err:#}"));
                    continue;
                }
            };
            match startup_window {
                Some(handle) => {
                    let _ = handle.update(cx, |host, window, cx| {
                        host.open_document(markdown, Some(absolute_path), window, cx);
                    });
                }
                None => {
                    startup_window = Some(open_editor_window(cx, markdown, Some(absolute_path)))
                }
            }
        }
        if startup_window.is_none() {
            open_editor_window(cx, String::new(), None);
        }
        for path in imports {
            app_menu::import_document_in_tab(cx, path);
        }
        app_menu::restore_session_drafts(cx);
        app_menu::install_menus(cx);
        cx.refresh_windows();
    });
}
