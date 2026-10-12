# Velotype – Handoff (Stand 2026-10-12)

Ziel: Velotype (Rust/GPUI-Markdown-Editor) an Typora heranführen. Gearbeitet wird im Fork
`origin = github.com/mzilinski/velotype`. `upstream = manyougz/velotype` bleibt unberührt,
PRs dorthin nur auf ausdrücklichen Wunsch.

## Arbeitsweise

- Jedes Feature kommt auf einen eigenen Branch, abgezweigt von `dev`. Der Branch `integration` führt alle zusammen und ist der Stand zum Testen.
- **Push nur nach expliziter Freigabe** durch Malte.
- Jede Commit-Message endet mit `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`.
- Test-Builds: `cargo build --release`, dann die Exe kopieren nach
  `C:\Users\mzili\Developer\velotype-builds\velotype-integration-N.exe`. Bei jedem Build wird N hochgezählt, weil eine laufende Exe gesperrt ist.
- Tests mit `cargo test`. Zuletzt liefen 921 grün; ein Netzwerk-Test ist mit `#[ignore]` markiert und läuft nur mit `-- --ignored`.
- `.claude/` steht in `.git/info/exclude`, damit Agent-Worktrees nicht mit committet werden.

## Offener Punkt

Alles ist gepusht (Stand 2026-10-12): `integration` enthält alle Feature-Branches. Test-Exe unter Windows: `velotype-integration-8.exe`.
Die neuen Features sind nur per Unit-/gpui-Tests geprüft, nicht von Hand in der App.

**CI** (`.github/workflows/ci.yml`, `gh run list`): mit dem Test-Fix grün auf Linux, macOS und Windows (Lauf 38224033844 auf `fix/ci-segfault`).
Ursache des SIGSEGV: der Upstream-Test `editor::selection::tests::cross_block_cut_writes_markdown_deletes_range_and_undo_restores`
rief als einziger Fenster-Test `cx.quit()` auf dem von `add_window_view` zurückgegebenen `VisualTestContext` auf (Use-after-free in gpui,
Details in der Mac-Runde, Punkt 1). Fix (5919c4b auf `fix/unix-test-segfault`, in `integration` gemergt): `quit()` läuft über den
ursprünglichen `TestAppContext`. Der parallele Versuch 775ebad auf `fix/ci-segfault` (`#[gpui::test]`) umgeht dieselbe Ursache und ist
überholt; `fix/ci-segfault` nicht mergen, sondern löschen (`git push origin --delete fix/ci-segfault`).
**Nicht gepusht:** `feat/selection-commands` und `integration` (Merge). Test-Exe: `velotype-integration-10.exe`.

## Mac-Runde

Ziel: einmal von Hand prüfen, was nur auf macOS anders läuft (der SIGSEGV ist bereits behoben).
Entwickelt wird danach weiter unter Windows. Funde und Fixes kommen hier in die Datei und auf eigene Branches.

**Einrichtung** (auf dem Mac erledigt: `~/Developer/velotype`, `upstream` eingetragen, Excludes gesetzt)
```
xcode-select --install                     # falls noch nicht da
curl https://sh.rustup.rs -sSf | sh -s -- -y && source ~/.cargo/env
git clone https://github.com/mzilinski/velotype.git && cd velotype
git switch integration
git show origin/handoff:HANDOFF.md > HANDOFF.md && echo HANDOFF.md >> .git/info/exclude && echo .claude/ >> .git/info/exclude
git remote add upstream https://github.com/manyougz/velotype.git
cargo build
```
`HANDOFF.md` liegt nur auf dem Branch `handoff` (enthält nur diese Datei) und ist lokal ausgeschlossen.
Nach Änderungen zurückspielen: `git switch handoff` geht nicht wegen der ausgeschlossenen Datei; stattdessen
`git worktree add ../velotype-handoff handoff`, Datei hineinkopieren, dort committen und pushen.

**1. SIGSEGV – behoben** (Mac, 2026-10-12; Branch `fix/unix-test-segfault`, in `integration` gemerged)
- Lokal reproduziert mit `cargo test -- --test-threads=1`: der Test stürzt erst in `cx.quit()` am Ende ab, alle Assertions davor
  waren grün. Ein Minimaltest (leeres Fenster + `cx.quit()`) stürzt genauso ab.
