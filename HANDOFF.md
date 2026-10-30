# Velotype – Handoff (Stand 2026-10-30)

Ziel: Velotype (Rust/GPUI-Markdown-Editor) an Typora heranführen, seit dem 27.10. auch Obsidian-Funktionen
(Links, Tags, Tagesnotizen, Vorlagen, Abfragen, Graph). Gearbeitet wird im Fork
`origin = github.com/mzilinski/velotype`. `upstream = manyougz/velotype` bleibt unberührt,
PRs dorthin nur auf ausdrücklichen Wunsch.

## Arbeitsweise

- Jedes Feature kommt auf einen eigenen Branch, abgezweigt von `integration`. `integration` führt alle zusammen und ist der Stand zum Testen;
  `main` wird per Fast-Forward auf `integration` nachgezogen (Stand 2026-10-29: beide auf e3dbd8c). `dev` gibt es nicht mehr.
- Gemergte Branches werden lokal und auf origin gelöscht (Aufräumen am 2026-10-29: übrig sind nur `main`, `integration`, `handoff`).
  Die Branch-Namen in der Tabelle „Erledigt“ sind deshalb nur noch Bezeichnungen, der Code steckt in `integration`.
- **Push nur nach expliziter Freigabe** durch Malte.
- Jede Commit-Message endet mit `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Test-Builds: `cargo build --release`, dann die Exe kopieren nach
  `C:\Users\mzili\Developer\velotype-builds\velotype-integration-N.exe`. Bei jedem Build wird N hochgezählt, weil eine laufende Exe gesperrt ist.
- Linux-Rechner (CachyOS): Klon in `~/Developer/velotype`, Rust per `pacman -S rustup` (stable 1.99). Test-Build ist einfach
  `target/release/velotype`; die GPUI-Systembibliotheken waren schon installiert.
- Tests mit `cargo test`; ein Netzwerk-Test ist mit `#[ignore]` markiert und läuft nur mit `-- --ignored`.
- Regel für Fenster-Tests: nach `add_window_view` nie `cx.quit()` auf dem zurückgegebenen `cx` (Use-after-free in gpui, siehe Mac-Runde, Punkt 1).
- `.claude/` (und `HANDOFF.md`, falls lokal kopiert) gehört in `.git/info/exclude`, damit Agent-Worktrees nicht mit committet werden.
  Auf dem Linux-Rechner fehlt dieser Eintrag noch.
- `gh` zeigt in diesem Klon standardmäßig auf `upstream` (manyougz). Für den Fork `gh … -R mzilinski/velotype` angeben.
- `HANDOFF.md` bearbeiten: `git worktree add ../velotype-handoff handoff`, dort ändern, committen, pushen, danach `git worktree remove ../velotype-handoff`.

## Aktueller Stand

Alles ist gepusht (2026-10-28). `main` = `integration` = e3dbd8c, Version 0.7.8. CI auf `integration` und `main` grün
(Linux, macOS, Windows; Lauf 40675401479). Seit dem 13.10. dazugekommen (Details in der Tabelle „Erledigt“):
- **Obsidian-Funktionen** (2026-10-27): Links-Reiter mit Backlinks, ausgehenden Links, unverlinkten Erwähnungen und Tags; `[[`-Vervollständigung;
  Links beim Umbenennen/Verschieben anpassen; Front Matter als Eigenschaften-Tabelle; Tagesnotiz; Vorlagen; Dataview-Abfragen; Graph.
- **Upstream gemergt** (2026-10-28, `upstream/main`): Umstieg auf `gpui-pre` 0.3.9, PicGo-Server-Upload, .md-Dateizuordnung, optionaler Updater.
- Fixes: Einstellungen bleiben im Datei-Menü an ihrem Platz; ein Panel-/Overlay-Kürzel schließt beim zweiten Drücken.
- Mac: App 0.7.8 aus e3dbd8c in `/Applications` installiert (`scripts/create_macos_app_dist.sh`, dann `ditto` nach `/Applications/Velotype.app`).
  Demo-Vault `~/Documents/Velotype-Demo` (17 Notizen, `Start.md` = Tour mit Kürzeln, `Übersicht.md` = Abfragen, `.obsidian/daily-notes.json`
  und `templates.json`). Monats-/Wochentagsnamen in Vorlagen sind englisch (Moment-Format), deshalb `{{date:DD.MM.YYYY}}`.
- **Upstream-Issue** zu den Fork-Funktionen: https://github.com/manyougz/velotype/issues/134 (2026-10-28). Antwort abwarten; je nach
  Antwort kleine PRs schneiden oder Fork eigenständig weiterführen (ggf. umbenennen).
