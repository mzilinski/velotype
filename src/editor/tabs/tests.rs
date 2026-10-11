//! Tab host behaviour: opening, switching, closing and the unsaved-changes
//! walk when a window with several tabs closes.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui::{ClickEvent, Context, Entity, TestAppContext, Window, WindowHandle};

use super::{EditorTabEvent, EditorWindow};
use crate::components::{CloseTab, CloseWindow, NewTab, NextTab, PrevTab};
use crate::editor::Editor;
use crate::i18n::I18nManager;
use crate::theme::ThemeManager;

fn init_test_app(cx: &mut TestAppContext) {
    cx.update(|cx| {
        I18nManager::init(cx);
        ThemeManager::init(cx);
        crate::components::init(cx);
    });
}

fn temp_markdown_path(test_name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "velotype-{test_name}-{}-{nanos}.md",
        std::process::id()
    ))
}

fn open_tab_window(cx: &mut TestAppContext, markdown: &str) -> WindowHandle<EditorWindow> {
    let markdown = markdown.to_string();
    let handle = cx.update(|cx| crate::app_menu::open_editor_window(cx, markdown, None));
    cx.run_until_parked();
    handle
}

fn tab_state(handle: &WindowHandle<EditorWindow>, cx: &mut TestAppContext) -> (usize, usize) {
    handle
        .update(cx, |host, _window, _cx| {
            (host.tab_count(), host.active_index())
        })
        .expect("editor window should be open")
}

fn editor_at(
    handle: &WindowHandle<EditorWindow>,
    index: usize,
    cx: &mut TestAppContext,
) -> Entity<Editor> {
    handle
        .update(cx, |host, _window, _cx| {
            host.editors()
                .nth(index)
                .cloned()
                .expect("tab index should exist")
        })
        .expect("editor window should be open")
}

fn with_host<R>(
    handle: &WindowHandle<EditorWindow>,
    cx: &mut TestAppContext,
    update: impl FnOnce(&mut EditorWindow, &mut Window, &mut Context<EditorWindow>) -> R,
) -> R {
    let result = handle
        .update(cx, update)
        .expect("editor window should be open");
    cx.run_until_parked();
    result
}

fn with_tab_editor<R>(
    handle: &WindowHandle<EditorWindow>,
    index: usize,
    cx: &mut TestAppContext,
    update: impl FnOnce(&mut Editor, &mut Window, &mut Context<Editor>) -> R,
) -> R {
    let editor = editor_at(handle, index, cx);
    let result = handle
        .update(cx, |_host, window, cx| {
            editor.update(cx, |editor, cx| update(editor, window, cx))
        })
        .expect("editor window should be open");
    cx.run_until_parked();
    result
}

fn window_is_open(handle: &WindowHandle<EditorWindow>, cx: &mut TestAppContext) -> bool {
    let window_id = handle.window_id();
    cx.update(|cx| {
        cx.windows()
            .iter()
            .any(|window| window.window_id() == window_id)
    })
}

#[gpui::test]
async fn tabs_open_switch_and_close_in_one_window(cx: &mut TestAppContext) {
    init_test_app(cx);
    let handle = open_tab_window(cx, "first");
    assert_eq!(tab_state(&handle, cx), (1, 0));

    with_host(&handle, cx, |host, window, cx| host.new_tab(window, cx));
    with_host(&handle, cx, |host, window, cx| host.new_tab(window, cx));
    assert_eq!(tab_state(&handle, cx), (3, 2));
    assert_eq!(cx.update(|cx| cx.windows().len()), 1);

    with_host(&handle, cx, |host, window, cx| {
        host.activate_next_tab(window, cx)
    });
    assert_eq!(tab_state(&handle, cx), (3, 0), "next tab wraps around");
    with_host(&handle, cx, |host, window, cx| {
        host.activate_previous_tab(window, cx)
    });
    assert_eq!(tab_state(&handle, cx), (3, 2), "previous tab wraps around");
    with_host(&handle, cx, |host, window, cx| {
        host.activate_tab(1, window, cx)
    });
    assert_eq!(tab_state(&handle, cx), (3, 1));

    // Closing a clean tab before the active one keeps the same tab active.
    let active_editor = editor_at(&handle, 1, cx);
    with_host(&handle, cx, |host, window, cx| {
        host.request_close_tab(0, window, cx)
    });
    assert_eq!(tab_state(&handle, cx), (2, 0));
    assert_eq!(
        editor_at(&handle, 0, cx).entity_id(),
        active_editor.entity_id()
    );

    // Closing the active clean tab activates its neighbour.
    with_host(&handle, cx, |host, window, cx| {
        host.request_close_tab(0, window, cx)
    });
    assert_eq!(tab_state(&handle, cx), (1, 0));
    assert!(window_is_open(&handle, cx));

    // Closing the last tab closes the window.
    with_host(&handle, cx, |host, window, cx| {
        host.request_close_tab(0, window, cx)
    });
    assert!(!window_is_open(&handle, cx));
}

