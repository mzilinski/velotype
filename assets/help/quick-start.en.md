# Velotype – Quick Start

Velotype is a Markdown editor that shows your formatting while you type. You write Markdown and see finished headings, lists, tables and formulas.

> This document is read-only. **View → Read-Only Mode** unlocks it for trying things out; changes are not saved.

## Writing

- Just type Markdown: `# ` at the start of a line becomes a heading, `- ` a list, `> ` a quote, `**bold**` turns bold.
- When the cursor is inside a formatted span, Velotype shows its Markdown characters there so you can edit them.
- **Ctrl+B**, **Ctrl+I**, **Ctrl+U** make the selection bold, italic or underlined; **Ctrl+K** turns it into a link (a URL on the clipboard is used).
- **Ctrl+1** to **Ctrl+6** turn the paragraph into a heading, **Ctrl+0** back into a paragraph.
- The **Paragraph** and **Format** menus (and a **right click**) contain every command with its shortcut.

## Source and view

- **Ctrl+/** switches between the rendered view and the plain Markdown source (also **View → Source Code Mode** and the bottom left of the status bar).
- **View → Focus Mode** (F8) dims everything but the current paragraph, **Typewriter Mode** (F9) keeps the cursor line in the middle of the window.
- **Ctrl++** / **Ctrl+-** make the document text larger or smaller, **Ctrl+Shift+0** restores the actual size.
- **View → Toolbar** shows a row of buttons for the most used commands.

## Files and folders

- **File → Open Folder** shows a folder in the sidebar (**Ctrl+W** shows and hides it). Its tabs show the **file tree**, the document's **outline** and the folder's **articles** (Ctrl+Shift+3, 1, 2).
- Right click in the file tree to create, rename or delete (to the trash) files and folders.
- **Ctrl+P** quickly opens a file of the folder, **Ctrl+Shift+F** searches (and replaces) in all files of the folder, also with regular expressions.
- Several documents open as **tabs**; **Ctrl+Tab** switches between them.

## Saving and safety

- **Ctrl+S** saves. Velotype writes unchanged parts back exactly as they were in the file.
- **File → Auto Save** saves documents shortly after every change.
- Before a file is overwritten, Velotype keeps a **backup** (**File → Show Backups**).
- Untitled documents are kept as drafts when you quit and reopen on the next start.

## Images

- Paste images, drag them in, or pick a file with **Ctrl+Shift+I**. Where pasted images are copied to is set in **Preferences → Image**.
- **Format → Image** and a right click on an image offer size, syntax, copying and moving the image file, and uploading with your own command (for example PicGo).

## Export and printing

- **Export** writes HTML (with or without styles) and PDF; with [Pandoc](https://pandoc.org) installed also Word, OpenDocument, LaTeX, EPUB and more. **Ctrl+Shift+E** repeats the last export.
- **File → Print…** (Alt+Shift+P) opens the document ready to print in the browser.
- **File → Import…** converts Word, OpenDocument and other files to Markdown with Pandoc.

## Appearance

- The **Theme** menu switches the color scheme; Velotype can follow the system's light or dark appearance.
- Put Typora themes (`.css`) and your own Velotype themes into the folder that **Theme → Open Theme Folder** shows.
- Fonts, status bar, shortcuts and more are in **File → Preferences**.

## More

- **Help → Markdown Reference** shows every formatting with examples.
- **Help → Keyboard Shortcuts** lists every command with its current shortcut.