- Ursache ist ein Use-after-free in gpui 0.2.2, kein Velotype-Code: `TestAppContext::add_window_view` gibt einen geleakten
  `&mut VisualTestContext` zurück (`into_mut`) und registriert einen `on_quit`-Callback, der genau diese Box freigibt.
  `TestAppContext::quit` ruft erst die `on_quit`-Callbacks und greift danach auf `self.app` zu. Ruft man `quit()` über den
  `VisualTestContext` auf (Deref), ist `self` dann schon freigegeben. Der macOS-Allocator überschreibt freigegebene Blöcke
  (Crash in `AppCell::borrow_mut`, Adresse 0x5/0x15), Windows lässt den Inhalt stehen, deshalb dort grün.
- Regel für neue Tests: nach `add_window_view` nie `cx.quit()` auf dem zurückgegebenen `cx`; entweder weglassen (wie in
  `src/editor/tests.rs`) oder den Original-Kontext unter anderem Namen behalten und dort `quit()` aufrufen.
- Ergebnis auf dem Mac (macOS 27.0.1, Rust 1.99): `cargo test` 908 grün, 1 ignoriert, auch mit `--test-threads=1`.
  Clippy-Warnungen (7) sind vorbestehend. `cargo build --release` läuft, die App startet.
- Debug-Tipp: lldb im Batch-Modus führt `-o`-Befehle nach dem Absturz nicht mehr aus, `-k 'bt'` nimmt. Alternativ liefert der
  macOS-Crash-Report (`~/Library/Logs/DiagnosticReports/velotype-*.ips`) den Stack.

**2. Von Hand prüfen** (`cargo run --release`, Testdatei z. B. `test.md`)
- Native Menüleiste: Velotype, Datei, Bearbeiten, Absatz, Format, Export, Sprache, Theme, Arbeitsbereich, Ansicht, Hilfe.
  Kommen Menübefehle beim Text an (z. B. Absatz → Zitat, Format → Durchgestrichen mit Auswahl)?