#[gpui::test]
async fn tab_actions_dispatch_through_the_app_menu(cx: &mut TestAppContext) {
    init_test_app(cx);
    let handle = open_tab_window(cx, "first");

    cx.update(|cx| crate::app_menu::dispatch_menu_action(&NewTab, cx));
    cx.run_until_parked();
    assert_eq!(tab_state(&handle, cx), (2, 1));

    cx.update(|cx| crate::app_menu::dispatch_menu_action(&PrevTab, cx));
    cx.run_until_parked();
    assert_eq!(tab_state(&handle, cx), (2, 0));

    cx.update(|cx| crate::app_menu::dispatch_menu_action(&NextTab, cx));
    cx.run_until_parked();
    assert_eq!(tab_state(&handle, cx), (2, 1));

    cx.update(|cx| crate::app_menu::dispatch_menu_action(&CloseTab, cx));
    cx.run_until_parked();
    assert_eq!(tab_state(&handle, cx), (1, 0));
}

#[gpui::test]
async fn dirty_tab_close_prompts_and_discard_closes_only_that_tab(cx: &mut TestAppContext) {
    init_test_app(cx);
    let handle = open_tab_window(cx, "first");
    with_host(&handle, cx, |host, window, cx| host.new_tab(window, cx));
    with_host(&handle, cx, |host, window, cx| {
        host.activate_tab(0, window, cx)
    });
    with_tab_editor(&handle, 1, cx, |editor, _window, cx| editor.mark_dirty(cx));

    with_host(&handle, cx, |host, window, cx| {
        host.request_close_tab(1, window, cx)
    });
    assert_eq!(
        tab_state(&handle, cx),
        (2, 1),
        "the dirty tab is focused and kept open while its dialog is shown"
    );
    assert!(with_tab_editor(&handle, 1, cx, |editor, _window, _cx| {
        // The focus queued by the tab switch must not land behind the dialog.
        editor.show_unsaved_changes_dialog && editor.pending_focus.is_none()
    }));

    // Switching away is blocked while the dialog is open.
    with_host(&handle, cx, |host, window, cx| {
        host.activate_tab(0, window, cx)
    });
    assert_eq!(tab_state(&handle, cx), (2, 1));

    with_tab_editor(&handle, 1, cx, |editor, window, cx| {
        editor.on_discard_and_close(&ClickEvent::default(), window, cx)
    });
    assert_eq!(tab_state(&handle, cx), (1, 0));
    assert!(window_is_open(&handle, cx));
}

#[gpui::test]
async fn dirty_tab_close_cancel_keeps_the_tab(cx: &mut TestAppContext) {
    init_test_app(cx);
    let handle = open_tab_window(cx, "first");
    with_host(&handle, cx, |host, window, cx| host.new_tab(window, cx));
    with_tab_editor(&handle, 1, cx, |editor, _window, cx| editor.mark_dirty(cx));

    with_host(&handle, cx, |host, window, cx| {
        host.request_close_tab(1, window, cx)
    });
    with_tab_editor(&handle, 1, cx, |editor, window, cx| {
        editor.on_cancel_close_dialog(&ClickEvent::default(), window, cx)
    });

    assert_eq!(tab_state(&handle, cx), (2, 1));
    assert!(with_tab_editor(&handle, 1, cx, |editor, _window, _cx| {
        !editor.show_unsaved_changes_dialog && editor.document_dirty
    }));
}

