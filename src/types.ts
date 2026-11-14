export type DocumentFormat = "pdf" | "docx" | "epub";

/** Eintrag in der Bibliothek */
export interface DocMeta {
  id: string;
  name: string;
  format: DocumentFormat;
  size: number;
  pages: number;
  addedAt: number;
  openedAt: number;
  lastPage: number;
  folderId: string | null;
  thumb?: string; // kleine Vorschau (data URL)
  encrypted?: boolean;
  /** Pfad auf der Festplatte (nur Desktop) – "Speichern" überschreibt dann diese Datei */
  path?: string;
  starred?: boolean;
  /** Prüfsumme der zuletzt auf die Festplatte geschriebenen Fassung */
  diskHash?: string;
}

export interface Folder {
  id: string;
  name: string;
  color: string;
  createdAt: number;
}

// ---------------------------------------------------------------- Kommentare / Markierungen
// Alle Koordinaten in PDF-Punkten im (ungedrehten) Seitenkoordinatensystem.
export type Pt = [number, number];
export type Rect = [number, number, number, number]; // x1, y1, x2, y2 (x1<x2, y1<y2)

interface AnnBase {
  id: string;
  page: number; // 1-basiert
  color: string;
  opacity: number;
  createdAt: number;
  author?: string;
  note?: string; // Kommentar zum Objekt
}

export type MarkupType = "highlight" | "underline" | "strike";

export type Annotation =
  | (AnnBase & { type: MarkupType; rects: Rect[]; text?: string })
  | (AnnBase & { type: "ink"; paths: Pt[][]; width: number; highlighter?: boolean })
  | (AnnBase & { type: "rect" | "ellipse"; rect: Rect; width: number; fill?: string | null })
  | (AnnBase & { type: "line" | "arrow"; from: Pt; to: Pt; width: number })
  | (AnnBase & { type: "text"; rect: Rect; text: string; fontSize: number; bold?: boolean })
  | (AnnBase & { type: "note"; at: Pt; text: string })
  | (AnnBase & { type: "image"; rect: Rect; src: string; kind: "signature" | "image" | "stamp" })
  | (AnnBase & { type: "redact"; rect: Rect });

export type AnnotationType = Annotation["type"];

export type Tool =
  | "hand"
  | "select"
  | MarkupType
  | "ink"
  | "highlighter"
  | "rect"
  | "ellipse"
  | "line"
  | "arrow"
  | "text"
  | "note"
  | "eraser"
  | "redact"
  | "signature"
  | "image"
  | "stamp";

export interface SavedSignature {
  id: string;
  src: string; // PNG data URL
  kind: "signature" | "initials";
  createdAt: number;
}

export interface Settings {
  lang: "de" | "en";
  theme: "light" | "dark" | "system";
  author: string;
  reading: "normal" | "night" | "sepia";
  scrollMode: "vertical" | "horizontal" | "wrapped" | "page";
  spread: "none" | "odd" | "even";
  sort: "recent" | "added" | "name" | "size";
  view: "grid" | "list";
}

// ---------------------------------------------------------------- DOCX / EPUB
export interface ParsedDocumentPage {
  pageNumber: number;
  title?: string;
  htmlContent?: string;
  textContent?: string;
}

export interface ParsedDocument {
  id: string;
  title: string;
  format: DocumentFormat;
  totalPages: number;
  pages: ParsedDocumentPage[];
  rawText?: string;
}
