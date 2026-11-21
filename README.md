# LightPDF Studio

Freier, quelloffener PDF-Reader und -Editor – **komplett in Rust**, eine Codebasis für **Windows, macOS, Linux, Android und iOS**, mit voller Touch-Unterstützung.

Version 3 ist eine Neuentwicklung von LightPDF Studio 2 (React + Electron + Capacitor). Kein Webview, kein JavaScript, kein C/C++-PDF-Kern: Anzeige, Bearbeitung und Oberfläche sind reines Rust.

## Was ist neu in 3.0

- **Pure Rust:** [hayro](https://github.com/LaurenzV/hayro) rendert die Seiten, [lopdf](https://github.com/J-F-Liu/lopdf) bearbeitet sie, [egui](https://github.com/emilk/egui) zeichnet die Oberfläche.
- **Touch ohne Markierungs-Pannen:** Ein Gesten-Schiedsrichter (`crates/lightpdf-core/src/gesture.rs`) entscheidet für jede Geste, ob gezeichnet oder navigiert wird:
  - Zwei Finger = immer Zoomen/Verschieben. Ein angefangener Strich wird **verworfen**, statt als Fehlstrich liegen zu bleiben.
  - Der Finger, der nach dem Pinch liegen bleibt, beginnt **keinen** neuen Strich.
  - Textfeld, Notiz, Unterschrift und Stempel entstehen nur durch ein sauberes Antippen mit einem Finger. Wischen mit diesen Werkzeugen scrollt.
  - Ein Strich beginnt genau dort, wo der Finger aufgesetzt hat, obwohl erst nach ein paar Pixeln entschieden wird.
  - Pinch mit dem Radierer löscht nichts. Liegt ein Stift (Pen/Maus) auf, werden Handballen-Berührungen ignoriert.
  - Abgesichert durch Unit-Tests, einen Fuzz-Test mit 3000 zufälligen Mehrfinger-Abläufen und UI-Tests, die die echte App mit synthetischen Touch-Ereignissen steuern.
- **Kompatibel:** Kommentare aus LightPDF Studio 2 (`/LPData`) lassen sich weiter bearbeiten. Die Android-App verwendet dieselbe App-ID und denselben Schlüssel und installiert sich als Update über v2.

## Funktionen

| Bereich | Funktionen |
|---|---|
| **Lesen** | flüssiges Scrollen mit Schwung, Pinch-Zoom, Doppeltipp-Zoom, Strg+Mausrad, Trackpad-Zoom, Seitenbreite/ganze Seite, fortlaufend / horizontal / Doppelseite, Ansicht drehen, Nacht- & Sepia-Modus, Vollbild, Miniaturen, Lesezeichen (Inhaltsverzeichnis), Volltextsuche mit Trefferanzeige, klickbare Links, zuletzt gelesene Seite merken. Bei hohem Zoom wird der sichtbare Ausschnitt scharf nachgerendert. |
| **Kommentieren** | Markieren, Unterstreichen, Durchstreichen (per Textauswahl, auch bei gedrehtem Text), Stift, Textmarker, Rechteck, Ellipse, Linie, Pfeil, Textfeld, Notiz, Stempel (Genehmigt, Entwurf, Vertraulich … + eigene), Bilder, Radierer, Auswählen/Verschieben/Größe ändern, Farben, Stärke, Deckkraft, Kommentar zu jedem Objekt, Kommentarliste, markierten Text kopieren, Rückgängig/Wiederholen |
| **Ausfüllen & Signieren** | PDF-Formulare ausfüllen (Text, Checkboxen, Auswahllisten), ✓ ✗ •, Datum, Unterschrift zeichnen / tippen / als Bild (weißer Hintergrund wird entfernt), Initialen, gespeicherte Unterschriften, Formular fixieren, echtes Schwärzen |
| **Seiten** | Seiten organisieren (Ziehen oder Tasten), drehen, löschen, duplizieren, verschieben, extrahieren, leere Seiten / PDFs einfügen. Kommentare wandern mit ihren Seiten, alles ist rückgängig zu machen. |
| **Werkzeuge** | Zusammenführen, Teilen (alle N Seiten oder nach Bereichen), Komprimieren (verlustfrei / mittel / stark), Kennwortschutz **AES-256** setzen & entfernen, Wasserzeichen, Seitenzahlen / Kopf- & Fußzeile (`{n}`, `{total}`, `{date}`, `{name}`), Export als Bilder (PNG/JPG im ZIP) oder Text, Dokumenteigenschaften, PDF aus Bildern, leeres PDF, Kommentare einbrennen |
| **Bibliothek** | Cover-Vorschau, Ordner, Markierungen, Suche, Sortierung, Raster/Liste, Mehrfachauswahl (lange drücken), Drag & Drop |
| **System** | Deutsch & Englisch, Hell/Dunkel/System, große Bedienelemente bei Touch, Telefon-Layout mit Werkzeugen unten (Daumenreichweite), Tastaturkürzel (Strg+Z/Y/S/F, Entf, V/H/P/T/N/E …) |

**Kommentare sind echte PDF-Annotationen** mit Erscheinungsbild. Sie erscheinen in Acrobat, Vorschau, Chrome, Poppler usw. und werden beim erneuten Öffnen in LightPDF Studio wieder bearbeitbar.

### Plattformen

| Plattform | Paket | „Öffnen mit“ / Dateien |
|---|---|---|
| Windows | ZIP mit `LightPDF Studio.exe` | Doppelklick / „Öffnen mit“, Drag & Drop, Speichern überschreibt das Original |
| macOS | universelle DMG (Apple Silicon + Intel), ad-hoc signiert | Öffnen-Dialog, Drag & Drop ins Fenster |
| Linux | `tar.gz` mit `.desktop`-Datei | „Öffnen mit“, Drag & Drop |
| Android | signierte APK (arm64, armv7, x86_64) | erscheint in „Öffnen mit“ und „Teilen“, speichert zurück in die Quelle, Exporte landen in `Download/LightPDF` |
| iOS / iPadOS | unsignierte IPA (selbst signieren, z. B. mit Xcode, AltStore oder Sideloadly) | „Öffnen in …“/Teilen legt Dateien in die App, der Ordner ist in der Dateien-App sichtbar |

### Bewusste Grenzen

- **Schwärzen** baut die betroffenen Seiten als Bild neu auf. Der Inhalt unter den Balken ist dadurch wirklich entfernt, die Seite ist danach aber nicht mehr durchsuchbar.
- **Komprimieren „mittel/stark“** rastert alle Seiten (Text danach nicht mehr markierbar). „Verlustfrei“ behält alles.
- Fließtext im PDF direkt umschreiben und OCR sind nicht enthalten.
- Fremde Kommentare (z. B. aus Acrobat) werden angezeigt, sind aber nicht bearbeitbar.
- Gegenüber v2 noch nicht wieder enthalten: Word-/EPUB-Dateien lesen, Vorlesen. Drucken öffnet das PDF auf dem Desktop im System-Viewer, auf Android wird exportiert.
- macOS: Doppelklick im Finder auf ein PDF übergibt die Datei noch nicht (fehlt in winit). Öffnen-Dialog und Drag & Drop funktionieren.
- Textfelder und Formulare verwenden die PDF-Standardschriften (WinAnsi). Zeichen außerhalb davon werden als `?` geschrieben, wie in v2.

## Entwickeln

```bash
cargo run --release              # Desktop-App starten (optional: Pfad zu einem PDF)
cargo test --workspace           # alle Tests (PDF-Engine, Gesten, UI)
```

Android (Android SDK + NDK, JDK, `cargo install cargo-apk`):

```bash
cargo apk build -p lightpdf-studio --release --lib
# fügt MainActivity (Java, System-Dateiauswahl) hinzu und signiert neu
packaging/android/add-java.sh target/release/apk/LightPDF-Studio.apk <keystore.jks> <passwort>
```

iOS (auf einem Mac mit Xcode): `cargo build --release --target aarch64-apple-ios -p lightpdf-studio`. Wie daraus ein App-Bundle wird, steht im Workflow.

`.github/workflows/build.yml` testet und baut für alle fünf Plattformen und hängt die Pakete bei einem Tag `v*` an ein Release.

**Android-Signatur:** `crates/lightpdf-studio/android/lightpdf-release.jks` ist der mitgelieferte Test-Schlüssel aus v2 (Kennwort in `keystore.properties`). Für den Play Store einen eigenen Schlüssel erzeugen und beides ersetzen.

### Aufbau

```
crates/lightpdf-core/      PDF-Engine ohne Oberfläche
  gesture.rs               Gesten-Schiedsrichter (Touch / Stift / Maus)
  render.rs                Seiten rendern (hayro) in Hintergrund-Threads, Textebene für Suche & Markieren
  annotio.rs               Kommentare ⇄ echte PDF-Annotationen
  ops.rs                   alle Bearbeitungsfunktionen (Seiten, Werkzeuge, Formulare, Verschlüsselung …)
  pdfdoc.rs, geom.rs, text.rs, model.rs
crates/lightpdf-studio/    App (egui/eframe)
  reader/                  Ansicht, Eingabe, Zeichnen, Werkzeugleisten
  library.rs, organizer.rs, dialogs.rs, signature.rs, browser.rs (nur iOS), platform.rs, storage.rs
packaging/                 Info.plist (macOS/iOS), .desktop-Datei, Symbole (aus icon.svg: make-icons.py)
```

## Herkunft

Dieses Projekt basiert auf **LightPDF** (Android-PDF-Reader mit Markierungsfunktion). Übernommen wurden das Design (Creme/Salbeigrün), das App-Symbol und die Übersetzungen. Symbole: [Lucide](https://lucide.dev) (ISC-Lizenz, `crates/lightpdf-studio/assets/LUCIDE-LICENSE`). Bitte stelle sicher, dass du die Rechte an diesen Bestandteilen hast, bevor du das Projekt veröffentlichst.

## Lizenz

MIT – siehe [LICENSE](LICENSE).
