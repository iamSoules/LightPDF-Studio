// Alle Bearbeitungsfunktionen auf Dokumentebene (pdf-lib + pdf.js für Rasterisierung).
import {
  PDFDocument,
  StandardFonts,
  degrees,
  rgb,
  PDFName,
  PDFDict,
  PDFArray,
  PDFNumber,
  PDFRef,
  PDFRawStream,
  PDFContentStream,
  decodePDFRawStream,
  pushGraphicsState,
  popGraphicsState,
  concatTransformationMatrix,
  drawObject,
  type PDFFont,
} from "@cantoo/pdf-lib";
import JSZip from "jszip";
import type { Annotation, Rect } from "../types";
import { extractOwnAnnotations, hexToRgb, writeAnnotations, wrapText } from "./annotio";
import { openPdf, renderPageToCanvas, pageText, type PDFDocumentProxy } from "./pdfjs";

export type Bytes = Uint8Array;

export async function load(bytes: Bytes | ArrayBuffer, password?: string) {
  const d = await PDFDocument.load(bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes), {
    password,
    ignoreEncryption: true,
    updateMetadata: false,
    throwOnInvalidObject: false,
  } as any);
  if (password !== undefined) {
    // Inhalt ist jetzt entschlüsselt → Verschlüsselungsobjekt entfernen, sonst wäre die Datei kaputt
    const enc = d.context.trailerInfo.Encrypt;
    if (enc instanceof PDFRef) d.context.delete(enc);
    for (const [ref, obj] of d.context.enumerateIndirectObjects()) {
      if (obj instanceof PDFDict && obj.get(PDFName.of("Filter"))?.toString() === "/Standard" && obj.has(PDFName.of("O")))
        d.context.delete(ref);
    }
    delete (d.context.trailerInfo as any).Encrypt;
    (d as any).isEncrypted = false;
  }
  return d;
}

/**
 * Speichern. Das Format (kompakte Objekt-Streams oder klassisch) wählt die Bibliothek selbst:
 * PDF/A-1 verbietet Objekt-Streams, ältere PDF-Versionen kennen sie nicht.
 */
const save = (d: PDFDocument) => d.save();

