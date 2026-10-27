//! Action definitions and key bindings for both block editing and app-level
//! window/menu commands.
//!
//! Text-editing actions are scoped to the `"BlockEditor"` key context on each
//! block. Window and menu commands use global bindings so they remain
//! available even when focus is on non-block UI such as dialogs or buttons.

use std::collections::{BTreeMap, BTreeSet};

use gpui::*;
use schemars::JsonSchema;
use serde::Deserialize;

actions!(
    velotype,
    [
        Newline,
        DeleteBack,
        Delete,
        WordDeleteBack,
        WordDeleteForward,
        FocusPrev,
        FocusNext,
        MoveLeft,
        MoveRight,
        WordMoveLeft,
        WordMoveRight,
        Home,
        End,
        BlockUp,
        BlockDown,
        PageUp,
        PageDown,
        JumpToTop,
        JumpToBottom,
        SelectLeft,
        SelectRight,
        WordSelectLeft,
        WordSelectRight,
        SelectHome,
        SelectEnd,
        SelectAll,
        Copy,
        Cut,
        Paste,
        Undo,
        Redo,
        BoldSelection,
        ItalicSelection,
        UnderlineSelection,
        CodeSelection,
        SetLineEndingCrlf,
        SetLineEndingLf,
        ToggleFinalNewline,
        OpenEmojiPicker,
        PrintDocument,
        ExportAgain,
        ExportHtmlPlain,
        MoveDocument,
        ShowProperties,
        InsertImage,
        CopyImage,
        SetImageRoot,
        RevealImageFile,
        ToggleImageSyntax,
        CopyImageTo,
        MoveImageTo,
        DeleteImageFile,
        ToggleAutosave,
        ShowBackups,
        UploadImage,
        UploadAllImages,
        SelectLine,
        SelectStyledScope,
        SelectWord,
        SelectBlock,
        JumpToSelection,
        DeleteWord,
        DeleteLine,
        DeleteStyledScope,
        DeleteBlock,
        PasteAsPlainText,
        CopyAsPlainText,
        CopyAsHtml,
        IncreaseHeadingLevel,
        DecreaseHeadingLevel,
        ToggleQuote,
        ToggleOrderedList,
        ToggleBulletList,
        ToggleTaskList,
        ToggleTaskStatus,
        InsertTable,
        InsertCodeBlock,
        InsertMathBlock,
        InsertHorizontalRule,
        InsertTableOfContents,
        InsertFootnote,
        InsertLinkReference,
        InsertFrontMatter,
        StrikethroughSelection,
        HighlightSelection,
        SuperscriptSelection,
        SubscriptSelection,
        InlineMathSelection,
        CommentSelection,
        InsertLink,
        ClearFormat,
        SetParagraph,
        SetHeading1,
        SetHeading2,
        SetHeading3,
        SetHeading4,
        SetHeading5,
        SetHeading6,
        IndentBlock,
        OutdentBlock,
        ExitCodeBlock,
        SaveDocument,
        NewWindow,
        OpenFile,
        OpenPreferences,
        NoRecentFiles,
        SaveDocumentAs,
        ExportHtml,
        ExportPdf,
        ImportWithPandoc,
        AddLanguageConfig,
        AddThemeConfig,
        OpenThemeFolder,
        OpenQuickStart,
        OpenMarkdownReference,
        OpenKeyboardShortcuts,
        ToggleFollowSystemAppearance,
        QuitApplication,
        CloseWindow,
        CheckForUpdates,
        ShowAbout,
        InstallCliTool,
        UninstallCliTool,
        ToggleAutoPair,
        ToggleSmartQuotes,
        ToggleSmartDashes,
        ToggleFocusMode,
        ToggleTypewriterMode,
        ToggleReadOnly,
        ToggleSpellcheck,
        ToggleStatusBar,
        ToggleFullScreen,
        ToggleAlwaysOnTop,
        ResetZoom,
        ZoomIn,
        ZoomOut,
        QuickOpen,
        OpenFolder,
        CloseFolder,
        ReloadFromDisk,
        SaveAll,
        ReopenClosedTab,
        RevealInFileManager,
        RevealInSidebar,
        DeleteCurrentFile,
        ClearRecentFiles,
        SearchInWorkspace,
        DismissTransientUi,
        ToggleViewMode,
        ToggleWorkspace,
        ToggleToolbar,
        ShowFileTree,
        ShowOutline,
        ShowArticles,
        ShowLinks,
        FindInDocument,
        FindAndReplace,
        FindNext,
        FindPrevious,
        NewTab,
        CloseTab,
        NextTab,
        PrevTab,
    ]
);

/// Inserts a GitHub-style alert block (`> [!NOTE]` and friends).
#[derive(Clone, Debug, PartialEq, Deserialize, JsonSchema, gpui::Action)]
#[action(namespace = velotype)]
#[serde(deny_unknown_fields)]
pub struct InsertCallout {
    /// Alert type: note, tip, important, warning or caution.
    pub kind: String,
}

/// Zooms the image at the caret (Format > Image > Zoom).
#[derive(Clone, Debug, PartialEq, Deserialize, JsonSchema, gpui::Action)]
#[action(namespace = velotype)]
#[serde(deny_unknown_fields)]
pub struct SetImageZoom {
    /// Zoom in percent; 100 restores the plain Markdown image.
    pub percent: u32,
}

/// Exports the document with Pandoc (File > Export > Word, LaTeX, ...).
#[derive(Clone, Debug, PartialEq, Deserialize, JsonSchema, gpui::Action)]
#[action(namespace = velotype)]
#[serde(deny_unknown_fields)]
pub struct ExportWithPandoc {
    /// Pandoc writer name such as `docx` or `latex`.
    pub format: String,
}

/// Selects a theme from the app-level theme registry.
#[derive(Clone, Debug, PartialEq, Deserialize, JsonSchema, gpui::Action)]
#[action(namespace = velotype)]
#[serde(deny_unknown_fields)]
pub struct SelectTheme {
    /// Stable theme id from the built-in theme catalog.
    pub theme_id: String,
}

/// Selects a UI language from the app-level language registry.
#[derive(Clone, Debug, PartialEq, Deserialize, JsonSchema, gpui::Action)]
#[action(namespace = velotype)]
#[serde(deny_unknown_fields)]
pub struct SelectLanguage {
    /// Stable language id from the built-in language catalog.
    pub language_id: String,
}

/// Selects the spell-check dictionary language.
#[derive(Clone, Debug, PartialEq, Deserialize, JsonSchema, gpui::Action)]
#[action(namespace = velotype)]
#[serde(deny_unknown_fields)]
pub struct SelectSpellcheckLanguage {
    /// BCP-47 language tag such as `de-DE`.
    pub language: String,
}

/// Opens a previously recorded Markdown file path.
#[derive(Clone, Debug, PartialEq, Deserialize, JsonSchema, gpui::Action)]
#[action(namespace = velotype)]
#[serde(deny_unknown_fields)]
pub struct OpenRecentFile {
    /// Path stored in Velotype's recent-file history.
    pub path: String,
}

/// Group used to organize editable shortcuts in preferences.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum ShortcutCategory {
    /// Document and window commands.
    File,
    /// Text and clipboard commands.
    Edit,
    /// Caret, selection, and view navigation.
    Navigation,
    /// Inline text formatting commands.
    Formatting,
    /// Block structure commands.
    Block,
    /// Commands outside the other groups.
    Other,
}