- **2026-10-30 (Mac): Absturz behoben**, `fix/selection-word-count-panic` (8dc09ce), lokal in `integration` gemergt (63d3390), **noch nicht gepusht**.
  Die installierte App (e3dbd8c) stürzte ab, wenn in der Quelltext-Ansicht oder in einem Codeblock Text nach einem escapeten Zeichen (`*`, `\`)
  und vor einem Umlaut ausgewählt war und die Wortzählung lief. Details in der Tabelle „Erledigt“. App in `/Applications` noch nicht neu gebaut.
- Bekannt: Clippy-Fehler `text_offsets.rs:116` (`reversed_empty_ranges` in einem Test) und einige Warnungen sind vorbestehend/upstream.

Die neuen Funktionen sind nur per Unit-/gpui-Tests geprüft, nicht von Hand in der App.

**Offen:**
- Nach der Fehlermeldung „Öffnen fehlgeschlagen“ (nativer TaskDialog, Windows) dauerte der nächste Öffnen-Dialog >4 min. Rückmeldung von Malte ausstehend,
  ob die Meldung hinter dem Fenster hing (dann durch In-Fenster-Meldung ersetzen). Test-Checkliste (Artifact, privat): https://claude.ai/artifact/DMG154bZTwyfnu49VYsSLB
- Obsidian-Funktionen und der Upstream-Merge von Hand in der App prüfen (v. a. Bild einfügen mit Upload/PicGo, Updater zeigt auf Fork-Releases).
- Mac-Runde (unten) steht noch aus.
- `.github/workflows/ci.yml` hat `dev` noch als Trigger-Branch; kann raus.

**Pandoc:** auf dem Windows-Rechner installiert (winget, 3.12.1, liegt in `%LOCALAPPDATA%Pandoc`, nicht im PATH; die Ersatzsuche findet es). Die Tests `round_trip_through_installed_pandoc` und `all_formats_and_import_with_images` laufen nur, wo Pandoc installiert ist, sonst überspringen sie sich.
**PicGo:** nicht installiert (braucht ein Bild-Hosting-Konto); der Upload ist mit einem echten Skript getestet (`upload_script_receives_paths_with_spaces`).

**Mac-Runde, offene Prüfpunkte:**
1. Ansicht → Immer im Vordergrund (`src/window_pin.rs`, `objc`/`setLevel:`): Fenster bleibt über anderen, Abschalten geht.
2. Pandoc per `brew install pandoc`, dann `cargo test pandoc -- --nocapture` (muss „using /opt/homebrew/bin/pandoc“ zeigen) und in der aus dem Finder gestarteten App Export → Word sowie Datei → Importieren… (Finder-Apps sehen den Shell-PATH nicht, deshalb die Suche in `/opt/homebrew/bin`).
3. Datei → Drucken… öffnet Safari/Standardbrowser mit Druckdialog.
4. Emoji-Auswahl: Emojis farbig?
**Mac bitte prüfen:** `src/window_pin.rs` (Immer im Vordergrund über `objc`/`setLevel:`) ist unter macOS nur von CI kompiliert, nicht ausprobiert.

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
- Native Menüleiste: Velotype, Datei, Bearbeiten, Absatz, Format, Export, Theme, Ansicht, Sprache, Hilfe.
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
| feat/view-menu | Ansicht: Statusleiste ein/aus (gespeichert), Vollbild (F11, Mac Strg+Cmd+F), Immer im Vordergrund (`src/window_pin.rs`, Windows `SetWindowPos`, macOS `NSWindow setLevel:`, unter Linux ausgeblendet), Originalgröße/Vergrößern/Verkleinern (Strg+Shift+0/=/-, auch Strg++/Strg+_). Zoom skaliert Dokumentschrift und -abstände im `ThemeManager` (`Theme::zoomed`, `unzoomed()` für Export), Stufen 50–300 %, gespeichert als `editor.zoom_percent` |
| feat/line-endings | Bearbeiten → Zeilenenden: Windows (CRLF) / Unix (LF) pro Dokument (beim Laden erkannt, beim Speichern beibehalten; vorher wurde CRLF still zu LF), „Beim Speichern mit Zeilenumbruch enden“ (`editor.final_newline`). Statusleiste zeigt LF/CRLF, Klick schaltet um. `src/editor/line_endings.rs`; Menü-Häkchen über Global `MenuLineEnding`, das das aktive Fenster beim Rendern setzt |
| feat/smart-punctuation | Bearbeiten → Typografische Zeichen: Anführungszeichen (deutsche Oberfläche „…“ ‚…‘, sonst “…” ‘…’; ' nach Buchstabe = Apostroph ’) und Striche (-- → –, dritter - → —). Mit Autopair (Paar einfügen, überspringen). Nicht in Code/Mathe/HTML/Front Matter/Inline-Code/Quellcode-Modus, nicht bei `---` und `<!--`. Standard aus (`editor.smart_quotes`, `editor.smart_dashes`). `src/components/block/smart_punctuation.rs` |
| feat/emoji-picker | Bearbeiten → Emoji und Symbole…: Suchraster (Crate `emojis`, englische Namen und :Kürzel:), Pfeiltasten/Enter/Klick fügt am Cursor des zuvor fokussierten Blocks ein, Escape schließt. `src/editor/emoji_picker.rs` |
| feat/pandoc | Export-Menü: Word, ODT, RTF, EPUB, LaTeX, reStructuredText, MediaWiki, Org über Pandoc (`ExportWithPandoc { format }`); Datei → Importieren… (Word/ODT/HTML/LaTeX/EPUB → neuer Tab, Bilder nach `<name>-media`). Pandoc-Suche in PATH und Standardorten, sonst Dialog mit Link auf pandoc.org. `src/export/pandoc.rs`, `src/editor/pandoc.rs` |
| feat/print | Datei → Drucken… (Alt+Shift+P): HTML-Export mit eingebetteten Bildern in Temp-Datei, Standardbrowser öffnet Druckdialog per Skript. `src/editor/print.rs` |
| feat/image-upload | Format → Bild → Bild hochladen / Alle lokalen Bilder hochladen; Einstellung „Upload-Befehl“ (Einstellungen → Bild, `editor.image_upload_command`, z. B. `picgo upload`): Bilddateien als Argumente, letzte URL-Zeilen der Ausgabe ersetzen die Links (ein Undo-Schritt). Neue Einfügeoption „Mit dem Upload-Befehl hochladen“ für eingefügte/gezogene Bilder. `src/editor/image_upload.rs` |
| fix/zoom-line-height | Zeilenhöhen (in rem) wachsen mit dem Zoom (Fenster-rem = 16 px × Zoom; vorher überlappten Zeilen bei 200 %). Quellcode-Modus nutzt die Code-Schrift statt der Fließtextschrift |
| fix/source-fidelity | **Originaltreue beim Speichern:** Vorher schrieb Velotype jede Datei beim Speichern komplett neu (Leerzeilen zwischen Überschriften, eingerückter Code → Fences, `~`, `` ` `` …; geerbt von upstream). Jetzt meldet der Parser pro Root-Region die Originalzeilen (`build_root_blocks_with_regions`, `SourceRegion`), `src/editor/source_fidelity.rs` sucht deren normalisierten Text im Dokument; beim Speichern (`document_text_for_disk`) und beim Wechsel in den Quellcode-Modus werden per Zeilen-Diff (Crate `similar`) unveränderte Regionen und Leerzeilen aus dem Original geschrieben. Undo und Source-Mapping arbeiten weiter auf dem normalisierten Text. Geprüft an 790 echten .md-Dateien: 777 byte-gleich. Dazu Fix: Inline-Parser lief bei vielen nicht schließbaren `*`-Öffnern exponentiell (mkdocs-Glossar mit `*[ABK]: …` fror Velotype ein; auch upstream) |
| fix/parser-stability | Originaltreue erst nach dem Setzen der Linkreferenzen berechnet (`[Text][ref]` las sich beim Laden anders). `_`/`__` innerhalb von Wörtern öffnen/schließen keine Hervorhebung mehr (CommonMark; `A_B_C` wurde als `A*B*C` gespeichert). Literale `*`, `_`, `~`, `^`, Backticks nur noch escapen, wo sie formatieren würden (`literal_marker_needs_escape`; `*` am Zeilenanfang bleibt escaped). Eingerückter Code schluckt keine Leerzeilen mehr. Prüfung an 790 Dateien: 780 byte-gleich (ohne Originaltreue wären es 10), 1 Abweichung = gemischte Zeilenenden (gewollt), 9 über 300 KB übersprungen |
| fix/shortcuts | Zoom: Strg++ (auch Strg+=, Strg+Shift+=), Strg+- (auch Strg+_, Strg+Shift+-), Originalgröße Strg+Shift+0. Überschriftenebene: Alt+Shift+↑/↓ (Mac Cmd+Alt+↑/↓). Strg+0 bleibt Absatz. Quellcode-Modus: Strg+/ (Cmd+/) und Ansicht → Quellcode-Modus mit Häkchen (`MenuDocumentState`). Strg+Tab / Strg+Shift+Tab wechseln Tabs; Cmd+Tab nicht mehr belegt |
| feat/autosave | Datei → Automatisch speichern (`editor.autosave`, Standard aus): geänderte Dokumente mit Datei werden ~2 s nach der Änderung gespeichert (`pending_autosave`, im nächsten Frame); ein fehlgeschlagenes Autosave meldet sich einmal und pausiert bis zum nächsten erfolgreichen Speichern. Sicherungskopien: vor jedem Überschreiben Kopie nach `<config>/backups/<name>-<FNV-Hash des Pfads>/<Zeitstempel>.md`, höchstens eine pro 10 min, 20 pro Datei; Datei → Sicherungskopien anzeigen. `src/editor/autosave.rs` |
| feat/export-options | Export → HTML (ohne Styles) (`ExportFormat::HtmlPlain`); Export → Mit vorherigen Einstellungen exportieren (Strg+Shift+E): `LastExport` pro Dokument (Builtin oder Pandoc), Export ohne Dialog an denselben Pfad. Export in „Ziel wählen“ (`export_document_via_prompt`) und „exportieren“ (`export_document_to`, `export_with_pandoc_to`) getrennt |
| feat/move-and-properties | Datei → Verschieben nach… (rename, über Laufwerke copy+delete; unbenannt = Speichern unter), Datei → Eigenschaften (Pfad, Größe, Erstellt/Geändert in Ortszeit via `chrono`, Wörter, Zeichen, Zeilen, Kodierung, Zeilenenden). Wortzählung (auch Statusleiste) zählt Markdown-Zeichen wie `#`, `-`, `>`, `|` nicht mehr. `src/editor/file_info.rs` |
| feat/image-menu | Format → Bild: Bild einfügen… (Strg+Shift+I), Speicherort öffnen, Bildgröße 25–200 % (`SetImageZoom { percent }`, Stufen jetzt auch 33/67/80/200, auch im Kontextmenü), Markdown/HTML-Syntax wechseln, Bild kopieren nach… / verschieben nach… (Ordnerdialog, freier Name, Link wird umgeschrieben), Bilddatei löschen (Papierkorb + Bild aus Dokument). Gemeinsame Basis `replace_image_block`. `src/editor/image_menu.rs` |
| feat/image-paths | Bild kopieren (Zwischenablage, gpui `ClipboardItem::new_image`). `typora-root-url` aus der Front Matter: Quellen mit `/` relativ zu diesem Ordner (Editor via `Block::set_image_root`, Bild-Menü, Export); Format → Bild → Bildstammordner festlegen… schreibt den Schlüssel. Datei → Verschieben nach… schreibt relative Bildlinks um (`relative_link_path` mit `../`, `moved_image_links`) |
| feat/context-menu | Kontextmenü im Dokument (`ContextMenuState::General`, `src/editor/context_menu_general.rs`): Ausschneiden/Kopieren/Einfügen, als Text/HTML kopieren, als Text einfügen, Absatz ▸ und Format ▸ (direkt aus `app_menu::paragraph_menu`/`format_menu` übernommen, mit Tastenkürzeln), Einfügen ▸ (Tabelle… nur wo erlaubt, Bild…, Emoji…), „Mit Google nach … suchen“ bei Auswahl. Einträge fokussieren den rechtsgeklickten Block und dispatchen die Aktion; Untermenüs per Hover mit 150 ms Verzögerung beim Verlassen, Panels bleiben im Fenster; auch im Schreibschutz. Altes „Einfügen ›“-Menü entfernt. Dateibaum-Menü: In neuem Fenster öffnen |
| feat/articles-toolbar | Seitenleiste: Reiter „Artikel“ (`WorkspaceTab::Articles`, `src/editor/articles.rs`): Markdown-Dateien des Ordners flach, neueste zuerst, mit Titel (erste Überschrift), Datum, Textanfang; im Hintergrund gelesen (erste 4 KB, max. 500 Dateien). Ansicht → Gliederung / Artikel / Dateien (Strg+Shift+1/2/3). Ansicht → Werkzeugleiste (`editor.toolbar`, Standard aus, `src/editor/toolbar.rs`): Knöpfe über dem Dokument, dispatchen die Menü-Aktionen über den fokussierten Block (Klick behält Fokus, per simuliertem Klick getestet), Tooltip mit Name und Kürzel |
| feat/css-themes | Typora-CSS-Themes: `.css` im Theme-Ordner bzw. über „Theme hinzufügen…“ werden Themes (`css-<Name>`). GPUI rendert kein CSS, daher `src/theme/css_theme.rs`: einfacher Regel-Parser (Kommentare, At-Rules wie `@media` übersprungen, `var(--x)` aufgelöst), überträgt Hintergrund, Text, Links, Überschriften (Farben, Größen), Zitat, Code/`.md-fences`, Tabelle, Auswahl, Trennlinie, `mark` auf GitHub- bzw. Night-Basis (nach Helligkeit). `Theme.css` (serde skip) hängt der HTML/PDF-Export ans CSS, `<main id="write">` wie Typora. Theme → Theme-Ordner öffnen. Geprüft mit Typoras github/newsprint/night/pixyll |
| feat/help-docs | Hilfe → Schnellstart, Markdown-Referenz (gebündelt in `assets/help/*.{de,en}.md`, Sprache nach UI, zh → en), Tastenkürzel (aus aktueller Belegung erzeugt, nach Kategorien wie in den Einstellungen, nur Kürzel dieser Plattform, „Strg“ auf Deutsch). Öffnen als schreibgeschützter Tab über eine Kopie in `<config>/help` (`app_menu::open_read_only_document`). Test: Referenz rendert Tabelle, Mathe, TOC, Fußnote, Hinweis, Code und bleibt unverändert gespeichert byte-gleich |
| feat/about-fork | Über-Dialog: „Weiterentwicklung von Malte Zilinski“, Link auf github.com/mzilinski/velotype (`ABOUT_GITHUB_URL`), Herkunftshinweis „Basiert auf Velotype von manyougz und Mitwirkenden, Apache-Lizenz 2.0“ mit Link (`ABOUT_ORIGIN_URL`, `help_about_origin`). Umbenennung und eigene App-Kennung bewusst noch nicht (Malte: „erst mal nicht“). Die Update-Prüfung zeigt seit dem Upstream-Merge vom 28.10. auf die Releases des Forks |
| perf/workspace-scan | Langsames Öffnen behoben: jeder Tab las den Ordner der Seitenleiste rekursiv auf dem UI-Thread (beim 2. Öffnen = neuer Tab; gemessen 0,5 s Developer, 9,3 s Home). Jetzt `WorkspaceTreeCache` (gpui-Global in `src/editor/workspace.rs`): Baum je Wurzel, geteilt von allen Tabs/Fenstern, im Hintergrund gelesen („Ordner wird gelesen …“), älter als 10 s → beim Öffnen der Seitenleiste/neuem Tab im Hintergrund neu gelesen, Limit 20.000 Einträge mit Hinweis. `forget_workspace_tree` nach Dateioperationen (je Wurzel einmal). Gliederung nutzt `last_stable_source_text` statt das Dokument bei jedem Render zu serialisieren |
| diag/perf-log | Diagnose-Protokoll `<config>/logs/perf.log` (`src/perf_log.rs`): `perf_log::Span`/`step`/`note` an allen Stufen des Öffnens (Dialog, Lesen, Parsen, Runtimes, Fidelity, Undo-Snapshot, Recent, Menüs, Render, Rechtschreibung, Dateibaum) ab 15 ms; Watchdog-Thread meldet UI-Hänger ≥250 ms mit letztem Schritt; neu ab 2 MB; in Tests aus |
| fix/open-non-utf8 | Ursache des „Hängers“ beim Öffnen: Word-Datei gewählt → UTF-8-Fehler. Jetzt `src/text_file.rs`: `read_text_file` (UTF-8 ±BOM, UTF-16 BOM, sonst Windows-1252, Binär → `NotText` mit Meldung `open_not_text_message`); .docx/.odt/.rtf/.epub öffnen über `app_menu::import_document_in_tab` (Pandoc im Hintergrund) aus Dialog, CLI, Dateibaum, Zuletzt geöffnet. CLI: vorhandene unlesbare Datei wird nicht mehr als leeres Dokument mit Pfad geöffnet (Überschreibgefahr) |
| fix/unix-test-segfault | SIGSEGV im Testprozess auf macOS/Linux: `quit()` nicht über den geleakten `VisualTestContext` (Use-after-free in gpui) |
| merge/upstream-0.7.5 | Upstream v0.7.5: einstellbare Schriften für Text/Code/Oberfläche (#121), Doppelklick-Wortauswahl (#119, war schon in `dev`), Gliederungs-Navigation (#116), Markdown in `<div>`-Blöcken, IME-Unicode-Absturz behoben (`src/ime.rs`, `src/text_offsets.rs`). 36 Konfliktdateien, fast nur upstream-Doku-Kommentare an Fork-Code. Gliederung: Fork-Variante `jump_to_source_line` bleibt, upstreams `reveal_outline_heading` entfällt, sein Test bleibt. Upstream-Test `source_selection_snapshot_clips_partial_unicode_offsets` erwartet jetzt unescapten Quelltext (`*测试`, wegen `fix/source-fidelity`) |
| feat/menu-cleanup | Menü „Arbeitsbereich“ entfernt (Seitenleiste ein/aus jetzt unter Ansicht über Gliederung/Artikel/Dateien), Sprache rechts vor Hilfe; Reihenfolge Export, Theme, Ansicht, Sprache, Hilfe. Deutsch „Theme“ statt „Design“ (Menü, Einstellungen, Dialoge). Wunsch von Malte: Sprache evtl. nur noch in den Einstellungen – dort gibt es noch keine Sprachauswahl |
| feat/backlinks | Seitenleiste: Reiter „Links“ (Ansicht → Links, Strg/Cmd+Shift+B; Strg+Shift+4 war auf macOS Screenshot). Link-Index des Ordners (`src/editor/link_index.rs`): `[[Wiki-Links]]` und relative Markdown-Links mit Zeile und Text, ohne Code/Bilder/Front Matter; Auflösung wie Obsidian (neben der Datei, ab Wurzel, dann Dateiname irgendwo, ohne Groß/Klein). Im Hintergrund gelesen, geteilt von allen Tabs/Fenstern, neu nach 10 s, nach Speichern und Dateioperationen. Ohne geöffneten Ordner gilt der Vault: nächster Ordner darüber mit `.obsidian` oder `.git`, nie Home. Geklickte Wiki-Links finden ihre Notiz im ganzen Ordner (`src/editor/backlinks.rs`). Reiterleiste bricht um |
| feat/link-autocomplete | `[[` tippen listet Notizen des Vaults (fuzzy, mit Ordner); ↑/↓, Enter/Tab vervollständigt zu `[[Notiz]]` (Pfad ab Wurzel bei doppeltem Namen), Escape schließt. Fix: Tippen in automatisch gepaarte `[[ ]]` ließ den Cursor hinter `]]` springen. `src/editor/link_completion.rs` |
| feat/rename-links | Umbenennen/Verschieben (Dateibaum, Datei → Verschieben nach…) sucht Links auf die alte Stelle und bietet an, sie umzuschreiben („12 Links in 4 Dokumenten …“). Wiki-Links bekommen den neuen Namen (bzw. Pfad, wenn nicht mehr eindeutig), Anker/Beschriftungen bleiben; Markdown-Links den neuen relativen Pfad. Tabs mit ungespeicherten Änderungen werden übersprungen und gelistet. `src/editor/link_rewrite.rs` |
| feat/tags | Tags wie Obsidian: `#tag` im Text (verschachtelt mit `/`) und `tags:`/`tag:` in der Front Matter; nicht in Code/Links/HTML, ohne Groß/Klein. Links-Reiter zeigt Tags des Dokuments und alle Tags des Vaults mit Anzahl; Klick listet die Dokumente. `src/editor/tags.rs` |
| feat/outgoing-mentions | Links-Reiter: ausgehende Links aus dem Live-Text (fehlende Notizen gedimmt), „Unverlinkte Erwähnungen“ (Dateiname als ganzes Wort in anderen Dokumenten, max. 200, im Hintergrund). `src/editor/mentions.rs` |
| feat/properties | Front Matter ohne Fokus als Eigenschaften-Tabelle (Listen/Tags als Chips, Leeres als Strich, verschachteltes YAML als Quelltext); mit Fokus YAML zum Bearbeiten. Beim Öffnen wird der erste Block nach der Front Matter fokussiert. `src/components/markdown/front_matter.rs` |
| feat/daily-notes | Datei → Heutige Notiz (Strg/Cmd+Alt+J), legt sie beim ersten Mal an; Ordner, Datumsformat und Vorlage aus `.obsidian/daily-notes.json`. Einfügen → Vorlage einfügen… (Strg/Cmd+Alt+Shift+T) aus `.obsidian/templates.json` bzw. `Templates/`; Variablen `{{title}}`, `{{date}}`, `{{time}}`, `{{date:FORMAT}}`. Einfügelogik jetzt in `Block::paste_text`. `src/editor/daily_notes.rs` |
| feat/queries | ```` ```dataview ````/```` ```query ```` zeigt ohne Fokus das Ergebnis als Tabelle/Liste (Klick öffnet). Teilmenge von Dataview: TABLE/LIST [WITHOUT ID], FROM #tag/"ordner"/[[notiz]]/outgoing([[notiz]]) mit and/or/-, WHERE mit Vergleichen und contains/startswith/length/date(today)…, SORT, LIMIT; über Front-Matter- und `file.*`-Felder. Link-Index hält dafür Eigenschaften und ausgehende Links. `src/editor/query.rs` |
| feat/graph | Ansicht → Graph (Strg/Cmd+Alt+G): Notizen als Knoten, Links als Kanten, Fruchterman-Reingold im Hintergrund. Lokal: Notizen bis zwei Links entfernt; global: die 400 am besten verbundenen. Hover hebt Links hervor, Klick öffnet. `src/editor/graph.rs` |
| fix/windows-backlink-test | Backlink-Test vergleicht Pfade statt Strings (Windows-Backslashes) |
| fix/menu-daily-note-order | „Heutige Notiz“ im Datei-Menü nach „Einstellungen“, damit die Menü-Tests unter Linux/Windows stimmen |
| fix/toggle-workspace-tabs | Kürzel der Seitenleisten-Reiter, der Paletten (Quick Open, Ordnersuche, Vorlage) und der Emoji-Auswahl schließen beim zweiten Drücken (wie schon der Graph) |
| merge upstream/main (2026-10-28) | Upstream bis 0.7.8: Umstieg auf `gpui-pre` 0.3.9 (Fork-Code portiert: Fokus braucht `cx`, Menüs mit `disabled`, `AsyncApp::update` liefert Wert, `ScrollHandle::max_offset` ist `Point`), PicGo-Server-Upload, .md-Dateizuordnung in den Einstellungen, optionaler Updater (`src/updater/`, Standard aus). Bild einfügen: upstreams Batch/PicGo nutzt die Fork-Ablage (`images.rs`); „Hochladen“ (Befehl) und „PicGo“ (Server) getrennt, keins ist lokaler Fallback. Beenden/Update fragen jeden Tab nach Speichern. Export läuft auf dem Background-Executor. Updater zeigt auf Fork-Releases. Deutsche Texte für neue Upstream-Strings |
| fix/selection-word-count-panic | Absturz (SIGABRT) beim Rendern der Statusleiste: `Editor::selected_markdown_text` schnitt `serialize_markdown()` mit Offsets aus `current_range_to_markdown_range`, die bei Raw-/Codeblöcken Anzeige-Offsets sind; der serialisierte Titel escapet aber (`*` → `\*`), so landete der Schnitt mitten in einem Umlaut. Kopieren nutzte denselben Pfad. Neu `Block::markdown_for_current_range`: Raw/Code schneidet den Anzeigetext, sonst serialisiertes Markdown auf Zeichengrenzen gerundet. Regressionstest `source_view_selection_after_escaped_markup_keeps_umlauts_whole` reproduziert die Panic auf dem alten Code |
| feat/images | Bilder per Drag & Drop (`src/editor/images.rs`), eingefügte Bilder relativ und mit Zeitstempel-Namen, unbenannte Dokumente → Temp-Ordner, `width`/`height` an `<img>`, Kontextmenü Zoom 25–150 % |

## Wichtige Stellen im Code

- `src/editor/workspace_search.rs`: Palette (PaletteState, fuzzy_score, search_files, reveal_search_hit)
- `src/editor/tabs.rs`: `EditorTabEvent::OpenPathAndReveal`
- `src/editor/find.rs`, `writing.rs`, `spelling.rs`: Suchen, Schreibhilfen, Kontextmenü der Rechtschreibprüfung
- `src/editor/link_index.rs`: gemeinsamer Link-Index (Links, Tags, Eigenschaften je Notiz); darauf bauen `backlinks.rs`, `link_completion.rs`,
  `link_rewrite.rs`, `tags.rs`, `mentions.rs`, `query.rs`, `graph.rs` auf. Vault-Ermittlung in `backlinks.rs`
- `src/spellcheck/{mod,hunspell,windows_backend}.rs`. Die Wörterbücher werden zur Laufzeit von
  wooorm/dictionaries geladen, weil sie unter GPL stehen und deshalb nicht mitgeliefert werden. Ablage: `%APPDATA%\manyougz\Velotype\config\dictionaries`,
  dazu `personal.txt` als eigenes Wörterbuch. de-DE und en-US sind lokal schon installiert.
- `src/components/actions.rs`: Actions plus SHORTCUT_DEFINITIONS. Beschriftungen stehen in `src/config/preferences.rs` (`shortcut_command_label`).
- i18n-Helfer: `node <scratchpad>/add-i18n.js . <anchorKey> spec.json </dev/null` trägt Keys nach einem Anker in alle Stellen ein (der Anker muss in der Merge-Funktion mehrzeilig formatiert sein, z. B. `quick_open_placeholder`).
- i18n: Jeder neue String braucht Einträge in `I18nStrings`, `I18nStringsDe`, der Key-Liste, der Merge-Funktion, in `en_us`, `zh_cn` und `de_de` sowie in `assets/custom-language.example.jsonc`.
- Menü-Tests in `src/app_menu.rs`: Indizes EXPORT/THEME/VIEW/LANGUAGE/HELP_IDX = 4/5/6/7/8 (macOS je +1 wegen App-Menü).

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
~~3. Dateibaum und Dateiaktionen~~ (erledigt; „Verschieben nach“, „Eigenschaften“, Kodierung, Autosave fehlen), ~~4. Auswahl- und Löschbefehle~~ (erledigt), ~~5. Ansicht (Zoom, Vollbild, Immer im Vordergrund)~~ (erledigt; Werkzeugleiste und Artikel-Liste fehlen),
~~6. Typografie/Zeilenenden/Emoji~~ (erledigt; Typora-Modus „beim Rendern umwandeln“ und Unicode-Satzzeichen-Mapping fehlen), ~~7. Pandoc, Drucken, Bild-Upload~~ (erledigt; „Export mit vorherigen Einstellungen“, HTML ohne Styles, Bild-Export, S3-Upload fehlen).

**Nächster Plan (mit Malte abgestimmt, 2026-10-12):**
- ~~A. Fixes~~ (erledigt, siehe `fix/parser-stability`, `fix/shortcuts`): (1) Inline-Parser liest Konstrukte nach dem ersten Rendern anders als beim Laden (siehe Folgepunkte unten), (2) unnötige Escapes `~`/`` ` `` beim Bearbeiten, (3) eingerückter Code wird beim Bearbeiten zu Fence mit überflüssiger Leerzeile, (4) Kürzel: Zoom auf Strg++/Strg+-/Strg+0 (Überschriftenebene neu belegen), Quellcode-Modus ins Menü Ansicht mit Strg+/, Strg+Tab wechselt Tabs. Bewusste Abweichung von Typora ist ok.
- ~~8. Datei-Menü abschließen~~ (erledigt; beim Verschieben werden relative Bildpfade nicht angepasst): Autosave und Sicherungskopien (zuerst), Export mit vorherigen Einstellungen (Strg+Shift+E), HTML ohne Styles, Verschieben nach…, Eigenschaften.
- ~~9. Bild-Menü~~ (erledigt; „Alle Bilder neu laden“ fehlt, GPUI cacht geladene Bilder): Bild einfügen per Dialog (Strg+Shift+I), Bildordner öffnen, Markdown/HTML-Syntax wechseln, Bilder kopieren/verschieben mit Pfadanpassung, Bilddatei löschen, Bild kopieren, `typora-root-url`.
- ~~10. Kontextmenüs~~ (erledigt; „Mathe kopieren“ als MathML/TeX fehlt): Block- und Textformate, Kopieren/Einfügen als …, Einfügen von Absatz, Bild, Mathe, Code.
- ~~11. Ansicht~~ (erledigt): Werkzeugleiste, Artikel-Liste in der Seitenleiste.
- Später: ~~CSS-Themes wie Typora mit „Theme-Ordner öffnen“~~ (erledigt; Schriftarten aus CSS werden nicht übernommen, Fonts bleiben Einstellung), ~~mitgelieferte Hilfedokumente~~ (erledigt).
- Grundsatz: Verhalten aus upstream ist kein Grund, Fehler zu behalten; geerbte Fehler wie eigene beheben.
**Originaltreue, Rest:** Bearbeitete Absätze werden neu erzeugt; dabei wird z. B. `*[Text][ref]*` zu `[*Text*][ref]` (gleiche Darstellung) und eingerückter Code zu einem Fence. Gemischte Zeilenenden werden auf die Mehrheit vereinheitlicht (wie Typora).

Nach Punkt 1 noch offen bzw. abweichend: Strg+T bleibt „Neuer Tab“ (Typora: Tabelle; Tabelle auf macOS Cmd+Alt+T, unter Windows ohne Standardkürzel). Absatz davor/danach einfügen fehlt. Kein Häkchen-Status im Menü für den aktuellen Blocktyp. Strg+Shift+[ / ] und Alt+Shift+5 sind unter Windows nicht von Hand geprüft (Tastenlayout).

## Hinweise zu Fallstricken

- **gpui-pre 0.3.9:** `window.focus(&handle, cx)` braucht `cx` (Borrow-Probleme mit `.focus_handle.clone()` lösen); `Menu { disabled: false, … }`;
  `AsyncApp::update`/`Entity::update` geben den Wert zurück (kein `let _ =`, sonst Clippy); `ScrollHandle::max_offset()` ist ein Point (`.y`).
  Der Test-Scheduler bricht bei Wakern aus fremden Threads ab → Hintergrundarbeit über `cx.background_executor().spawn`.
- **Sichtprüfung auf dem Mac ohne Tastatur:** Fenster-ID per Swift-Snippet (CGWindowListCopyWindowInfo, Owner „velotype“, Layer 0),
  dann `screencapture -x -o -l <id>`; Menüs per `osascript` (System Events → `click menu item "…" of menu "Ansicht" of menu bar 1`).
  Nie Tastendrücke senden und App mit `open -g` starten – sonst landet Maltes Tippen in Velotype. Verdeckte Fenster zeichnen auf macOS
  nicht neu (Screenshot zeigt alten Frame), für Prüfungen muss das Fenster sichtbar sein.
- **Crash-Reports auf dem Mac symbolisieren:** Release-Builds sind gestrippt (`strip = true`) und brechen bei Panics ab (`panic = "abort"`),
  die Panic-Meldung geht beim Start aus dem Finder verloren. Gleichen Stand mit `CARGO_PROFILE_RELEASE_STRIP=false
  CARGO_PROFILE_RELEASE_DEBUG=line-tables-only cargo build --release --target-dir target/symbolicate` bauen; das Layout weicht leicht ab,
  deshalb die Frames über Instruktionsbytes mit maskierten Branch-/ADRP-Immediates zuordnen, dann `atos -i`. Danach `target/symbolicate` löschen.
- `sed` mit `\u{...}` oder mit leeren Variablen hat schon Dateien zerschossen. Mehrzeilige Änderungen besser mit dem Edit-Tool machen.
- Merge-Konflikte entstehen oft bei Tests, die am Dateiende angehängt wurden. Danach prüfen, ob noch alle Tests vorhanden sind.
- Kein `cat > /tmp/...` mit Heredoc im Hintergrund, das hat sich schon einmal aufgehängt.
