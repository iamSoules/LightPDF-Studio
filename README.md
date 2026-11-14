# LightPDF Studio

Freier, quelloffener PDF-Reader und -Editor für **Windows, macOS und Android** – mit den wichtigsten Funktionen von Adobe Acrobat. Basiert auf [LightPDF](#herkunft) (React + Capacitor) und wurde komplett neu aufgebaut: flüssigeres Interface, echte PDF-Bearbeitung, alles offline.

## Funktionen

| Bereich | Funktionen |
|---|---|
| **Lesen** | flüssiges Scrollen (nur sichtbare Seiten werden gerendert), Pinch-Zoom, Doppeltipp-Zoom, Strg+Mausrad, Seitenbreite/ganze Seite, fortlaufend / horizontal / Einzelseite / Doppelseite, Ansicht drehen, Nacht- & Sepia-Modus, Vollbild, Vorlesen, Miniaturen, Inhaltsverzeichnis (Lesezeichen), Volltextsuche mit Trefferanzeige, klickbare Links, zuletzt gelesene Seite merken |
| **Kommentieren** | Markieren, Unterstreichen, Durchstreichen (per Textauswahl), Notizen, Textfelder, Stift, freier Textmarker, Rechteck, Ellipse, Linie, Pfeil, Stempel (Genehmigt, Entwurf, Vertraulich … + eigene), Bilder, Radierer, Auswählen/Verschieben/Größe ändern, Farben, Stärke, Deckkraft, Kommentar zu jedem Objekt, Kommentarliste, Rückgängig/Wiederholen |
| **Ausfüllen & Signieren** | PDF-Formulare ausfüllen (Textfelder, Checkboxen, Auswahllisten), Text, ✓ ✗ ●, Datum, Unterschrift zeichnen / tippen / als Bild (weißer Hintergrund wird entfernt), Initialen, gespeicherte Unterschriften, Formular fixieren |
| **Seiten** | Seiten organisieren (Drag & Drop), drehen, löschen, duplizieren, extrahieren, leere Seiten / PDFs / Bilder einfügen |
| **Werkzeuge** | Zusammenführen, Teilen (alle N Seiten oder nach Bereichen), Komprimieren (verlustfrei / mittel / stark), Kennwortschutz (AES) setzen & entfernen, echtes Schwärzen, Wasserzeichen, Seitenzahlen / Kopf- & Fußzeile, Export als Bilder (PNG/JPG, ZIP) oder Text, Dokumenteigenschaften, Drucken, PDF aus Bildern, leeres PDF |
| **Bibliothek** | Ordner, Markierungen, Suche, Sortierung, Raster/Liste, Mehrfachauswahl, Drag & Drop, Word- (.docx) und EPUB-Dateien lesen |
| **System** | Desktop: „Öffnen mit“/Doppelklick, Speichern überschreibt die Originaldatei (Strg+S). Android: erscheint als PDF-App in „Öffnen mit“ und „Teilen“, randlose Darstellung, Zurück-Taste. Deutsch & Englisch, Hell/Dunkel |

**Kommentare sind echte PDF-Annotationen** – sie erscheinen in Acrobat, Vorschau, Chrome usw. und werden beim erneuten Öffnen in LightPDF Studio wieder bearbeitbar.

### Bewusste Grenzen
- **Schwärzen** baut die betroffenen Seiten als Bild neu auf. Dadurch ist der Inhalt unter den Balken wirklich entfernt, die Seite ist danach aber nicht mehr durchsuchbar.
- **Komprimieren „mittel/stark“** rastert alle Seiten (Text nicht mehr markierbar). „Verlustfrei“ behält alles.
- Vorhandenen Fließtext im PDF direkt umschreiben (wie „PDF bearbeiten“ in Acrobat) und OCR sind nicht enthalten.
- Fremde Kommentare (z. B. aus Acrobat) werden angezeigt, sind aber nicht in LightPDF bearbeitbar.

## Installation

- **Windows:** `LightPDF-Studio-…-Windows-portable.exe` starten – keine Installation. Alternativ das ZIP entpacken und `LightPDF Studio.exe` starten (startet schneller).
- **macOS:** DMG öffnen, App nach „Programme“ ziehen. Beim ersten Start einmal freigeben (siehe `Zuerst lesen.txt` in der DMG), da die App nicht von Apple notarisiert ist.
- **Android:** APK aufs Handy kopieren und öffnen („Installation aus unbekannten Quellen“ erlauben).

## Entwickeln

```bash
npm install
npm run dev            # Web-Version unter http://localhost:3000
npm run electron       # Desktop-App starten
npm run dist:win       # Windows: portable EXE + ZIP   (release/)
npm run dist:mac       # macOS: .app                   (release/) – DMG am besten auf einem Mac bauen
npm run dist:android   # Android-APK  (android/app/build/outputs/apk/release/)
```

Die Ordner `.github/workflows` enthalten einen Build für alle drei Plattformen auf GitHub Actions. Auf dem macOS-Runner entsteht dort eine native, komprimierte DMG.

**Android-Signatur:** `android/lightpdf-release.jks` ist ein mitgelieferter Test-Schlüssel (Kennwort in `android/keystore.properties`). Für eine Veröffentlichung im Play Store einen eigenen Schlüssel erzeugen und die beiden Dateien ersetzen.

### Aufbau

```
src/
  components/   Bibliothek, Reader, Viewer (pdf.js), Kommentar-Ebene, Seitenleiste,
                Seiten organisieren, Unterschrift & Stempel, Werkzeug-Dialoge
  lib/          pdfjs.ts (Rendern), annotio.ts (Kommentare ⇄ PDF-Annotationen),
                pdfops.ts (alle Bearbeitungsfunktionen), db.ts (IndexedDB),
                platform.ts (Web / Electron / Android), library.ts (Import)
electron/       Desktop-Hauptprozess + Preload
android/        Capacitor-Projekt (MainActivity: Öffnen mit, Teilen, Systemleisten)
tests/          Tests der PDF-Funktionen (node tests/run.mjs tests/pdfops.test.ts)
```

Technik: React 19, Vite, Tailwind 4, [pdf.js](https://mozilla.github.io/pdf.js/) (Anzeige), [@cantoo/pdf-lib](https://github.com/cantoo-scribe/pdf-lib) (Bearbeitung, Verschlüsselung), Electron, Capacitor.

## Herkunft

Dieses Projekt basiert auf **LightPDF** (Android-PDF-Reader mit Markierungsfunktion). Übernommen wurden das Design (Creme/Salbeigrün, Nunito), App-Symbol, der DOCX/EPUB-Parser und die Android-Anbindung („Öffnen mit“, Zurück-Taste). Bitte stelle sicher, dass du die Rechte an diesen Bestandteilen hast, bevor du das Projekt veröffentlichst.

## Lizenz

MIT – siehe [LICENSE](LICENSE).