/// Command identity for shortcut configuration and GPUI action binding.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum ShortcutCommand {
    /// Insert a line break or split the current block.
    Newline,
    /// Delete backward from the caret.
    DeleteBack,
    /// Delete forward from the caret.
    Delete,
    /// Delete the preceding word.
    WordDeleteBack,
    /// Delete the following word.
    WordDeleteForward,
    /// Focus the previous block.
    FocusPrev,
    /// Focus the next block.
    FocusNext,
    /// Move the caret left.
    MoveLeft,
    /// Move the caret right.
    MoveRight,
    /// Move the caret one word left.
    WordMoveLeft,
    /// Move the caret one word right.
    WordMoveRight,
    /// Move to the start of the current line.
    Home,
    /// Move to the end of the current line.
    End,
    /// Move to the preceding block.
    BlockUp,
    /// Move to the following block.
    BlockDown,
    /// Move one page upward.
    PageUp,
    /// Move one page downward.
    PageDown,
    /// Move to the start of the document.
    JumpToTop,
    /// Move to the end of the document.
    JumpToBottom,
    /// Extend the selection left.
    SelectLeft,
    /// Extend the selection right.
    SelectRight,
    /// Extend the selection one word left.
    WordSelectLeft,
    /// Extend the selection one word right.
    WordSelectRight,
    /// Extend the selection to the start of the line.
    SelectHome,
    /// Extend the selection to the end of the line.
    SelectEnd,
    /// Cycle through the editor's select-all scopes.
    SelectAll,
    /// Copy the current selection.
    Copy,
    /// Cut the current selection.
    Cut,
    /// Paste clipboard content.
    Paste,
    /// Undo the latest edit.
    Undo,
    /// Redo the latest undone edit.
    Redo,
    /// Toggle bold formatting on the selection.
    BoldSelection,
    /// Toggle italic formatting on the selection.
    ItalicSelection,
    /// Toggle underline formatting on the selection.
    UnderlineSelection,
    /// Toggle inline code formatting on the selection.
    CodeSelection,
    SetLineEndingCrlf,
    SetLineEndingLf,
    ToggleFinalNewline,
    OpenEmojiPicker,
    PrintDocument,
    ExportAgain,
    ExportHtmlPlain,
    MoveDocument,
    ShowProperties,
    InsertImage,
    CopyImage,
    SetImageRoot,
    RevealImageFile,
    ToggleImageSyntax,
    CopyImageTo,
    MoveImageTo,
    DeleteImageFile,
    ToggleAutosave,
    ShowBackups,
    UploadImage,
    UploadAllImages,
    SelectLine,
    SelectStyledScope,
    SelectWord,
    SelectBlock,
    JumpToSelection,
    DeleteWord,
    DeleteLine,
    DeleteStyledScope,
    DeleteBlock,
    PasteAsPlainText,
    CopyAsPlainText,
    CopyAsHtml,
    IncreaseHeadingLevel,
    DecreaseHeadingLevel,
    ToggleQuote,
    ToggleOrderedList,
    ToggleBulletList,
    ToggleTaskList,
    ToggleTaskStatus,
    InsertTable,
    InsertCodeBlock,
    InsertMathBlock,
    InsertHorizontalRule,
    InsertTableOfContents,
    InsertFootnote,
    InsertLinkReference,
    InsertFrontMatter,
    StrikethroughSelection,
    HighlightSelection,
    SuperscriptSelection,
    SubscriptSelection,
    InlineMathSelection,
    CommentSelection,
    InsertLink,
    ClearFormat,
    SetParagraph,
    SetHeading1,
    SetHeading2,
    SetHeading3,
    SetHeading4,
    SetHeading5,
    SetHeading6,
    /// Increase block indentation.
    IndentBlock,
    /// Decrease block indentation.
    OutdentBlock,
    /// Leave a fenced code block.
    ExitCodeBlock,
    /// Save the current document.
    SaveDocument,
    /// Save the document under a chosen path.
    SaveDocumentAs,
    /// Open a new editor window.
    NewWindow,
    /// Prompt to open a document.
    OpenFile,
    /// Request guarded application quit.
    QuitApplication,
    /// Request closing the current window.
    CloseWindow,
    ToggleAutoPair,
    ToggleSmartQuotes,
    ToggleSmartDashes,
    ToggleFocusMode,
    ToggleTypewriterMode,
    ToggleReadOnly,
    ToggleSpellcheck,
    ToggleStatusBar,
    ToggleFullScreen,
    ToggleAlwaysOnTop,
    ResetZoom,
    ZoomIn,
    ZoomOut,
    QuickOpen,
    OpenFolder,
    CloseFolder,
    ReloadFromDisk,
    SaveAll,
    ReopenClosedTab,
    RevealInFileManager,
    RevealInSidebar,
    DeleteCurrentFile,
    ClearRecentFiles,
    SearchInWorkspace,
    /// Dismiss transient editor UI.
    DismissTransientUi,
    /// Toggle the editor's view mode.
    ToggleViewMode,
    /// Toggle workspace visibility.
    ToggleWorkspace,
    ToggleToolbar,
    ShowFileTree,
    ShowOutline,
    ShowArticles,
    ShowLinks,
    FindInDocument,
    FindAndReplace,
    FindNext,
    FindPrevious,
    NewTab,
    CloseTab,
    NextTab,
    PrevTab,
}

/// Shortcut metadata used for defaults, preference labels, and binding context.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ShortcutDefinition {
    /// Command represented by this definition.
    pub(crate) command: ShortcutCommand,
    /// Stable key used in persisted shortcut preferences.
    pub(crate) id: &'static str,
    /// Group displayed in shortcut preferences.
    pub(crate) category: ShortcutCategory,
    /// Platform-specific fallback keystrokes.
    pub(crate) default_keys: &'static [&'static str],
    /// GPUI key context, or `None` for a global command.
    pub(crate) context: Option<&'static str>,
}

/// GPUI key context shared by block-editing commands.
const BLOCK_CONTEXT: Option<&str> = Some("BlockEditor");
/// Current persisted identifier for the unified select-all action.
const SELECT_ALL_ID: &str = "select_all";
/// Older preference identifiers migrated to unified select-all.
const LEGACY_SELECT_ALL_IDS: &[&str] = &[
    "select_all_source_text",
    "select_focused_block_text_rendered",
];

// On macOS cmd-q is the system quit shortcut; Windows/Linux use Alt+F4 (OS-handled).
#[cfg(target_os = "macos")]
/// macOS application quit binding.
const QUIT_APPLICATION_DEFAULT_KEYS: &[&str] = &["cmd-q"];
#[cfg(not(target_os = "macos"))]
/// Empty app-level quit binding on platforms with OS-managed quit keys.
const QUIT_APPLICATION_DEFAULT_KEYS: &[&str] = &[];

// On macOS cmd-w closes the active tab (closing the window with its last tab), so
// closing the whole window moves to cmd-shift-w. Other platforms keep ctrl-q.
#[cfg(target_os = "macos")]
/// macOS current-window close binding.
const CLOSE_WINDOW_DEFAULT_KEYS: &[&str] = &["cmd-shift-w"];
#[cfg(not(target_os = "macos"))]
/// Non-macOS current-window close binding.
const CLOSE_WINDOW_DEFAULT_KEYS: &[&str] = &["ctrl-q"];

// ctrl-w already toggles the workspace drawer on Windows/Linux, so tabs close with
// the conventional ctrl-f4 there.
#[cfg(target_os = "macos")]
const CLOSE_TAB_DEFAULT_KEYS: &[&str] = &["cmd-w"];
#[cfg(not(target_os = "macos"))]
const CLOSE_TAB_DEFAULT_KEYS: &[&str] = &["ctrl-f4"];