#[gpui::test]
async fn dirty_tab_save_and_close_writes_file_then_closes_tab(cx: &mut TestAppContext) {
    init_test_app(cx);
    let path = temp_markdown_path("tab-save-and-close");
    fs::write(&path, "saved tab").expect("write tab markdown");

    let handle = open_tab_window(cx, "first");
    with_host(&handle, cx, |host, window, cx| {
        host.open_document("saved tab".to_string(), Some(path.clone()), window, cx);
    });
    assert_eq!(tab_state(&handle, cx), (2, 1));
    with_tab_editor(&handle, 1, cx, |editor, _window, cx| editor.mark_dirty(cx));
    with_host(&handle, cx, |host, window, cx| {
        host.request_close_tab(1, window, cx)
    });

    with_tab_editor(&handle, 1, cx, |editor, window, cx| {
        editor.on_save_and_close(&ClickEvent::default(), window, cx);
        // Rendering normally picks the pending save up; run it directly.
        editor.pending_save = false;
        editor.save_document(window, cx);
    });

    assert_eq!(tab_state(&handle, cx), (1, 0));
    assert_eq!(
        fs::read_to_string(&path).expect("read saved tab").trim(),
        "saved tab"
    );
    let _ = fs::remove_file(path);
}

#[gpui::test]
async fn closing_window_walks_dirty_tabs_one_dialog_at_a_time(cx: &mut TestAppContext) {
    init_test_app(cx);
    let handle = open_tab_window(cx, "first");
    with_host(&handle, cx, |host, window, cx| host.new_tab(window, cx));
    with_host(&handle, cx, |host, window, cx| host.new_tab(window, cx));
    with_tab_editor(&handle, 0, cx, |editor, _window, cx| editor.mark_dirty(cx));
    with_tab_editor(&handle, 2, cx, |editor, _window, cx| editor.mark_dirty(cx));
    let last_dirty = editor_at(&handle, 2, cx);

    assert!(!with_host(&handle, cx, |host, window, cx| {
        host.on_window_should_close(window, cx)
    }));
    assert_eq!(tab_state(&handle, cx), (3, 0), "first dirty tab is shown");
    assert!(with_tab_editor(&handle, 0, cx, |editor, _window, _cx| {
        editor.show_unsaved_changes_dialog
    }));

    with_tab_editor(&handle, 0, cx, |editor, window, cx| {
        editor.on_discard_and_close(&ClickEvent::default(), window, cx)
    });
    assert!(window_is_open(&handle, cx));
    let (count, active) = tab_state(&handle, cx);
    assert_eq!(count, 2);
    assert_eq!(
        editor_at(&handle, active, cx).entity_id(),
        last_dirty.entity_id(),
        "the next dirty tab is shown after the first one is resolved"
    );
    assert!(with_tab_editor(
        &handle,
        active,
        cx,
        |editor, _window, _cx| { editor.show_unsaved_changes_dialog }
    ));

    with_tab_editor(&handle, active, cx, |editor, window, cx| {
        editor.on_discard_and_close(&ClickEvent::default(), window, cx)
    });
    assert!(
        !window_is_open(&handle, cx),
        "remaining clean tabs close with the window"
    );
}

