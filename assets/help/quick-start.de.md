# Velotype – Schnellstart

Velotype ist ein Markdown-Editor, der deine Formatierung beim Schreiben direkt anzeigt. Du schreibst Markdown, siehst aber fertige Überschriften, Listen, Tabellen und Formeln.

> Dieses Dokument ist schreibgeschützt. Über **Ansicht → Schreibschutz** kannst du es zum Ausprobieren freigeben, Änderungen werden aber nicht gespeichert.

## Schreiben

- Tippe einfach Markdown: `# ` am Zeilenanfang wird eine Überschrift, `- ` eine Liste, `> ` ein Zitat, `**fett**` wird fett.
- Steht der Cursor in einem formatierten Abschnitt, zeigt Velotype dort die Markdown-Zeichen, damit du sie bearbeiten kannst.
- **Strg+B**, **Strg+I**, **Strg+U** formatieren die Auswahl fett, kursiv oder unterstrichen; **Strg+K** macht einen Link daraus (eine Adresse in der Zwischenablage wird übernommen).
- **Strg+1** bis **Strg+6** machen den Absatz zur Überschrift, **Strg+0** wieder zum normalen Absatz.
- Die Menüs **Absatz** und **Format** (und ein **Rechtsklick**) enthalten alle Befehle mit ihren Tastenkürzeln.

## Quelltext und Ansicht

- **Strg+/** schaltet zwischen der gerenderten Ansicht und dem reinen Markdown-Quelltext um (auch **Ansicht → Quelltextmodus** und unten links in der Statusleiste).
- **Ansicht → Fokus-Modus** (F8) blendet alles außer dem aktuellen Absatz ab, **Schreibmaschinen-Modus** (F9) hält die Zeile mit dem Cursor in der Fenstermitte.
- **Strg++** / **Strg+-** vergrößern und verkleinern die Schrift des Dokuments, **Strg+Shift+0** stellt die Originalgröße wieder her.
- **Ansicht → Werkzeugleiste** zeigt eine Knopfleiste mit den wichtigsten Befehlen.

## Dateien und Ordner

- **Datei → Ordner öffnen** zeigt einen Ordner in der Seitenleiste (**Strg+W** blendet sie ein und aus). Die Reiter zeigen den **Dateibaum**, die **Gliederung** des Dokuments und die **Artikel** des Ordners (Strg+Shift+3, 1, 2).
- Ein Rechtsklick im Dateibaum legt Dateien und Ordner an, benennt sie um oder löscht sie (in den Papierkorb).
- **Strg+P** öffnet schnell eine Datei aus dem Ordner, **Strg+Shift+F** sucht (und ersetzt) in allen Dateien des Ordners, auch mit regulären Ausdrücken.
- Mehrere Dokumente öffnen sich als **Tabs**; **Strg+Tab** wechselt zwischen ihnen.

## Speichern und Sicherheit

- **Strg+S** speichert. Velotype schreibt unveränderte Teile genau so zurück, wie sie in der Datei standen.
- **Datei → Automatisch speichern** speichert Dokumente kurz nach jeder Änderung.
- Bevor eine Datei überschrieben wird, legt Velotype eine **Sicherungskopie** an (**Datei → Sicherungskopien anzeigen**).
- Unbenannte Dokumente werden beim Beenden als Entwurf aufgehoben und beim nächsten Start wieder geöffnet.

## Bilder

- Bilder kannst du einfügen, hineinziehen oder mit **Strg+Shift+I** aus einer Datei wählen. Wohin eingefügte Bilder kopiert werden, legst du unter **Einstellungen → Bild** fest.
- **Format → Bild** und ein Rechtsklick auf ein Bild bieten Größe, Syntax, Kopieren und Verschieben der Bilddatei sowie das Hochladen mit einem eigenen Befehl (zum Beispiel PicGo).

## Export und Drucken

- **Export** erzeugt HTML (mit oder ohne Styles) und PDF; mit installiertem [Pandoc](https://pandoc.org) auch Word, OpenDocument, LaTeX, EPUB und mehr. **Strg+Shift+E** wiederholt den letzten Export.
- **Datei → Drucken…** (Alt+Shift+P) öffnet das Dokument druckfertig im Browser.
- **Datei → Importieren…** wandelt Word-, OpenDocument- und andere Dateien mit Pandoc in Markdown um.

## Aussehen

- Das Menü **Theme** wechselt das Farbschema; Velotype kann der hellen oder dunklen Systemdarstellung folgen.
- Typora-Themes (`.css`) und eigene Velotype-Themes legst du in den Ordner, den **Theme → Theme-Ordner öffnen** zeigt.
- Schriften, Statusleiste, Tastenkürzel und mehr stellst du unter **Datei → Einstellungen** ein.

## Mehr

- **Hilfe → Markdown-Referenz** zeigt alle Formatierungen mit Beispielen.
- **Hilfe → Tastenkürzel** listet alle Befehle mit ihrer aktuellen Belegung.