/** Ist das Dokument als PDF/A gekennzeichnet? (steht in den XMP-Metadaten) */
function pdfaPart(d: PDFDocument): string | null {
  const xml = readXmp(d);
  const m = xml?.match(/pdfaid:part\s*=\s*["'](\d)["']|<pdfaid:part>\s*(\d)\s*<\/pdfaid:part>/);
  return m ? (m[1] ?? m[2]) : null;
}

function readXmp(d: PDFDocument): string | null {
  const obj = d.catalog.lookup(PDFName.of("Metadata"));
  if (!(obj instanceof PDFRawStream)) return null;
  try {
    return new TextDecoder().decode(decodePDFRawStream(obj).decode());
  } catch {
    return null;
  }
}

/** PDF/A-Kennzeichnung entfernen (nötig, bevor ein Dokument verschlüsselt werden darf) */
function stripPdfA(d: PDFDocument) {
  const xml = readXmp(d);
  if (!xml) return;
  const cleaned = xml
    .replace(/\s*pdfaid:(part|conformance|amd|rev)\s*=\s*("[^"]*"|'[^']*')/g, "")
    .replace(/<pdfaid:(part|conformance|amd|rev)>[^<]*<\/pdfaid:\1>\s*/g, "");
  const stream = d.context.stream(cleaned, { Type: "Metadata", Subtype: "XML" });
  d.catalog.set(PDFName.of("Metadata"), d.context.register(stream));
}

export async function isPdfA(bytes: Bytes, pw?: string) {
  try {
    return pdfaPart(await load(bytes, pw));
  } catch {
    return null;
  }
}

// ------------------------------------------------------------------ Import / Export
/** Beim Öffnen: eigene Annotationen herauslösen, damit sie bearbeitbar bleiben */
export async function importPdf(bytes: Bytes, password?: string): Promise<{ base: Bytes; annots: Annotation[] }> {
  try {
    const d = await load(bytes, password);
    const annots = extractOwnAnnotations(d);
    if (!annots.length) return { base: bytes, annots: [] };
    return { base: await save(d), annots };
  } catch {
    return { base: bytes, annots: [] };
  }
}

/** Fertiges PDF erzeugen: Basis + Kommentare (+ optional Kennwort wieder setzen) */
export async function exportPdf(base: Bytes, annots: Annotation[], opts: { password?: string; flatten?: boolean } = {}) {
  const d = await load(base, opts.password);
  await writeAnnotations(
    d,
    annots.filter((a) => a.type !== "redact" || !opts.flatten),
  );
  if (opts.flatten) flattenAnnotations(d);
  if (opts.password) {
    if (pdfaPart(d)) stripPdfA(d);
    d.encrypt({ userPassword: opts.password, ownerPassword: opts.password });
  }
  return save(d);
}

/** Annotationen fest in den Seiteninhalt einbrennen (nicht mehr bearbeitbar) */
function flattenAnnotations(d: PDFDocument) {
  try {
    d.getForm().flatten();
  } catch {
    /* kein Formular */
  }
  const num = (arr: PDFArray | undefined, n: number, def: number[]) => {
    if (!arr) return def;
    const out: number[] = [];
    for (let i = 0; i < n; i++) out.push((arr.lookup(i) as PDFNumber)?.asNumber?.() ?? def[i]);
    return out;
  };
  for (const page of d.getPages()) {
    const annots = page.node.lookupMaybe(PDFName.of("Annots"), PDFArray);
    if (!annots) continue;
    const keep: unknown[] = [];
    for (let i = 0; i < annots.size(); i++) {
      const a = annots.lookupMaybe(i, PDFDict);
      if (!a) continue;
      const sub = a.lookupMaybe(PDFName.of("Subtype"), PDFName)?.asString();
      if (sub === "/Link" || sub === "/Popup" || sub === "/Widget") {
        keep.push(annots.get(i));
        continue;
      }
      const flags = (a.lookupMaybe(PDFName.of("F"), PDFNumber)?.asNumber() ?? 0) | 0;
      const ap = a.lookupMaybe(PDFName.of("AP"), PDFDict);
      const n = ap?.get(PDFName.of("N"));
      if (!(n instanceof PDFRef) || flags & 2) continue; // versteckt oder ohne Aussehen
      const xo = d.context.lookup(n);
      if (!(xo instanceof PDFRawStream || xo instanceof PDFContentStream || (xo as any)?.dict)) continue;
      const [rx1, ry1, rx2, ry2] = num(a.lookupMaybe(PDFName.of("Rect"), PDFArray), 4, [0, 0, 0, 0]);
      const [bx1, by1, bx2, by2] = num((xo as any).dict.lookupMaybe(PDFName.of("BBox"), PDFArray), 4, [rx1, ry1, rx2, ry2]);
      const sx = (rx2 - rx1) / (bx2 - bx1 || 1),
        sy = (ry2 - ry1) / (by2 - by1 || 1);
      const name = page.node.newXObject("Flat", n);
      page.pushOperators(
        pushGraphicsState(),
        concatTransformationMatrix(sx, 0, 0, sy, rx1 - bx1 * sx, ry1 - by1 * sy),
        drawObject(name),
        popGraphicsState(),
      );
    }
    if (keep.length) page.node.set(PDFName.of("Annots"), d.context.obj(keep as any));
    else page.node.delete(PDFName.of("Annots"));
  }
}

// ------------------------------------------------------------------ Seiten organisieren
export async function rotatePages(bytes: Bytes, pages: number[], delta: number, pw?: string) {
  const d = await load(bytes, pw);
  for (const i of pages) {
    const p = d.getPage(i);
    p.setRotation(degrees((((p.getRotation().angle + delta) % 360) + 360) % 360));
  }
  return save(d);
}

export async function deletePages(bytes: Bytes, pages: number[], pw?: string) {
  const d = await load(bytes, pw);
  [...pages].sort((a, b) => b - a).forEach((i) => d.removePage(i));
  return save(d);
}

/** order: neue Reihenfolge als Liste alter Indizes (darf Duplikate enthalten) */
export async function reorderPages(bytes: Bytes, order: number[], pw?: string) {
  const d = await load(bytes, pw);
  const copies = await d.copyPages(d, order);
  const n = d.getPageCount();
  for (let i = n - 1; i >= 0; i--) d.removePage(i);
  copies.forEach((p) => d.addPage(p));
  return save(d);
}

export async function insertBlank(bytes: Bytes, at: number, pw?: string) {
  const d = await load(bytes, pw);
  const ref = d.getPage(Math.max(0, Math.min(at, d.getPageCount()) - 1));
  const { width, height } = ref ? ref.getSize() : { width: 595.28, height: 841.89 };
  d.insertPage(at, [width, height]);
  return save(d);
}

export async function insertPdf(bytes: Bytes, other: Bytes, at: number, pw?: string) {
  const d = await load(bytes, pw);
  const o = await load(other);
  const pages = await d.copyPages(o, o.getPageIndices());
  pages.forEach((p, i) => d.insertPage(at + i, p));
  return save(d);
}

export async function extractPages(bytes: Bytes, pages: number[], pw?: string) {
  const src = await load(bytes, pw);
  const d = await PDFDocument.create();
  (await d.copyPages(src, pages)).forEach((p) => d.addPage(p));
  return save(d);
}

export async function merge(list: Bytes[]) {
  const d = await PDFDocument.create();
  for (const b of list) {
    const s = await load(b);
    (await d.copyPages(s, s.getPageIndices())).forEach((p) => d.addPage(p));
  }
  return save(d);
}

/** Bereiche wie "1-3, 5, 8-" → Seitenindizes (0-basiert) */
export function parseRanges(spec: string, total: number): number[][] {
  const groups: number[][] = [];
  for (const part of spec.split(/[;,]/).map((s) => s.trim()).filter(Boolean)) {
    const m = part.match(/^(\d*)\s*-\s*(\d*)$/);
    let a: number, b: number;
    if (m) {
      a = m[1] ? +m[1] : 1;
      b = m[2] ? +m[2] : total;
    } else {
      a = b = +part;
    }
    if (!a || !b) continue;
    a = Math.max(1, Math.min(total, a));
    b = Math.max(1, Math.min(total, b));
    const g: number[] = [];
    for (let i = Math.min(a, b); i <= Math.max(a, b); i++) g.push(i - 1);
    groups.push(g);
  }
  return groups;
}

export async function split(bytes: Bytes, groups: number[][], pw?: string) {
  const out: Bytes[] = [];
  for (const g of groups) out.push(await extractPages(bytes, g, pw));
  return out;
}

// ------------------------------------------------------------------ Erstellen
async function imageToJpeg(file: Blob): Promise<{ bytes: Uint8Array; w: number; h: number; png: boolean }> {
  const type = file.type;
  const buf = new Uint8Array(await file.arrayBuffer());
  const bmp = await createImageBitmap(file);
  const w = bmp.width,
    h = bmp.height;
  if (type === "image/jpeg" || type === "image/png") {
    bmp.close();
    return { bytes: buf, w, h, png: type === "image/png" };
  }
  const c = document.createElement("canvas");
  c.width = w;
  c.height = h;
  c.getContext("2d")!.drawImage(bmp, 0, 0);
  bmp.close();
  const blob: Blob = await new Promise((r) => c.toBlob((b) => r(b!), "image/jpeg", 0.92));
  return { bytes: new Uint8Array(await blob.arrayBuffer()), w, h, png: false };
}

export type PageSizeOpt = "fit" | "a4" | "letter";
const SIZES = { a4: [595.28, 841.89], letter: [612, 792] } as const;

export async function imagesToPdf(files: Blob[], size: PageSizeOpt = "a4", margin = 24) {
  const d = await PDFDocument.create();
  for (const f of files) {
    const im = await imageToJpeg(f);
    const img = im.png ? await d.embedPng(im.bytes) : await d.embedJpg(im.bytes);
    if (size === "fit") {
      const p = d.addPage([im.w * 0.75, im.h * 0.75]);
      p.drawImage(img, { x: 0, y: 0, width: im.w * 0.75, height: im.h * 0.75 });
      continue;
    }
    let [pw, ph]: number[] = [...SIZES[size]];
    if (im.w > im.h) [pw, ph] = [ph, pw];
    const s = Math.min((pw - 2 * margin) / im.w, (ph - 2 * margin) / im.h);
    const p = d.addPage([pw, ph]);
    p.drawImage(img, { x: (pw - im.w * s) / 2, y: (ph - im.h * s) / 2, width: im.w * s, height: im.h * s });
  }
  return save(d);
}

export async function blankPdf(pages = 1, size: "a4" | "letter" = "a4") {
  const d = await PDFDocument.create();
  for (let i = 0; i < pages; i++) d.addPage([...SIZES[size]] as [number, number]);
  return save(d);
}

// ------------------------------------------------------------------ Stempeln: Wasserzeichen, Seitenzahlen, Kopf-/Fußzeile
export interface WatermarkOpts {
  text: string;
  size: number;
  color: string;
  opacity: number;
  angle: number;
  pages?: number[]; // 0-basiert, leer = alle
}

export async function watermark(bytes: Bytes, o: WatermarkOpts, pw?: string) {
  const d = await load(bytes, pw);
  const font = await d.embedFont(StandardFonts.HelveticaBold);
  const [r, g, b] = hexToRgb(o.color);
  const pages = d.getPages();
  const idx = o.pages?.length ? o.pages : pages.map((_, i) => i);
  for (const i of idx) {
    const p = pages[i];
    if (!p) continue;
    const v = visual(p);
    const text = safe(font, o.text);
    const tw = font.widthOfTextAtSize(text, o.size);
    const rad = (o.angle * Math.PI) / 180;
    // Textanfang so wählen, dass die Mitte des Textes in der Seitenmitte liegt (in sichtbaren Koordinaten)
    const vx = v.vw / 2 - (tw / 2) * Math.cos(rad) + (o.size / 3) * Math.sin(rad);
    const vy = v.vh / 2 - (tw / 2) * Math.sin(rad) - (o.size / 3) * Math.cos(rad);
    const [x, y] = v.toUser(vx, vy);
    p.drawText(text, { x, y, size: o.size, font, color: rgb(r, g, b), opacity: o.opacity, rotate: degrees(o.angle + v.R) });
  }
  return save(d);
}

/** Sichtbare Seitengeometrie (berücksichtigt /Rotate): visuelle Koordinaten → PDF-Koordinaten */
function visual(p: import("@cantoo/pdf-lib").PDFPage) {
  const R = ((p.getRotation().angle % 360) + 360) % 360;
  const box = p.getMediaBox();
  const W = box.width,
    H = box.height,
    x0 = box.x,
    y0 = box.y;
  const vw = R % 180 ? H : W,
    vh = R % 180 ? W : H;
  const toUser = (vx: number, vy: number): [number, number] => {
    switch (R) {
      case 90:
        return [x0 + W - vy, y0 + vx];
      case 180:
        return [x0 + W - vx, y0 + H - vy];
      case 270:
        return [x0 + vy, y0 + H - vx];
      default:
        return [x0 + vx, y0 + vy];
    }
  };
  return { R, vw, vh, toUser };
}

export type Position = "tl" | "tc" | "tr" | "bl" | "bc" | "br";
export interface StampTextOpts {
  template: string; // {n}, {total}, {date}, {name}
  position: Position;
  size: number;
  color: string;
  start: number;
  margin: number;
  skipFirst: boolean;
  name: string;
}

export async function stampText(bytes: Bytes, o: StampTextOpts, pw?: string) {
  const d = await load(bytes, pw);
  const font = await d.embedFont(StandardFonts.Helvetica);
  const [r, g, b] = hexToRgb(o.color);
  const pages = d.getPages();
  const date = new Date().toLocaleDateString();
  pages.forEach((p, i) => {
    if (o.skipFirst && i === 0) return;
    const text = safe(
      font,
      o.template
        .replaceAll("{n}", String(i + o.start))
        .replaceAll("{total}", String(pages.length + o.start - 1))
        .replaceAll("{date}", date)
        .replaceAll("{name}", o.name),
    );
    const v = visual(p);
    const tw = font.widthOfTextAtSize(text, o.size);
    const vx = o.position.endsWith("l") ? o.margin : o.position.endsWith("r") ? v.vw - o.margin - tw : (v.vw - tw) / 2;
    const vy = o.position.startsWith("t") ? v.vh - o.margin - o.size : o.margin;
    const [x, y] = v.toUser(vx, vy);
    p.drawText(text, { x, y, size: o.size, font, color: rgb(r, g, b), rotate: degrees(v.R) });
  });
  return save(d);
}

function safe(font: PDFFont, s: string) {
  let out = "";
  for (const ch of s) {
    try {
      font.encodeText(ch);
      out += ch;
    } catch {
      out += "?";
    }
  }
  return out;
}

// ------------------------------------------------------------------ Schützen
export interface ProtectOpts {
  userPassword: string;
  ownerPassword?: string;
  allowPrint: boolean;
  allowCopy: boolean;
  allowModify: boolean;
}

export async function protect(bytes: Bytes, o: ProtectOpts, pw?: string) {
  const d = await load(bytes, pw);
  if (pdfaPart(d)) stripPdfA(d); // PDF/A verbietet Verschlüsselung
  d.encrypt({
    userPassword: o.userPassword,
    ownerPassword: o.ownerPassword || o.userPassword,
    permissions: {
      printing: o.allowPrint ? "highResolution" : undefined,
      copying: o.allowCopy,
      modifying: o.allowModify,
      annotating: o.allowModify,
      fillingForms: true,
      contentAccessibility: true,
      documentAssembly: o.allowModify,
    },
  } as any);
  return save(d);
}

export async function removePassword(bytes: Bytes, pw: string) {
  return save(await load(bytes, pw));
}

// ------------------------------------------------------------------ Metadaten
export interface Meta {
  title: string;
  author: string;
  subject: string;
  keywords: string;
  creator: string;
  producer: string;
  created?: string;
  modified?: string;
}

export async function getMeta(bytes: Bytes, pw?: string): Promise<Meta> {
  const d = await load(bytes, pw);
  return {
    title: d.getTitle() ?? "",
    author: d.getAuthor() ?? "",
    subject: d.getSubject() ?? "",
    keywords: d.getKeywords() ?? "",
    creator: d.getCreator() ?? "",
    producer: d.getProducer() ?? "",
    created: d.getCreationDate()?.toLocaleString(),
    modified: d.getModificationDate()?.toLocaleString(),
  };
}

export async function setMeta(bytes: Bytes, m: Partial<Meta>, pw?: string) {
  const d = await load(bytes, pw);
  if (m.title !== undefined) d.setTitle(m.title);
  if (m.author !== undefined) d.setAuthor(m.author);
  if (m.subject !== undefined) d.setSubject(m.subject);
  if (m.keywords !== undefined) d.setKeywords(m.keywords.split(/[,;]\s*/).filter(Boolean));
  d.setModificationDate(new Date());
  d.setProducer("LightPDF Studio");
  return save(d);
}

// ------------------------------------------------------------------ Formulare
export async function flattenForm(bytes: Bytes, pw?: string) {
  const d = await load(bytes, pw);
  d.getForm().flatten();
  return save(d);
}

export async function hasForm(bytes: Bytes, pw?: string) {
  try {
    return (await load(bytes, pw)).getForm().getFields().length > 0;
  } catch {
    return false;
  }
}

// ------------------------------------------------------------------ Rasterisieren: Schwärzen & Komprimieren
async function rasterPage(js: PDFDocumentProxy, pageNo: number, dpi: number, rects: Rect[] = []) {
  const page = await js.getPage(pageNo);
  const vp1 = page.getViewport({ scale: 1, rotation: 0 });
  const scale = dpi / 72;
  const canvas = await renderPageToCanvas(js, pageNo, vp1.width * scale, { dpr: 1, rotation: -page.rotate, withAnnotations: false });
  const ctx = canvas.getContext("2d")!;
  ctx.fillStyle = "#000";
  const vp = page.getViewport({ scale, rotation: 0 });
  for (const r of rects) {
    const [ax, ay] = vp.convertToViewportPoint(r[0], r[1]);
    const [bx, by] = vp.convertToViewportPoint(r[2], r[3]);
    ctx.fillRect(Math.min(ax, bx), Math.min(ay, by), Math.abs(bx - ax), Math.abs(by - ay));
  }
  return { canvas, view: page.view, rotate: page.rotate };
}

async function canvasJpeg(c: HTMLCanvasElement, q: number) {
  const blob: Blob = await new Promise((r) => c.toBlob((b) => r(b!), "image/jpeg", q));
  return new Uint8Array(await blob.arrayBuffer());
}

/**
 * Echtes Schwärzen: Betroffene Seiten werden als Bild neu aufgebaut,
 * der Text darunter ist danach wirklich entfernt (nicht nur überdeckt).
 */
export async function applyRedactions(bytes: Bytes, byPage: Map<number, Rect[]>, pw?: string, dpi = 200) {
  const js = await openPdf(bytes, pw);
  const d = await load(bytes, pw);
  for (const [pageNo, rects] of byPage) {
    const { canvas, view, rotate } = await rasterPage(js, pageNo, dpi, rects);
    const jpg = await d.embedJpg(await canvasJpeg(canvas, 0.9));
    const w = view[2] - view[0],
      h = view[3] - view[1];
    const np = d.insertPage(pageNo - 1, [w, h]);
    np.setRotation(degrees(rotate));
    np.drawImage(jpg, { x: 0, y: 0, width: w, height: h });
    d.removePage(pageNo);
  }
  await js.destroy();
  d.setModificationDate(new Date());
  return save(d);
}

/** Komprimieren: "lossless" = nur Struktur optimieren, sonst Seiten als JPEG neu aufbauen */
export async function compress(bytes: Bytes, level: "lossless" | "medium" | "strong", pw?: string, onProgress?: (i: number, n: number) => void) {
  if (level === "lossless") return save(await load(bytes, pw));
  const dpi = level === "medium" ? 150 : 96;
  const q = level === "medium" ? 0.72 : 0.55;
  const js = await openPdf(bytes, pw);
  const d = await PDFDocument.create();
  for (let i = 1; i <= js.numPages; i++) {
    const { canvas, view, rotate } = await rasterPage(js, i, dpi);
    const jpg = await d.embedJpg(await canvasJpeg(canvas, q));
    const w = view[2] - view[0],
      h = view[3] - view[1];
    const p = d.addPage([w, h]);
    p.setRotation(degrees(rotate));
    p.drawImage(jpg, { x: 0, y: 0, width: w, height: h });
    onProgress?.(i, js.numPages);
  }
  await js.destroy();
  const out = await save(d);
  return out.length < bytes.length ? out : bytes;
}

// ------------------------------------------------------------------ Exportieren
export async function exportImages(
  js: PDFDocumentProxy,
  fmt: "png" | "jpeg",
  dpi: number,
  pages: number[],
  onProgress?: (i: number, n: number) => void,
) {
  const zip = new JSZip();
  let i = 0;
  for (const p of pages) {
    const page = await js.getPage(p);
    const w = page.getViewport({ scale: 1, rotation: page.rotate }).width;
    const c = await renderPageToCanvas(js, p, (w * dpi) / 72, { dpr: 1 });
    const blob: Blob = await new Promise((r) => c.toBlob((b) => r(b!), "image/" + fmt, 0.9));
    zip.file(`Seite-${String(p).padStart(3, "0")}.${fmt === "jpeg" ? "jpg" : "png"}`, blob);
    onProgress?.(++i, pages.length);
  }
  return zip.generateAsync({ type: "uint8array" });
}

export async function exportText(js: PDFDocumentProxy) {
  const parts: string[] = [];
  for (let i = 1; i <= js.numPages; i++) parts.push(`--- Seite ${i} ---\n` + (await pageText(js, i)));
  return parts.join("\n\n");
}

export { wrapText };