/// Canonical command catalog and default key assignments.
const SHORTCUT_DEFINITIONS: &[ShortcutDefinition] = &[
    ShortcutDefinition {
        command: ShortcutCommand::Newline,
        id: "newline",
        category: ShortcutCategory::Block,
        default_keys: &["enter"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::DeleteBack,
        id: "delete_back",
        category: ShortcutCategory::Edit,
        default_keys: &["backspace"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::Delete,
        id: "delete",
        category: ShortcutCategory::Edit,
        default_keys: &["delete"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::WordDeleteBack,
        id: "word_delete_back",
        category: ShortcutCategory::Edit,
        default_keys: &["ctrl-backspace", "alt-backspace"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::WordDeleteForward,
        id: "word_delete_forward",
        category: ShortcutCategory::Edit,
        default_keys: &["ctrl-delete", "alt-delete"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::FocusPrev,
        id: "focus_prev",
        category: ShortcutCategory::Navigation,
        default_keys: &["up"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::FocusNext,
        id: "focus_next",
        category: ShortcutCategory::Navigation,
        default_keys: &["down"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::MoveLeft,
        id: "move_left",
        category: ShortcutCategory::Navigation,
        default_keys: &["left"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::MoveRight,
        id: "move_right",
        category: ShortcutCategory::Navigation,
        default_keys: &["right"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::WordMoveLeft,
        id: "word_move_left",
        category: ShortcutCategory::Navigation,
        default_keys: &["ctrl-left", "alt-left"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::WordMoveRight,
        id: "word_move_right",
        category: ShortcutCategory::Navigation,
        default_keys: &["ctrl-right", "alt-right"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::Home,
        id: "home",
        category: ShortcutCategory::Navigation,
        default_keys: &["home"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::End,
        id: "end",
        category: ShortcutCategory::Navigation,
        default_keys: &["end"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::BlockUp,
        id: "block_up",
        category: ShortcutCategory::Navigation,
        default_keys: &["ctrl-up", "alt-up"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::BlockDown,
        id: "block_down",
        category: ShortcutCategory::Navigation,
        default_keys: &["ctrl-down", "alt-down"],
        context: BLOCK_CONTEXT,
    },
    // Page scroll and document jumps operate on the editor viewport rather than
    // a single block, so they use global bindings (no context) and stay active
    // in both Rendered and Source mode.
    ShortcutDefinition {
        command: ShortcutCommand::PageUp,
        id: "page_up",
        category: ShortcutCategory::Navigation,
        default_keys: &["pageup"],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::PageDown,
        id: "page_down",
        category: ShortcutCategory::Navigation,
        default_keys: &["pagedown"],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::JumpToTop,
        id: "jump_to_top",
        category: ShortcutCategory::Navigation,
        default_keys: &["ctrl-home", "cmd-up"],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::JumpToBottom,
        id: "jump_to_bottom",
        category: ShortcutCategory::Navigation,
        default_keys: &["ctrl-end", "cmd-down"],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::SelectLeft,
        id: "select_left",
        category: ShortcutCategory::Navigation,
        default_keys: &["shift-left"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::SelectRight,
        id: "select_right",
        category: ShortcutCategory::Navigation,
        default_keys: &["shift-right"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::WordSelectLeft,
        id: "word_select_left",
        category: ShortcutCategory::Navigation,
        default_keys: &["ctrl-shift-left", "alt-shift-left"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::WordSelectRight,
        id: "word_select_right",
        category: ShortcutCategory::Navigation,
        default_keys: &["ctrl-shift-right", "alt-shift-right"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::SelectHome,
        id: "select_home",
        category: ShortcutCategory::Navigation,
        default_keys: &["shift-home"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::SelectEnd,
        id: "select_end",
        category: ShortcutCategory::Navigation,
        default_keys: &["shift-end"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::SelectAll,
        id: SELECT_ALL_ID,
        category: ShortcutCategory::Edit,
        default_keys: &["cmd-a", "ctrl-a"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::Copy,
        id: "copy",
        category: ShortcutCategory::Edit,
        default_keys: &["cmd-c", "ctrl-c"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::Cut,
        id: "cut",
        category: ShortcutCategory::Edit,
        default_keys: &["cmd-x", "ctrl-x"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::Paste,
        id: "paste",
        category: ShortcutCategory::Edit,
        default_keys: &["cmd-v", "ctrl-v"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::Undo,
        id: "undo",
        category: ShortcutCategory::Edit,
        default_keys: &["cmd-z", "ctrl-z"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::Redo,
        id: "redo",
        category: ShortcutCategory::Edit,
        default_keys: &["cmd-shift-z", "ctrl-y"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::BoldSelection,
        id: "bold_selection",
        category: ShortcutCategory::Formatting,
        default_keys: &["cmd-b", "ctrl-b"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::ItalicSelection,
        id: "italic_selection",
        category: ShortcutCategory::Formatting,
        default_keys: &["cmd-i", "ctrl-i"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::UnderlineSelection,
        id: "underline_selection",
        category: ShortcutCategory::Formatting,
        default_keys: &["cmd-u", "ctrl-u"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::CodeSelection,
        id: "code_selection",
        category: ShortcutCategory::Formatting,
        default_keys: &["cmd-`", "ctrl-`"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::SetLineEndingCrlf,
        id: "set_line_ending_crlf",
        category: ShortcutCategory::Edit,
        default_keys: &[],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::SetLineEndingLf,
        id: "set_line_ending_lf",
        category: ShortcutCategory::Edit,
        default_keys: &[],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::ToggleFinalNewline,
        id: "toggle_final_newline",
        category: ShortcutCategory::Edit,
        default_keys: &[],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::UploadImage,
        id: "upload_image",
        category: ShortcutCategory::Formatting,
        default_keys: &[],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::UploadAllImages,
        id: "upload_all_images",
        category: ShortcutCategory::Formatting,
        default_keys: &[],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::ToggleAutosave,
        id: "toggle_autosave",
        category: ShortcutCategory::File,
        default_keys: &[],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::ShowBackups,
        id: "show_backups",
        category: ShortcutCategory::File,
        default_keys: &[],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::CopyImage,
        id: "copy_image",
        category: ShortcutCategory::Formatting,
        default_keys: &[],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::SetImageRoot,
        id: "set_image_root",
        category: ShortcutCategory::Formatting,
        default_keys: &[],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::InsertImage,
        id: "insert_image",
        category: ShortcutCategory::Formatting,
        default_keys: &["cmd-shift-i", "ctrl-shift-i"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::RevealImageFile,
        id: "reveal_image_file",
        category: ShortcutCategory::Formatting,
        default_keys: &[],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::ToggleImageSyntax,
        id: "toggle_image_syntax",
        category: ShortcutCategory::Formatting,
        default_keys: &[],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::CopyImageTo,
        id: "copy_image_to",
        category: ShortcutCategory::Formatting,
        default_keys: &[],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::MoveImageTo,
        id: "move_image_to",
        category: ShortcutCategory::Formatting,
        default_keys: &[],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::DeleteImageFile,
        id: "delete_image_file",
        category: ShortcutCategory::Formatting,
        default_keys: &[],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::MoveDocument,
        id: "move_document",
        category: ShortcutCategory::File,
        default_keys: &[],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::ShowProperties,
        id: "show_properties",
        category: ShortcutCategory::File,
        default_keys: &[],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::ExportAgain,
        id: "export_again",
        category: ShortcutCategory::File,
        default_keys: &["cmd-shift-e", "ctrl-shift-e"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::ExportHtmlPlain,
        id: "export_html_plain",
        category: ShortcutCategory::File,
        default_keys: &[],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::PrintDocument,
        id: "print_document",
        category: ShortcutCategory::File,
        default_keys: &["alt-shift-p"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::OpenEmojiPicker,
        id: "open_emoji_picker",
        category: ShortcutCategory::Edit,
        default_keys: &[],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::SelectLine,
        id: "select_line",
        category: ShortcutCategory::Edit,
        default_keys: &["cmd-l", "ctrl-l"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::SelectStyledScope,
        id: "select_styled_scope",
        category: ShortcutCategory::Edit,
        default_keys: &["cmd-e", "ctrl-e"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::SelectWord,
        id: "select_word",
        category: ShortcutCategory::Edit,
        default_keys: &["cmd-d", "ctrl-d"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::SelectBlock,
        id: "select_block",
        category: ShortcutCategory::Edit,
        default_keys: &["cmd-alt-p", "ctrl-alt-p"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::JumpToSelection,
        id: "jump_to_selection",
        category: ShortcutCategory::Edit,
        default_keys: &["cmd-j", "ctrl-j"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::DeleteWord,
        id: "delete_word",
        category: ShortcutCategory::Edit,
        default_keys: &["cmd-shift-d", "ctrl-shift-d"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::DeleteLine,
        id: "delete_line",
        category: ShortcutCategory::Edit,
        default_keys: &["cmd-shift-alt-l", "ctrl-shift-alt-l"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::DeleteStyledScope,
        id: "delete_styled_scope",
        category: ShortcutCategory::Edit,
        default_keys: &["cmd-shift-alt-e", "ctrl-shift-alt-e"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::DeleteBlock,
        id: "delete_block",
        category: ShortcutCategory::Edit,
        default_keys: &["cmd-shift-alt-p", "ctrl-shift-alt-p"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::PasteAsPlainText,
        id: "paste_as_plain_text",
        category: ShortcutCategory::Edit,
        default_keys: &["cmd-shift-v", "ctrl-shift-v"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::CopyAsPlainText,
        id: "copy_as_plain_text",
        category: ShortcutCategory::Edit,
        default_keys: &[],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::CopyAsHtml,
        id: "copy_as_html",
        category: ShortcutCategory::Edit,
        default_keys: &[],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::IncreaseHeadingLevel,
        id: "increase_heading_level",
        category: ShortcutCategory::Block,
        default_keys: &["alt-shift-up", "cmd-alt-up"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::DecreaseHeadingLevel,
        id: "decrease_heading_level",
        category: ShortcutCategory::Block,
        default_keys: &["alt-shift-down", "cmd-alt-down"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::ToggleQuote,
        id: "toggle_quote",
        category: ShortcutCategory::Block,
        default_keys: &["cmd-alt-q", "ctrl-shift-q"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::ToggleOrderedList,
        id: "toggle_ordered_list",
        category: ShortcutCategory::Block,
        default_keys: &["cmd-alt-o", "ctrl-shift-["],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::ToggleBulletList,
        id: "toggle_bullet_list",
        category: ShortcutCategory::Block,
        default_keys: &["cmd-alt-u", "ctrl-shift-]"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::ToggleTaskList,
        id: "toggle_task_list",
        category: ShortcutCategory::Block,
        default_keys: &["cmd-alt-x", "ctrl-shift-x"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::ToggleTaskStatus,
        id: "toggle_task_status",
        category: ShortcutCategory::Block,
        default_keys: &[],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::InsertTable,
        id: "insert_table",
        category: ShortcutCategory::Block,
        default_keys: &["cmd-alt-t"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::InsertCodeBlock,
        id: "insert_code_block",
        category: ShortcutCategory::Block,
        default_keys: &["cmd-alt-c", "ctrl-shift-k"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::InsertMathBlock,
        id: "insert_math_block",
        category: ShortcutCategory::Block,
        default_keys: &["cmd-alt-b", "ctrl-shift-m"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::InsertHorizontalRule,
        id: "insert_horizontal_rule",
        category: ShortcutCategory::Block,
        default_keys: &[],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::InsertTableOfContents,
        id: "insert_table_of_contents",
        category: ShortcutCategory::Block,
        default_keys: &[],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::InsertFootnote,
        id: "insert_footnote",
        category: ShortcutCategory::Block,
        default_keys: &[],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::InsertLinkReference,
        id: "insert_link_reference",
        category: ShortcutCategory::Block,
        default_keys: &[],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::InsertFrontMatter,
        id: "insert_front_matter",
        category: ShortcutCategory::Block,
        default_keys: &[],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::StrikethroughSelection,
        id: "strikethrough_selection",
        category: ShortcutCategory::Formatting,
        default_keys: &["ctrl-shift-`", "alt-shift-5"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::HighlightSelection,
        id: "highlight_selection",
        category: ShortcutCategory::Formatting,
        default_keys: &[],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::SuperscriptSelection,
        id: "superscript_selection",
        category: ShortcutCategory::Formatting,
        default_keys: &[],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::SubscriptSelection,
        id: "subscript_selection",
        category: ShortcutCategory::Formatting,
        default_keys: &[],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::InlineMathSelection,
        id: "inline_math_selection",
        category: ShortcutCategory::Formatting,
        default_keys: &[],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::CommentSelection,
        id: "comment_selection",
        category: ShortcutCategory::Formatting,
        default_keys: &[],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::InsertLink,
        id: "insert_link",
        category: ShortcutCategory::Formatting,
        default_keys: &["cmd-k", "ctrl-k"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::ClearFormat,
        id: "clear_format",
        category: ShortcutCategory::Formatting,
        default_keys: &["cmd-\\", "ctrl-\\"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::SetParagraph,
        id: "set_paragraph",
        category: ShortcutCategory::Block,
        default_keys: &["cmd-0", "ctrl-0"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::SetHeading1,
        id: "set_heading_1",
        category: ShortcutCategory::Block,
        default_keys: &["cmd-1", "ctrl-1"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::SetHeading2,
        id: "set_heading_2",
        category: ShortcutCategory::Block,
        default_keys: &["cmd-2", "ctrl-2"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::SetHeading3,
        id: "set_heading_3",
        category: ShortcutCategory::Block,
        default_keys: &["cmd-3", "ctrl-3"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::SetHeading4,
        id: "set_heading_4",
        category: ShortcutCategory::Block,
        default_keys: &["cmd-4", "ctrl-4"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::SetHeading5,
        id: "set_heading_5",
        category: ShortcutCategory::Block,
        default_keys: &["cmd-5", "ctrl-5"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::SetHeading6,
        id: "set_heading_6",
        category: ShortcutCategory::Block,
        default_keys: &["cmd-6", "ctrl-6"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::IndentBlock,
        id: "indent_block",
        category: ShortcutCategory::Block,
        default_keys: &["tab"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::OutdentBlock,
        id: "outdent_block",
        category: ShortcutCategory::Block,
        default_keys: &["shift-tab"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::ExitCodeBlock,
        id: "exit_code_block",
        category: ShortcutCategory::Block,
        default_keys: &["cmd-enter", "ctrl-enter"],
        context: BLOCK_CONTEXT,
    },
    ShortcutDefinition {
        command: ShortcutCommand::SaveDocument,
        id: "save_document",
        category: ShortcutCategory::File,
        default_keys: &["cmd-s", "ctrl-s"],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::SaveDocumentAs,
        id: "save_document_as",
        category: ShortcutCategory::File,
        default_keys: &["cmd-shift-s", "ctrl-shift-s"],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::NewWindow,
        id: "new_window",
        category: ShortcutCategory::File,
        default_keys: &["cmd-n", "ctrl-n"],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::OpenFile,
        id: "open_file",
        category: ShortcutCategory::File,
        default_keys: &["cmd-o", "ctrl-o"],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::QuitApplication,
        id: "quit_application",
        category: ShortcutCategory::File,
        default_keys: QUIT_APPLICATION_DEFAULT_KEYS,
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::CloseWindow,
        id: "close_window",
        category: ShortcutCategory::File,
        default_keys: CLOSE_WINDOW_DEFAULT_KEYS,
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::DismissTransientUi,
        id: "dismiss_transient_ui",
        category: ShortcutCategory::Other,
        default_keys: &["escape"],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::ToggleAutoPair,
        id: "toggle_auto_pair",
        category: ShortcutCategory::Edit,
        default_keys: &[],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::ToggleSmartQuotes,
        id: "toggle_smart_quotes",
        category: ShortcutCategory::Edit,
        default_keys: &[],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::ToggleSmartDashes,
        id: "toggle_smart_dashes",
        category: ShortcutCategory::Edit,
        default_keys: &[],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::ToggleFocusMode,
        id: "toggle_focus_mode",
        category: ShortcutCategory::Navigation,
        default_keys: &["f8"],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::ToggleTypewriterMode,
        id: "toggle_typewriter_mode",
        category: ShortcutCategory::Navigation,
        default_keys: &["f9"],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::ToggleStatusBar,
        id: "toggle_status_bar",
        category: ShortcutCategory::Navigation,
        default_keys: &[],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::ToggleFullScreen,
        id: "toggle_full_screen",
        category: ShortcutCategory::Navigation,
        default_keys: &["f11", "ctrl-cmd-f"],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::ToggleAlwaysOnTop,
        id: "toggle_always_on_top",
        category: ShortcutCategory::Navigation,
        default_keys: &[],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::ResetZoom,
        id: "reset_zoom",
        category: ShortcutCategory::Navigation,
        default_keys: &["cmd-shift-0", "ctrl-shift-0"],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::ZoomIn,
        id: "zoom_in",
        category: ShortcutCategory::Navigation,
        default_keys: &[
            "cmd-+",
            "ctrl-+",
            "cmd-=",
            "ctrl-=",
            "cmd-shift-=",
            "ctrl-shift-=",
        ],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::ZoomOut,
        id: "zoom_out",
        category: ShortcutCategory::Navigation,
        default_keys: &[
            "cmd--",
            "ctrl--",
            "cmd-_",
            "ctrl-_",
            "cmd-shift--",
            "ctrl-shift--",
        ],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::ToggleReadOnly,
        id: "toggle_read_only",
        category: ShortcutCategory::Edit,
        default_keys: &["cmd-alt-r", "ctrl-alt-r"],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::ToggleSpellcheck,
        id: "toggle_spellcheck",
        category: ShortcutCategory::Edit,
        default_keys: &[],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::OpenFolder,
        id: "open_folder",
        category: ShortcutCategory::File,
        default_keys: &["cmd-shift-o", "ctrl-shift-o"],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::ReloadFromDisk,
        id: "reload_from_disk",
        category: ShortcutCategory::File,
        default_keys: &[],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::SaveAll,
        id: "save_all",
        category: ShortcutCategory::File,
        default_keys: &[],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::ReopenClosedTab,
        id: "reopen_closed_tab",
        category: ShortcutCategory::File,
        default_keys: &["cmd-shift-t", "ctrl-shift-t"],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::RevealInFileManager,
        id: "reveal_in_file_manager",
        category: ShortcutCategory::File,
        default_keys: &[],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::RevealInSidebar,
        id: "reveal_in_sidebar",
        category: ShortcutCategory::File,
        default_keys: &[],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::DeleteCurrentFile,
        id: "delete_current_file",
        category: ShortcutCategory::File,
        default_keys: &[],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::ClearRecentFiles,
        id: "clear_recent_files",
        category: ShortcutCategory::File,
        default_keys: &[],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::CloseFolder,
        id: "close_folder",
        category: ShortcutCategory::File,
        default_keys: &[],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::QuickOpen,
        id: "quick_open",
        category: ShortcutCategory::File,
        default_keys: &["cmd-p", "ctrl-p"],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::SearchInWorkspace,
        id: "search_in_workspace",
        category: ShortcutCategory::Edit,
        default_keys: &["cmd-shift-f", "ctrl-shift-f"],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::ToggleViewMode,
        id: "toggle_view_mode",
        category: ShortcutCategory::Navigation,
        default_keys: &["cmd-/", "ctrl-/"],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::ShowFileTree,
        id: "show_file_tree",
        category: ShortcutCategory::Navigation,
        default_keys: &["cmd-shift-3", "ctrl-shift-3"],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::ShowOutline,
        id: "show_outline",
        category: ShortcutCategory::Navigation,
        default_keys: &["cmd-shift-1", "ctrl-shift-1"],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::ShowArticles,
        id: "show_articles",
        category: ShortcutCategory::Navigation,
        default_keys: &["cmd-shift-2", "ctrl-shift-2"],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::ShowLinks,
        id: "show_links",
        category: ShortcutCategory::Navigation,
        default_keys: &["cmd-shift-b", "ctrl-shift-b"],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::ToggleToolbar,
        id: "toggle_toolbar",
        category: ShortcutCategory::Navigation,
        default_keys: &[],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::ToggleWorkspace,
        id: "toggle_workspace",
        category: ShortcutCategory::Navigation,
        default_keys: &["ctrl-w"],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::FindInDocument,
        id: "find_in_document",
        category: ShortcutCategory::Edit,
        default_keys: &["cmd-f", "ctrl-f"],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::FindAndReplace,
        id: "find_and_replace",
        category: ShortcutCategory::Edit,
        default_keys: &["cmd-alt-f", "ctrl-h"],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::FindNext,
        id: "find_next",
        category: ShortcutCategory::Edit,
        default_keys: &["cmd-g", "f3"],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::FindPrevious,
        id: "find_previous",
        category: ShortcutCategory::Edit,
        default_keys: &["cmd-shift-g", "shift-f3"],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::NewTab,
        id: "new_tab",
        category: ShortcutCategory::File,
        default_keys: &["cmd-t", "ctrl-t"],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::CloseTab,
        id: "close_tab",
        category: ShortcutCategory::File,
        default_keys: CLOSE_TAB_DEFAULT_KEYS,
        context: None,
    },
    // Like browsers and Typora: ctrl-tab cycles tabs (ctrl-pagedown/pageup
    // and the macOS cmd-shift-] / [ pair work too).
    ShortcutDefinition {
        command: ShortcutCommand::NextTab,
        id: "next_tab",
        category: ShortcutCategory::Navigation,
        default_keys: &["ctrl-tab", "ctrl-pagedown", "cmd-}"],
        context: None,
    },
    ShortcutDefinition {
        command: ShortcutCommand::PrevTab,
        id: "prev_tab",
        category: ShortcutCategory::Navigation,
        default_keys: &["ctrl-shift-tab", "ctrl-pageup", "cmd-{"],
        context: None,
    },
];

/// Returns the canonical, ordered shortcut catalog.
pub(crate) fn shortcut_definitions() -> &'static [ShortcutDefinition] {
    SHORTCUT_DEFINITIONS
}

/// Parses and deduplicates keystrokes, rejecting invalid, empty, or IME input.
pub(crate) fn normalize_shortcut_keys(keys: &[String]) -> Option<Vec<String>> {
    let mut seen = BTreeSet::new();
    let mut normalized = Vec::new();
    for key in keys {
        let parsed = Keystroke::parse(key.trim()).ok()?;
        if parsed.is_ime_in_progress() {
            return None;
        }
        let key = parsed.unparse();
        if seen.insert(key.clone()) {
            normalized.push(key);
        }
    }
    (!normalized.is_empty()).then_some(normalized)
}

/// Copies a command's default keystrokes into owned strings.
fn default_keys(definition: ShortcutDefinition) -> Vec<String> {
    definition
        .default_keys
        .iter()
        .map(|key| key.to_string())
        .collect()
}

/// Legacy preference keys that should feed a modern shortcut definition.
///
/// Select-all used to be represented by separate source/rendered commands. The
/// editor now cycles those behaviors through one action, so old preferences map
/// forward to `select_all` instead of being silently discarded on load.
fn legacy_shortcut_ids(definition: ShortcutDefinition) -> &'static [&'static str] {
    match definition.command {
        ShortcutCommand::SelectAll => LEGACY_SELECT_ALL_IDS,
        _ => &[],
    }
}

/// Reads a user shortcut override, preferring the current id before aliases.
fn configured_shortcut_keys(
    definition: ShortcutDefinition,
    config: &BTreeMap<String, Vec<String>>,
) -> Option<Vec<String>> {
    config
        .get(definition.id)
        .and_then(|keys| normalize_shortcut_keys(keys))
        .or_else(|| {
            legacy_shortcut_ids(definition).iter().find_map(|id| {
                config
                    .get(*id)
                    .and_then(|keys| normalize_shortcut_keys(keys))
            })
        })
}

/// Finds shared keys only when two commands use the same GPUI context.
fn shortcuts_conflict(
    left: ShortcutDefinition,
    left_keys: &[String],
    right: ShortcutDefinition,
    right_keys: &[String],
) -> bool {
    left.context == right.context && left_keys.iter().any(|key| right_keys.contains(key))
}

/// Keeps valid nondefault overrides and drops conflicting custom bindings.
///
/// Legacy select-all ids are mapped to the current id; conflicting overrides
/// revert to defaults until the effective binding set has no conflicts.
pub(crate) fn normalize_shortcut_config(
    config: &BTreeMap<String, Vec<String>>,
) -> BTreeMap<String, Vec<String>> {
    let mut effective: BTreeMap<&'static str, (bool, Vec<String>)> = BTreeMap::new();
    for definition in SHORTCUT_DEFINITIONS {
        let custom = configured_shortcut_keys(*definition, config);
        effective.insert(
            definition.id,
            match custom {
                Some(keys) if keys != default_keys(*definition) => (true, keys),
                _ => (false, default_keys(*definition)),
            },
        );
    }

    loop {
        let mut conflicted = BTreeSet::new();
        for (index, left) in SHORTCUT_DEFINITIONS.iter().enumerate() {
            let (left_custom, left_keys) = effective.get(left.id).expect("known shortcut");
            for right in SHORTCUT_DEFINITIONS.iter().skip(index + 1) {
                let (right_custom, right_keys) = effective.get(right.id).expect("known shortcut");
                if shortcuts_conflict(*left, left_keys, *right, right_keys) {
                    if *left_custom {
                        conflicted.insert(left.id);
                    }
                    if *right_custom {
                        conflicted.insert(right.id);
                    }
                }
            }
        }

        if conflicted.is_empty() {
            break;
        }

        for id in conflicted {
            if let Some(definition) = SHORTCUT_DEFINITIONS
                .iter()
                .find(|definition| definition.id == id)
            {
                effective.insert(definition.id, (false, default_keys(*definition)));
            }
        }
    }

    effective
        .into_iter()
        .filter_map(|(id, (custom, keys))| custom.then_some((id.to_string(), keys)))
        .collect()
}

/// Returns a command's normalized override or its default keystrokes.
pub(crate) fn resolved_shortcut_keys(
    config: &BTreeMap<String, Vec<String>>,
    command: ShortcutCommand,
) -> Vec<String> {
    let normalized = normalize_shortcut_config(config);
    let definition = SHORTCUT_DEFINITIONS
        .iter()
        .find(|definition| definition.command == command)
        .expect("known shortcut command");
    normalized
        .get(definition.id)
        .cloned()
        .unwrap_or_else(|| default_keys(*definition))
}

/// Finds the first command conflicting with valid proposed keys, if any.
pub(crate) fn shortcut_conflict_for(
    command: ShortcutCommand,
    proposed_keys: &[String],
    config: &BTreeMap<String, Vec<String>>,
) -> Option<ShortcutDefinition> {
    let definition = SHORTCUT_DEFINITIONS
        .iter()
        .find(|definition| definition.command == command)?;
    let proposed_keys = normalize_shortcut_keys(proposed_keys)?;
    for other in SHORTCUT_DEFINITIONS
        .iter()
        .filter(|other| other.command != command)
    {
        let other_keys = resolved_shortcut_keys(config, other.command);
        if shortcuts_conflict(*definition, &proposed_keys, *other, &other_keys) {
            return Some(*other);
        }
    }
    None
}

/// Constructs the GPUI key binding for a command and context.
fn key_binding_for(
    command: ShortcutCommand,
    key: &str,
    context: Option<&'static str>,
) -> KeyBinding {
    match command {
        ShortcutCommand::Newline => KeyBinding::new(key, Newline, context),
        ShortcutCommand::DeleteBack => KeyBinding::new(key, DeleteBack, context),
        ShortcutCommand::Delete => KeyBinding::new(key, Delete, context),
        ShortcutCommand::WordDeleteBack => KeyBinding::new(key, WordDeleteBack, context),
        ShortcutCommand::WordDeleteForward => KeyBinding::new(key, WordDeleteForward, context),
        ShortcutCommand::FocusPrev => KeyBinding::new(key, FocusPrev, context),
        ShortcutCommand::FocusNext => KeyBinding::new(key, FocusNext, context),
        ShortcutCommand::MoveLeft => KeyBinding::new(key, MoveLeft, context),
        ShortcutCommand::MoveRight => KeyBinding::new(key, MoveRight, context),
        ShortcutCommand::WordMoveLeft => KeyBinding::new(key, WordMoveLeft, context),
        ShortcutCommand::WordMoveRight => KeyBinding::new(key, WordMoveRight, context),
        ShortcutCommand::Home => KeyBinding::new(key, Home, context),
        ShortcutCommand::End => KeyBinding::new(key, End, context),
        ShortcutCommand::BlockUp => KeyBinding::new(key, BlockUp, context),
        ShortcutCommand::BlockDown => KeyBinding::new(key, BlockDown, context),
        ShortcutCommand::PageUp => KeyBinding::new(key, PageUp, context),
        ShortcutCommand::PageDown => KeyBinding::new(key, PageDown, context),
        ShortcutCommand::JumpToTop => KeyBinding::new(key, JumpToTop, context),
        ShortcutCommand::JumpToBottom => KeyBinding::new(key, JumpToBottom, context),
        ShortcutCommand::SelectLeft => KeyBinding::new(key, SelectLeft, context),
        ShortcutCommand::SelectRight => KeyBinding::new(key, SelectRight, context),
        ShortcutCommand::WordSelectLeft => KeyBinding::new(key, WordSelectLeft, context),
        ShortcutCommand::WordSelectRight => KeyBinding::new(key, WordSelectRight, context),
        ShortcutCommand::SelectHome => KeyBinding::new(key, SelectHome, context),
        ShortcutCommand::SelectEnd => KeyBinding::new(key, SelectEnd, context),
        ShortcutCommand::SelectAll => KeyBinding::new(key, SelectAll, context),
        ShortcutCommand::Copy => KeyBinding::new(key, Copy, context),
        ShortcutCommand::Cut => KeyBinding::new(key, Cut, context),
        ShortcutCommand::Paste => KeyBinding::new(key, Paste, context),
        ShortcutCommand::Undo => KeyBinding::new(key, Undo, context),
        ShortcutCommand::Redo => KeyBinding::new(key, Redo, context),
        ShortcutCommand::BoldSelection => KeyBinding::new(key, BoldSelection, context),
        ShortcutCommand::ItalicSelection => KeyBinding::new(key, ItalicSelection, context),
        ShortcutCommand::UnderlineSelection => KeyBinding::new(key, UnderlineSelection, context),
        ShortcutCommand::CodeSelection => KeyBinding::new(key, CodeSelection, context),
        ShortcutCommand::SetLineEndingCrlf => KeyBinding::new(key, SetLineEndingCrlf, context),
        ShortcutCommand::SetLineEndingLf => KeyBinding::new(key, SetLineEndingLf, context),
        ShortcutCommand::ToggleFinalNewline => KeyBinding::new(key, ToggleFinalNewline, context),
        ShortcutCommand::PrintDocument => KeyBinding::new(key, PrintDocument, context),
        ShortcutCommand::ExportAgain => KeyBinding::new(key, ExportAgain, context),
        ShortcutCommand::CopyImage => KeyBinding::new(key, CopyImage, context),
        ShortcutCommand::SetImageRoot => KeyBinding::new(key, SetImageRoot, context),
        ShortcutCommand::InsertImage => KeyBinding::new(key, InsertImage, context),
        ShortcutCommand::RevealImageFile => KeyBinding::new(key, RevealImageFile, context),
        ShortcutCommand::ToggleImageSyntax => KeyBinding::new(key, ToggleImageSyntax, context),
        ShortcutCommand::CopyImageTo => KeyBinding::new(key, CopyImageTo, context),
        ShortcutCommand::MoveImageTo => KeyBinding::new(key, MoveImageTo, context),
        ShortcutCommand::DeleteImageFile => KeyBinding::new(key, DeleteImageFile, context),
        ShortcutCommand::MoveDocument => KeyBinding::new(key, MoveDocument, context),
        ShortcutCommand::ShowProperties => KeyBinding::new(key, ShowProperties, context),
        ShortcutCommand::ExportHtmlPlain => KeyBinding::new(key, ExportHtmlPlain, context),
        ShortcutCommand::ToggleAutosave => KeyBinding::new(key, ToggleAutosave, context),
        ShortcutCommand::ShowBackups => KeyBinding::new(key, ShowBackups, context),
        ShortcutCommand::UploadImage => KeyBinding::new(key, UploadImage, context),
        ShortcutCommand::UploadAllImages => KeyBinding::new(key, UploadAllImages, context),
        ShortcutCommand::OpenEmojiPicker => KeyBinding::new(key, OpenEmojiPicker, context),
        ShortcutCommand::SelectLine => KeyBinding::new(key, SelectLine, context),
        ShortcutCommand::SelectStyledScope => KeyBinding::new(key, SelectStyledScope, context),
        ShortcutCommand::SelectWord => KeyBinding::new(key, SelectWord, context),
        ShortcutCommand::SelectBlock => KeyBinding::new(key, SelectBlock, context),
        ShortcutCommand::JumpToSelection => KeyBinding::new(key, JumpToSelection, context),
        ShortcutCommand::DeleteWord => KeyBinding::new(key, DeleteWord, context),
        ShortcutCommand::DeleteLine => KeyBinding::new(key, DeleteLine, context),
        ShortcutCommand::DeleteStyledScope => KeyBinding::new(key, DeleteStyledScope, context),
        ShortcutCommand::DeleteBlock => KeyBinding::new(key, DeleteBlock, context),
        ShortcutCommand::PasteAsPlainText => KeyBinding::new(key, PasteAsPlainText, context),
        ShortcutCommand::CopyAsPlainText => KeyBinding::new(key, CopyAsPlainText, context),
        ShortcutCommand::CopyAsHtml => KeyBinding::new(key, CopyAsHtml, context),
        ShortcutCommand::IncreaseHeadingLevel => {
            KeyBinding::new(key, IncreaseHeadingLevel, context)
        }
        ShortcutCommand::DecreaseHeadingLevel => {
            KeyBinding::new(key, DecreaseHeadingLevel, context)
        }
        ShortcutCommand::ToggleQuote => KeyBinding::new(key, ToggleQuote, context),
        ShortcutCommand::ToggleOrderedList => KeyBinding::new(key, ToggleOrderedList, context),
        ShortcutCommand::ToggleBulletList => KeyBinding::new(key, ToggleBulletList, context),
        ShortcutCommand::ToggleTaskList => KeyBinding::new(key, ToggleTaskList, context),
        ShortcutCommand::ToggleTaskStatus => KeyBinding::new(key, ToggleTaskStatus, context),
        ShortcutCommand::InsertTable => KeyBinding::new(key, InsertTable, context),
        ShortcutCommand::InsertCodeBlock => KeyBinding::new(key, InsertCodeBlock, context),
        ShortcutCommand::InsertMathBlock => KeyBinding::new(key, InsertMathBlock, context),
        ShortcutCommand::InsertHorizontalRule => {
            KeyBinding::new(key, InsertHorizontalRule, context)
        }
        ShortcutCommand::InsertTableOfContents => {
            KeyBinding::new(key, InsertTableOfContents, context)
        }
        ShortcutCommand::InsertFootnote => KeyBinding::new(key, InsertFootnote, context),
        ShortcutCommand::InsertLinkReference => KeyBinding::new(key, InsertLinkReference, context),
        ShortcutCommand::InsertFrontMatter => KeyBinding::new(key, InsertFrontMatter, context),
        ShortcutCommand::StrikethroughSelection => {
            KeyBinding::new(key, StrikethroughSelection, context)
        }
        ShortcutCommand::HighlightSelection => KeyBinding::new(key, HighlightSelection, context),
        ShortcutCommand::SuperscriptSelection => {
            KeyBinding::new(key, SuperscriptSelection, context)
        }
        ShortcutCommand::SubscriptSelection => KeyBinding::new(key, SubscriptSelection, context),
        ShortcutCommand::InlineMathSelection => KeyBinding::new(key, InlineMathSelection, context),
        ShortcutCommand::CommentSelection => KeyBinding::new(key, CommentSelection, context),
        ShortcutCommand::InsertLink => KeyBinding::new(key, InsertLink, context),
        ShortcutCommand::ClearFormat => KeyBinding::new(key, ClearFormat, context),
        ShortcutCommand::SetParagraph => KeyBinding::new(key, SetParagraph, context),
        ShortcutCommand::SetHeading1 => KeyBinding::new(key, SetHeading1, context),
        ShortcutCommand::SetHeading2 => KeyBinding::new(key, SetHeading2, context),
        ShortcutCommand::SetHeading3 => KeyBinding::new(key, SetHeading3, context),
        ShortcutCommand::SetHeading4 => KeyBinding::new(key, SetHeading4, context),
        ShortcutCommand::SetHeading5 => KeyBinding::new(key, SetHeading5, context),
        ShortcutCommand::SetHeading6 => KeyBinding::new(key, SetHeading6, context),
        ShortcutCommand::IndentBlock => KeyBinding::new(key, IndentBlock, context),
        ShortcutCommand::OutdentBlock => KeyBinding::new(key, OutdentBlock, context),
        ShortcutCommand::ExitCodeBlock => KeyBinding::new(key, ExitCodeBlock, context),
        ShortcutCommand::SaveDocument => KeyBinding::new(key, SaveDocument, context),
        ShortcutCommand::SaveDocumentAs => KeyBinding::new(key, SaveDocumentAs, context),
        ShortcutCommand::NewWindow => KeyBinding::new(key, NewWindow, context),
        ShortcutCommand::OpenFile => KeyBinding::new(key, OpenFile, context),
        ShortcutCommand::QuitApplication => KeyBinding::new(key, QuitApplication, context),
        ShortcutCommand::CloseWindow => KeyBinding::new(key, CloseWindow, context),
        ShortcutCommand::DismissTransientUi => KeyBinding::new(key, DismissTransientUi, context),
        ShortcutCommand::ToggleAutoPair => KeyBinding::new(key, ToggleAutoPair, context),
        ShortcutCommand::ToggleSmartQuotes => KeyBinding::new(key, ToggleSmartQuotes, context),
        ShortcutCommand::ToggleSmartDashes => KeyBinding::new(key, ToggleSmartDashes, context),
        ShortcutCommand::ToggleFocusMode => KeyBinding::new(key, ToggleFocusMode, context),
        ShortcutCommand::ToggleTypewriterMode => {
            KeyBinding::new(key, ToggleTypewriterMode, context)
        }
        ShortcutCommand::ToggleStatusBar => KeyBinding::new(key, ToggleStatusBar, context),
        ShortcutCommand::ToggleFullScreen => KeyBinding::new(key, ToggleFullScreen, context),
        ShortcutCommand::ToggleAlwaysOnTop => KeyBinding::new(key, ToggleAlwaysOnTop, context),
        ShortcutCommand::ResetZoom => KeyBinding::new(key, ResetZoom, context),
        ShortcutCommand::ZoomIn => KeyBinding::new(key, ZoomIn, context),
        ShortcutCommand::ZoomOut => KeyBinding::new(key, ZoomOut, context),
        ShortcutCommand::ToggleReadOnly => KeyBinding::new(key, ToggleReadOnly, context),
        ShortcutCommand::ToggleSpellcheck => KeyBinding::new(key, ToggleSpellcheck, context),
        ShortcutCommand::OpenFolder => KeyBinding::new(key, OpenFolder, context),
        ShortcutCommand::CloseFolder => KeyBinding::new(key, CloseFolder, context),
        ShortcutCommand::ReloadFromDisk => KeyBinding::new(key, ReloadFromDisk, context),
        ShortcutCommand::SaveAll => KeyBinding::new(key, SaveAll, context),
        ShortcutCommand::ReopenClosedTab => KeyBinding::new(key, ReopenClosedTab, context),
        ShortcutCommand::RevealInFileManager => KeyBinding::new(key, RevealInFileManager, context),
        ShortcutCommand::RevealInSidebar => KeyBinding::new(key, RevealInSidebar, context),
        ShortcutCommand::DeleteCurrentFile => KeyBinding::new(key, DeleteCurrentFile, context),
        ShortcutCommand::ClearRecentFiles => KeyBinding::new(key, ClearRecentFiles, context),
        ShortcutCommand::QuickOpen => KeyBinding::new(key, QuickOpen, context),
        ShortcutCommand::SearchInWorkspace => KeyBinding::new(key, SearchInWorkspace, context),
        ShortcutCommand::ToggleViewMode => KeyBinding::new(key, ToggleViewMode, context),
        ShortcutCommand::ShowFileTree => KeyBinding::new(key, ShowFileTree, context),
        ShortcutCommand::ShowOutline => KeyBinding::new(key, ShowOutline, context),
        ShortcutCommand::ShowArticles => KeyBinding::new(key, ShowArticles, context),
        ShortcutCommand::ShowLinks => KeyBinding::new(key, ShowLinks, context),
        ShortcutCommand::ToggleToolbar => KeyBinding::new(key, ToggleToolbar, context),
        ShortcutCommand::ToggleWorkspace => KeyBinding::new(key, ToggleWorkspace, context),
        ShortcutCommand::FindInDocument => KeyBinding::new(key, FindInDocument, context),
        ShortcutCommand::FindAndReplace => KeyBinding::new(key, FindAndReplace, context),
        ShortcutCommand::FindNext => KeyBinding::new(key, FindNext, context),
        ShortcutCommand::FindPrevious => KeyBinding::new(key, FindPrevious, context),
        ShortcutCommand::NewTab => KeyBinding::new(key, NewTab, context),
        ShortcutCommand::CloseTab => KeyBinding::new(key, CloseTab, context),
        ShortcutCommand::NextTab => KeyBinding::new(key, NextTab, context),
        ShortcutCommand::PrevTab => KeyBinding::new(key, PrevTab, context),
    }
}

/// Builds GPUI bindings from normalized custom keys and defaults.
pub(crate) fn resolved_keybindings(config: &BTreeMap<String, Vec<String>>) -> Vec<KeyBinding> {
    let normalized = normalize_shortcut_config(config);
    let mut bindings = Vec::new();
    for definition in SHORTCUT_DEFINITIONS {
        let keys = normalized
            .get(definition.id)
            .cloned()
            .unwrap_or_else(|| default_keys(*definition));
        bindings.extend(
            keys.iter()
                .map(|key| key_binding_for(definition.command, key, definition.context)),
        );
    }
    bindings
}

/// Installs the resolved shortcut set in GPUI.
pub(crate) fn install_keybindings(cx: &mut App, config: &BTreeMap<String, Vec<String>>) {
    cx.bind_keys(resolved_keybindings(config));
}

/// Register key bindings for the block editor.
#[allow(dead_code)]
pub fn init(cx: &mut App) {
    install_keybindings(cx, &BTreeMap::new());
}

/// Initializes action bindings with the supplied shortcut preferences.
pub(crate) fn init_with_keybindings(cx: &mut App, config: &BTreeMap<String, Vec<String>>) {
    install_keybindings(cx, config);
}

#[cfg(test)]
mod tests {
    use super::{
        ShortcutCommand, normalize_shortcut_config, resolved_shortcut_keys, shortcut_conflict_for,
    };
    use std::collections::BTreeMap;

    #[test]
    fn custom_shortcut_replaces_command_defaults() {
        let mut config = BTreeMap::new();
        config.insert("save_document".to_string(), vec!["ctrl-alt-s".to_string()]);

        assert_eq!(
            resolved_shortcut_keys(&config, ShortcutCommand::SaveDocument),
            vec!["ctrl-alt-s".to_string()]
        );
    }

    #[test]
    fn toggle_view_mode_has_default_shortcuts() {
        assert_eq!(
            resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::ToggleViewMode),
            vec!["cmd-/".to_string(), "ctrl-/".to_string()]
        );
    }

    #[test]
    fn zoom_heading_and_view_shortcuts_do_not_conflict() {
        let config = BTreeMap::new();
        assert!(
            resolved_shortcut_keys(&config, ShortcutCommand::ZoomOut)
                .contains(&"ctrl--".to_string())
        );
        assert!(
            resolved_shortcut_keys(&config, ShortcutCommand::ZoomIn)
                .contains(&"ctrl-+".to_string())
        );
        for command in [
            ShortcutCommand::ZoomIn,
            ShortcutCommand::ZoomOut,
            ShortcutCommand::ResetZoom,
            ShortcutCommand::ToggleViewMode,
            ShortcutCommand::IncreaseHeadingLevel,
            ShortcutCommand::DecreaseHeadingLevel,
            ShortcutCommand::SetParagraph,
        ] {
            let keys = resolved_shortcut_keys(&config, command);
            assert!(
                shortcut_conflict_for(command, &keys, &config).is_none(),
                "{command:?} default keys must not collide with another shortcut"
            );
        }
    }

    #[test]
    fn toggle_workspace_has_default_shortcuts() {
        assert_eq!(
            resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::ToggleWorkspace),
            vec!["ctrl-w".to_string()]
        );
    }

    #[test]
    fn tab_shortcuts_have_defaults_that_do_not_conflict() {
        let config = BTreeMap::new();
        assert_eq!(
            resolved_shortcut_keys(&config, ShortcutCommand::NewTab),
            vec!["cmd-t".to_string(), "ctrl-t".to_string()]
        );
        assert_eq!(
            resolved_shortcut_keys(&config, ShortcutCommand::NextTab),
            vec![
                "ctrl-tab".to_string(),
                "ctrl-pagedown".to_string(),
                "cmd-}".to_string()
            ]
        );
        assert_eq!(
            resolved_shortcut_keys(&config, ShortcutCommand::PrevTab),
            vec![
                "ctrl-shift-tab".to_string(),
                "ctrl-pageup".to_string(),
                "cmd-{".to_string()
            ]
        );
        for command in [
            ShortcutCommand::NewTab,
            ShortcutCommand::CloseTab,
            ShortcutCommand::NextTab,
            ShortcutCommand::PrevTab,
        ] {
            let keys = resolved_shortcut_keys(&config, command);
            assert!(
                shortcut_conflict_for(command, &keys, &config).is_none(),
                "{command:?} default keys must not collide with another shortcut"
            );
        }
    }

    #[test]
    fn select_all_has_default_shortcuts() {
        assert_eq!(
            resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::SelectAll),
            vec!["cmd-a".to_string(), "ctrl-a".to_string()]
        );
        assert!(
            shortcut_conflict_for(
                ShortcutCommand::SelectAll,
                &["cmd-a".to_string(), "ctrl-a".to_string()],
                &BTreeMap::new()
            )
            .is_none()
        );
    }

    #[test]
    fn select_all_shortcut_can_be_customized() {
        let mut config = BTreeMap::new();
        config.insert("select_all".to_string(), vec!["ctrl-shift-a".to_string()]);

        assert_eq!(
            resolved_shortcut_keys(&config, ShortcutCommand::SelectAll),
            vec!["ctrl-shift-a".to_string()]
        );
    }

    #[test]
    fn legacy_split_select_all_shortcut_config_maps_to_unified_command() {
        let mut config = BTreeMap::new();
        config.insert(
            "select_all_source_text".to_string(),
            vec!["ctrl-shift-a".to_string()],
        );

        assert_eq!(
            resolved_shortcut_keys(&config, ShortcutCommand::SelectAll),
            vec!["ctrl-shift-a".to_string()]
        );

        let normalized = normalize_shortcut_config(&config);
        assert_eq!(
            normalized.get("select_all"),
            Some(&vec!["ctrl-shift-a".to_string()])
        );
        assert!(!normalized.contains_key("select_all_source_text"));
        assert!(!normalized.contains_key("select_focused_block_text_rendered"));

        config.clear();
        config.insert(
            "select_focused_block_text_rendered".to_string(),
            vec!["ctrl-alt-shift-a".to_string()],
        );

        assert_eq!(
            resolved_shortcut_keys(&config, ShortcutCommand::SelectAll),
            vec!["ctrl-alt-shift-a".to_string()]
        );

        let normalized = normalize_shortcut_config(&config);
        assert_eq!(
            normalized.get("select_all"),
            Some(&vec!["ctrl-alt-shift-a".to_string()])
        );
        assert!(!normalized.contains_key("select_all_source_text"));
        assert!(!normalized.contains_key("select_focused_block_text_rendered"));
    }

    #[test]
    fn close_and_quit_defaults_are_platform_specific() {
        #[cfg(target_os = "macos")]
        {
            assert_eq!(
                resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::CloseWindow),
                vec!["cmd-shift-w".to_string()]
            );
            assert_eq!(
                resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::CloseTab),
                vec!["cmd-w".to_string()]
            );
            assert_eq!(
                resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::QuitApplication),
                vec!["cmd-q".to_string()]
            );
        }

        #[cfg(not(target_os = "macos"))]
        {
            assert_eq!(
                resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::CloseWindow),
                vec!["ctrl-q".to_string()]
            );
            assert_eq!(
                resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::CloseTab),
                vec!["ctrl-f4".to_string()]
            );
            assert!(
                resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::QuitApplication)
                    .is_empty()
            );
        }
    }

    #[test]
    fn word_and_block_shortcuts_have_ctrl_and_alt_defaults() {
        assert_eq!(
            resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::WordMoveLeft),
            vec!["ctrl-left".to_string(), "alt-left".to_string()]
        );
        assert_eq!(
            resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::WordDeleteBack),
            vec!["ctrl-backspace".to_string(), "alt-backspace".to_string()]
        );
        assert_eq!(
            resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::BlockUp),
            vec!["ctrl-up".to_string(), "alt-up".to_string()]
        );
        assert_eq!(
            resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::WordSelectRight),
            vec![
                "ctrl-shift-right".to_string(),
                "alt-shift-right".to_string()
            ]
        );
    }

    #[test]
    fn page_navigation_shortcuts_have_defaults() {
        assert_eq!(
            resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::PageUp),
            vec!["pageup".to_string()]
        );
        assert_eq!(
            resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::PageDown),
            vec!["pagedown".to_string()]
        );
        assert_eq!(
            resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::JumpToTop),
            vec!["ctrl-home".to_string(), "cmd-up".to_string()]
        );
        assert_eq!(
            resolved_shortcut_keys(&BTreeMap::new(), ShortcutCommand::JumpToBottom),
            vec!["ctrl-end".to_string(), "cmd-down".to_string()]
        );
    }

    #[test]
    fn invalid_or_empty_shortcuts_fall_back_to_defaults() {
        let mut config = BTreeMap::new();
        config.insert("save_document".to_string(), vec!["".to_string()]);
        config.insert("open_file".to_string(), vec!["a".to_string()]);

        let normalized = normalize_shortcut_config(&config);
        assert!(!normalized.contains_key("save_document"));
        assert!(!normalized.contains_key("open_file"));
    }

    #[test]
    fn conflicting_custom_shortcut_falls_back_to_default() {
        let mut config = BTreeMap::new();
        config.insert("copy".to_string(), vec!["ctrl-x".to_string()]);

        let normalized = normalize_shortcut_config(&config);
        assert!(!normalized.contains_key("copy"));
        assert_eq!(
            resolved_shortcut_keys(&config, ShortcutCommand::Copy),
            vec!["cmd-c".to_string(), "ctrl-c".to_string()]
        );
    }

    #[test]
    fn detects_shortcut_conflicts_for_preferences_drafts() {
        let conflict = shortcut_conflict_for(
            ShortcutCommand::Copy,
            &["ctrl-x".to_string()],
            &BTreeMap::new(),
        )
        .expect("copy should conflict with cut");

        assert_eq!(conflict.id, "cut");
    }
}