- Kürzel: Cmd+B/I/U, Cmd+1…6, Cmd+0, Cmd+= / Cmd+-, Cmd+Alt+Q (Zitat), Cmd+Alt+O/U/X (Listen), Cmd+Alt+C (Code),
  Cmd+Alt+B (Mathe), Cmd+Alt+T (Tabelle), Cmd+K (Link), Cmd+\ (Formatierung entfernen), Ctrl+Shift+` (Durchgestrichen),
  Cmd+Shift+V (Als Text einfügen), Cmd+P, Cmd+Shift+F, Cmd+Shift+O (Ordner öffnen), Cmd+T/W (Tabs).
- Zwischenablage: Text mit Formatierung aus Safari einfügen (wird Markdown?), Code aus einem Editor (bleibt wörtlich?),
  Als HTML kopieren → in TextEdit/Pages einfügen (formatiert?). Bild aus Vorschau einfügen und Bilddatei aus Finder ziehen.
- PDF-Export (findet er Chrome/Edge/Brave?), Theme „Systemdarstellung folgen“ beim Umschalten Hell/Dunkel,
  Ordner öffnen + Neustart (wird der Ordner wiederhergestellt?), Rechtschreibprüfung (ohne Wörterbuch keine Prüfung).
- Ergebnis je Punkt hier eintragen (ok / Fehler mit Beschreibung).

## Erledigt

| Branch | Inhalt |
|---|---|
| fix/single-line-input-ctrl | Ctrl statt Cmd für Clipboard und Select-All in Eingabefeldern (Windows/Linux) |
| feat/find-replace | Suchen-und-Ersetzen-Leiste (Strg+F / Strg+H), Treffer-Highlights, Suche auf Clean-Text |
| feat/heading-shortcuts | Strg+0…6 für Überschriften im Typora-Stil |
| feat/outline-jump | Klick auf eine Überschrift in der Outline springt dorthin |
| feat/block-prefix-reveal | #101: Inline-Source wie bei Typora; das Überschriften-Präfix wird bei Fokus sichtbar und editierbar |
| feat/table-insert-middle | Zeilen und Spalten mitten in der Tabelle einfügen |
| feat/writing-comfort | Auto-Pair, Fokus-Modus, Typewriter-Modus, Read-only-Modus, View-Menü |
| feat/document-features | u. a. `[[Wiki-Links]]` auf Nachbar-Notizen |
| feat/tabs | Tabs und Multi-File (EditorWindow-Host in `src/editor/tabs.rs`), Session-Drafts für ungespeicherte Tabs |
| feat/spellcheck | Hunspell über `spellbook`, 20 Sprachen, mehrere gleichzeitig; Fallback ist Windows ISpellChecker |
| feat/german-ui | Deutsche UI (`de_de()` in `src/i18n/mod.rs`) |
| feat/workspace-search | Strg+P Quick Open (Fuzzy, kombiniert mit Recent Files), Strg+Shift+F „Im Ordner suchen“ (im Hintergrund, max. 200 Treffer); ein Treffer öffnet die Datei im Tab und markiert die Stelle |
| feat/workspace-folder | Datei > Ordner öffnen (Strg+Shift+O) / Ordner schließen: Workspace-Root pro Fenster für alle Tabs, Drawer, Quick Open und Suche; wird beim nächsten Start wiederhergestellt (`.workspace` im Config-Ordner) |
| feat/workspace-replace | Ordnersuche mit Groß/Klein (Alt+C), ganzes Wort (Alt+W), Regex (Alt+R), Zähler „N Treffer in M Dateien“, max. 1000 Zeilen / 20 000 Dateien; Ersetzen in allen Dateien mit Bestätigung, überspringt Tabs mit ungespeicherten Änderungen, lädt saubere Tabs neu |
| feat/ci-platforms | GitHub-Actions-CI: build + test auf ubuntu-22.04, macos-15, windows-2025 |
| feat/export | Syntax-Highlighting im HTML/PDF-Export, Überschriften-IDs immer, PDF-Seiteneinstellungen (Format, Ausrichtung, Rand, Seitenzahlen; Einstellungen > Export, `[pdf_export]`), Browser-Check vor dem PDF-Dialog mit HTML-Ausweichweg, Bestätigung mit „Öffnen“ |
| feat/themes | Themes GitHub, Newsprint, Night, Whitey; „Systemdarstellung folgen“ (helles/dunkles Theme); Mermaid in Theme-Farben; SVG-Caches werden nach 30 Tagen bereinigt (`src/components/svg_cache.rs`) |
| feat/format-paragraph-menus | Menüs „Absatz“ und „Format“ wie Typora (Überschriften, Ebene ±, Tabelle, Mathe-/Codeblock, Zitat, Listen, Aufgabenstatus, Hinweisblöcke, Fußnote, Linkreferenz, Trennlinie, TOC, Front Matter; Durchgestrichen, Markierung, Hoch/Tief, Inline-Mathe, Kommentar, Link Strg+K, Formatierung entfernen Strg+\). Quelltext-Befehle in `src/editor/markdown_commands.rs` (rein, getestet), Editor-Anbindung in `paragraph_format.rs` (`apply_source_edit` = ein Undo-Schritt). Menüklicks laufen über `is_document_command_action` in `app_menu.rs` an den fokussierten Block |
| feat/clipboard | HTML aus Browser/Office wird beim Einfügen zu Markdown (`htmd`; nur wenn es Formatierung bringt, Code aus Editoren bleibt wörtlich), Als Text einfügen (Strg+Shift+V), Als reinen Text kopieren, Als HTML kopieren; neues Menü „Bearbeiten“. HTML-Zwischenablage über `arboard` in `src/clipboard.rs` (GPUI kann kein HTML) |
| feat/file-actions | Kontextmenü im Dateibaum (Öffnen, Neue Datei, Neuer Ordner, Umbenennen, In den Papierkorb, Pfad kopieren, Im Dateimanager zeigen; Namensdialog, offene Tabs folgen Umbenennungen, Tabs gelöschter Dateien schließen). Datei-Menü: Geschlossenen Tab wieder öffnen (Strg+Shift+T), Alle speichern, Von Festplatte neu laden, Im Dateimanager zeigen, In Seitenleiste zeigen, Datei in den Papierkorb, „Liste leeren“ bei zuletzt geöffneten. Code: `file_ops.rs` (rein, getestet), `workspace_actions.rs`, `file_menu.rs`; Papierkorb über `trash`-Crate |
| feat/selection-commands | Bearbeiten → Auswahl (Absatz/Block Strg+Alt+P, Zeile/Satz Strg+L, Formatbereich Strg+E, Wort Strg+D, Zum Anfang/Zur Auswahl Strg+J/Zum Ende) und Bearbeiten → Löschen (Absatz/Block Strg+Shift+Alt+P als Quelltext-Edit, Zeile/Satz Strg+Shift+Alt+L, Formatbereich Strg+Shift+Alt+E, Wort Strg+Shift+D). Bereichslogik in `src/components/block/scopes.rs` |
| fix/unix-test-segfault | SIGSEGV im Testprozess auf macOS/Linux: `quit()` nicht über den geleakten `VisualTestContext` (Use-after-free in gpui) |
| feat/images | Bilder per Drag & Drop (`src/editor/images.rs`), eingefügte Bilder relativ und mit Zeitstempel-Namen, unbenannte Dokumente → Temp-Ordner, `width`/`height` an `<img>`, Kontextmenü Zoom 25–150 % |

## Wichtige Stellen im Code

- `src/editor/workspace_search.rs`: Palette (PaletteState, fuzzy_score, search_files, reveal_search_hit)
- `src/editor/tabs.rs`: `EditorTabEvent::OpenPathAndReveal`
- `src/editor/find.rs`, `writing.rs`, `spelling.rs`: Suchen, Schreibhilfen, Kontextmenü der Rechtschreibprüfung
- `src/spellcheck/{mod,hunspell,windows_backend}.rs`. Die Wörterbücher werden zur Laufzeit von
  wooorm/dictionaries geladen, weil sie unter GPL stehen und deshalb nicht mitgeliefert werden. Ablage: `%APPDATA%\manyougz\Velotype\config\dictionaries`,
  dazu `personal.txt` als eigenes Wörterbuch. de-DE und en-US sind lokal schon installiert.
- `src/components/actions.rs`: Actions plus SHORTCUT_DEFINITIONS. Beschriftungen stehen in `src/config/preferences.rs` (`shortcut_command_label`).
- i18n-Helfer: `node <scratchpad>/add-i18n.js . <anchorKey> spec.json </dev/null` trägt Keys nach einem Anker in alle Stellen ein (der Anker muss in der Merge-Funktion mehrzeilig formatiert sein, z. B. `quick_open_placeholder`).
- i18n: Jeder neue String braucht Einträge in `I18nStrings`, `I18nStringsDe`, der Key-Liste, der Merge-Funktion, in `en_us`, `zh_cn` und `de_de` sowie in `assets/custom-language.example.jsonc`.
- Menü-Tests: Der View-Menüpunkt hat den Help-Index verschoben (HELP_IDX 6 bzw. 7).

## Bekannte Grenzen und mögliche nächste Schritte

- Ersetzen in Dateien lässt sich nicht rückgängig machen (wird vorher bestätigt).
- Bilder aus unbenannten Dokumenten bleiben im Temp-Ordner und werden beim ersten Speichern nicht verschoben.
- Newsprint hat keine Serifenschrift, weil Themes keine Schriftfamilie setzen können. Einige Mermaid-Typen (Quadrant, XY, Architektur, ER-Badges) haben feste Farben im Renderer-Crate.
- Mathe: keine Gleichungsnummerierung (`\tag`), `$$` muss allein auf der Zeile stehen; `align`/`cases` hängen von RaTeX ab.
- Rechtschreibprüfung hat außer auf Windows keinen System-Fallback (ohne geladenes Wörterbuch keine Prüfung).
- Typoras ```flow`/```sequence`-Blöcke und weitere Exportformate (DOCX/EPUB) fehlen.
- Issue #107 ließ sich nicht reproduzieren, ist vermutlich upstream behoben.

## Abweichungen zu Typora (Menü-Abgleich, Stand 2026-10-11)

Grundlage: `C:\Users\mzili\Documents\typora-menu-analysis\typora-menu-analysis.md` (Typora 1.14.10, Menübaum mit
Shortcuts). Abgeglichen gegen Actions, Menüs und Kontextmenüs im Code, nicht in der laufenden App. Vieles rendert
Velotype schon beim Tippen (Durchgestrichen, `==Markierung==`, Hoch-/Tiefgestellt, `[!NOTE]`-Hinweise, Fußnoten,
`[TOC]`, Mathe, Front Matter). Es fehlen aber Befehle und Kürzel dafür. Ein Bearbeiten-, Absatz- und Format-Menü gibt es nicht.

- **Datei, fehlt:** Neu laden von Festplatte, Alle speichern, Verschieben nach, Löschen (Papierkorb), Im Explorer zeigen,
  In Seitenleiste zeigen, Eigenschaften, geschlossenen Tab wieder öffnen (Strg+Shift+T), Liste der zuletzt geöffneten Dateien leeren,
  mit Kodierung neu öffnen (nur UTF-8), Drucken, Export mit vorherigen Einstellungen (Strg+Shift+E), HTML ohne Styles, Bild-Export,
  Pandoc-Export/-Import, Autosave/Backup.
- **Bearbeiten, fehlt:** Als Text einfügen (Strg+Shift+V), HTML→Markdown beim Einfügen, als HTML/Text kopieren, Bild kopieren,
  Auswahl Zeile (Strg+L), Wort (Strg+D), Formatbereich (Strg+E), Block, zur Auswahl springen (Strg+J); Zeile/Block/Bereich löschen;
  typografische Zeichen (Anführungszeichen, Striche); Zeilenenden CRLF/LF und Leerzeile am Ende; Emoji-Auswahl;
  Formeln neu berechnen.
- **Absatz, fehlt:** Überschriftenebene ± (Strg+= / Strg+-), Tabelle Strg+T, Zeile darunter Strg+Enter, Zeilen/Spalten verschieben
  (Alt+Pfeile), Tabelle kopieren/formatieren/löschen, Matheblock (Strg+Shift+M), Codeblock (Strg+Shift+K), Code-Werkzeuge,
  Zitat (Strg+Shift+Q), nummerierte Liste (Strg+Shift+[), Aufzählung (Strg+Shift+]), Aufgabenliste (Strg+Shift+X), Aufgabenstatus,
  Hinweisblöcke, Absatz davor/danach, Fußnote, Linkreferenz, Trennlinie, Inhaltsverzeichnis, YAML-Front-Matter.
- **Format, fehlt:** Durchgestrichen (Alt+Shift+5), Markierung, Hoch-/Tiefgestellt, Kommentar, Inline-Mathe, Link (Strg+K),
  Link öffnen/kopieren, Formatierung entfernen (Strg+\), Bildmenü (Bild einfügen Strg+Shift+I, lokale Bilder per Dialog,
  Bildordner öffnen, Markdown/HTML-Syntax wechseln, Bilder kopieren/verschieben, Bilddatei löschen, Hochladen,
  `typora-root-url`). Zoomstufen 33/67/80/200 % fehlen.
- **Ansicht, fehlt:** Statusleiste im Menü schalten, Werkzeugleiste, Vollbild (F11), Immer im Vordergrund, Zoom (vergrößern,
  verkleinern, Originalgröße), Artikel-Liste in der Seitenleiste. **Abweichung:** Strg+Tab schaltet in Velotype den
  Quellcode-Modus, in Typora wechselt es zwischen Dokumenten (Quellcode-Modus dort Strg+/).
- **Themes/Hilfe, fehlt:** CSS-Themes, „Theme-Ordner öffnen“, mitgelieferte Hilfedokumente (Schnellstart, Markdown-Referenz).
- **Kontextmenüs, fehlt:** Block- und Textformate, Kopieren/Einfügen als …, Einfügen von Absatz/Bild/Mathe/HTML/Code,
  Mathe kopieren, Websuche; Dateibaum-Menü (neue Datei/neuer Ordner, umbenennen, löschen, Pfad kopieren, im Explorer zeigen).

Reihenfolge (mit Malte abgestimmt): ~~1. Absatz- und Format-Menü mit Typora-Kürzeln~~ (erledigt), ~~2. Zwischenablage~~ (erledigt; Bild kopieren fehlt noch),
~~3. Dateibaum und Dateiaktionen~~ (erledigt; „Verschieben nach“, „Eigenschaften“, Kodierung, Autosave fehlen), ~~4. Auswahl- und Löschbefehle~~ (erledigt), 5. Ansicht (Zoom, Vollbild, Immer im Vordergrund),
6. Typografie/Zeilenenden/Emoji, 7. Pandoc, Drucken, Bild-Upload.

Nach Punkt 1 noch offen bzw. abweichend: Strg+T bleibt „Neuer Tab“ (Typora: Tabelle; Tabelle auf macOS Cmd+Alt+T, unter Windows ohne Standardkürzel). Absatz davor/danach einfügen fehlt. Kein Häkchen-Status im Menü für den aktuellen Blocktyp. Strg+Shift+[ / ] und Alt+Shift+5 sind unter Windows nicht von Hand geprüft (Tastenlayout).

## Hinweise zu Fallstricken

- `sed` mit `\u{...}` oder mit leeren Variablen hat schon Dateien zerschossen. Mehrzeilige Änderungen besser mit dem Edit-Tool machen.
- Merge-Konflikte entstehen oft bei Tests, die am Dateiende angehängt wurden. Danach prüfen, ob noch alle Tests vorhanden sind.
- Kein `cat > /tmp/...` mit Heredoc im Hintergrund, das hat sich schon einmal aufgehängt.