#[gpui::test]
async fn closing_window_cancel_stops_the_walk(cx: &mut TestAppContext) {
    init_test_app(cx);
    let handle = open_tab_window(cx, "first");
    with_host(&handle, cx, |host, window, cx| host.new_tab(window, cx));
    with_tab_editor(&handle, 0, cx, |editor, _window, cx| editor.mark_dirty(cx));
    with_tab_editor(&handle, 1, cx, |editor, _window, cx| editor.mark_dirty(cx));

    cx.update(|cx| crate::app_menu::dispatch_menu_action(&CloseWindow, cx));
    cx.run_until_parked();
    assert_eq!(tab_state(&handle, cx), (2, 0));

    with_tab_editor(&handle, 0, cx, |editor, window, cx| {
        editor.on_cancel_close_dialog(&ClickEvent::default(), window, cx)
    });
    assert_eq!(tab_state(&handle, cx), (2, 0));
    assert!(window_is_open(&handle, cx));
    assert!(!with_tab_editor(&handle, 1, cx, |editor, _window, _cx| {
        editor.show_unsaved_changes_dialog
    }));

    // A later tab close is a plain tab close, not a continuation of the
    // cancelled window close.
    with_host(&handle, cx, |host, window, cx| {
        host.request_close_tab(0, window, cx)
    });
    with_tab_editor(&handle, 0, cx, |editor, window, cx| {
        editor.on_discard_and_close(&ClickEvent::default(), window, cx)
    });
    assert_eq!(tab_state(&handle, cx), (1, 0));
    assert!(window_is_open(&handle, cx));
}

#[gpui::test]
async fn opening_files_reuses_pristine_tab_and_focuses_open_paths(cx: &mut TestAppContext) {
    init_test_app(cx);
    let first_path = temp_markdown_path("tab-open-first");
    let second_path = temp_markdown_path("tab-open-second");
    fs::write(&first_path, "first file").expect("write first markdown");
    fs::write(&second_path, "second file").expect("write second markdown");

    let handle = open_tab_window(cx, "");
    cx.update(|cx| crate::app_menu::open_file_in_tab(cx, &first_path))
        .expect("open first file");
    cx.run_until_parked();
    assert_eq!(
        tab_state(&handle, cx),
        (1, 0),
        "an empty untitled tab is replaced"
    );
    assert_eq!(
        with_tab_editor(&handle, 0, cx, |editor, _window, _cx| {
            editor.file_path.clone()
        }),
        Some(first_path.clone())
    );

    cx.update(|cx| crate::app_menu::open_file_in_tab(cx, &second_path))
        .expect("open second file");
    cx.run_until_parked();
    assert_eq!(tab_state(&handle, cx), (2, 1));
    assert_eq!(cx.update(|cx| cx.windows().len()), 1);

    cx.update(|cx| crate::app_menu::open_file_in_tab(cx, &first_path))
        .expect("reopen first file");
    cx.run_until_parked();
    assert_eq!(
        tab_state(&handle, cx),
        (2, 0),
        "an already open path focuses its tab"
    );

    let _ = fs::remove_file(first_path);
    let _ = fs::remove_file(second_path);
}

#[gpui::test]
async fn dropped_paths_open_as_tabs_of_the_hosting_window(cx: &mut TestAppContext) {
    init_test_app(cx);
    let path = temp_markdown_path("tab-dropped");
    fs::write(&path, "dropped file").expect("write dropped markdown");

    let handle = open_tab_window(cx, "keep me");
    let dropped = path.clone();
    with_tab_editor(&handle, 0, cx, move |_editor, _window, cx| {
        cx.emit(EditorTabEvent::OpenPaths {
            paths: vec![dropped],
            open_workspace: false,
        });
    });

    assert_eq!(tab_state(&handle, cx), (2, 1));
    let text = with_tab_editor(&handle, 1, cx, |editor, _window, cx| {
        editor.serialized_document_text(cx)
    });
    assert_eq!(text.trim(), "dropped file");
    let _ = fs::remove_file(path);
}

#[gpui::test]
async fn hosted_editor_routes_close_window_to_its_tab_host(cx: &mut TestAppContext) {
    init_test_app(cx);
    let handle = open_tab_window(cx, "first");
    with_host(&handle, cx, |host, window, cx| host.new_tab(window, cx));

    // The editor's own Close Window path (shortcut / titlebar button) defers to
    // the host, which closes every clean tab together with the window.
    with_tab_editor(&handle, 1, cx, |editor, window, cx| {
        assert!(!editor.close_guard_installed);
        editor.request_close_current_window(window, cx);
    });
    assert!(!window_is_open(&handle, cx));
}
