// Dateien in die Bibliothek aufnehmen
import type { DocMeta, DocumentFormat } from "../types";
import { db, uid } from "./db";
import { importPdf, imagesToPdf } from "./pdfops";
import { openPdf, PasswordNeeded, thumbnailDataUrl } from "./pdfjs";
import type { IncomingFile } from "./platform";

export function hashBytes(b: Uint8Array): string {
  // FNV-1a über eine Stichprobe + Länge (schnell auch bei großen Dateien)
  let h = 0x811c9dc5;
  const step = Math.max(1, Math.floor(b.length / 200_000));
  for (let i = 0; i < b.length; i += step) {
    h ^= b[i];
    h = Math.imul(h, 0x01000193);
  }
  return (h >>> 0).toString(16) + ":" + b.length;
}

const IMG = /\.(png|jpe?g|webp|gif|bmp|heic)$/i;

export function formatOf(name: string): DocumentFormat | "image" | null {
  const n = name.toLowerCase();
  if (n.endsWith(".pdf")) return "pdf";
  if (n.endsWith(".docx")) return "docx";
  if (n.endsWith(".epub")) return "epub";
  if (IMG.test(n)) return "image";
  return null;
}

const mime = (n: string) =>
  ({ png: "image/png", jpg: "image/jpeg", jpeg: "image/jpeg", webp: "image/webp", gif: "image/gif", bmp: "image/bmp", heic: "image/heic" })[n.split(".").pop()!.toLowerCase()] ?? "";

async function describe(meta: DocMeta, bytes: Uint8Array) {
  try {
    const d = await openPdf(bytes);
    meta.pages = d.numPages;
    meta.thumb = await thumbnailDataUrl(d);
    d.destroy();
  } catch (e) {
    if (e instanceof PasswordNeeded) meta.encrypted = true;
  }
}

/**
 * Nimmt Dateien auf. Bilder werden zu einem PDF zusammengefasst.
 * Dateien mit bekanntem Pfad (Desktop) werden nicht doppelt angelegt.
 */
export async function importFiles(files: IncomingFile[], folderId: string | null = null, onProgress?: (i: number, n: number) => void): Promise<DocMeta[]> {
  const out: DocMeta[] = [];
  const existing = await db.listDocs();
  const images = files.filter((f) => formatOf(f.name) === "image");
  const others = files.filter((f) => formatOf(f.name) && formatOf(f.name) !== "image");
  if (images.length) {
    const pdf = await imagesToPdf(images.map((f) => new Blob([f.data], { type: mime(f.name) })), "fit");
    const name = (images.length === 1 ? images[0].name.replace(IMG, "") : "Bilder " + new Date().toLocaleDateString()) + ".pdf";
    others.unshift({ name, data: pdf });
  }
  let i = 0;
  for (const f of others) {
    onProgress?.(++i, others.length);
    const fmt = formatOf(f.name) as DocumentFormat;
    const prev = f.path ? existing.find((d) => d.path === f.path) : undefined;
    if (prev && prev.diskHash === hashBytes(f.data)) {
      // Unveränderte Datei: vorhandenen Eintrag (mit allen Kommentaren) verwenden
      out.push(prev);
      continue;
    }
    const meta: DocMeta = prev
      ? { ...prev, size: f.data.length, openedAt: Date.now(), diskHash: hashBytes(f.data) }
      : {
          id: uid(),
          name: f.name,
          format: fmt,
          size: f.data.length,
          pages: 0,
          addedAt: Date.now(),
          openedAt: Date.now(),
          lastPage: 1,
          folderId,
          path: f.path,
          diskHash: f.path ? hashBytes(f.data) : undefined,
        };
    let data = f.data;
    let annots: Awaited<ReturnType<typeof importPdf>>["annots"] = [];
    if (fmt === "pdf") {
      const r = await importPdf(f.data);
      data = r.base;
      annots = r.annots;
      await describe(meta, data);
    }
    await db.putFile(meta.id, data);
    await db.putAnnots(meta.id, annots);
    await db.putDoc(meta);
    out.push(meta);
  }
  return out;
}

/** Ein neues, von der App erzeugtes PDF ablegen */
export async function addPdf(name: string, data: Uint8Array, folderId: string | null = null) {
  const [m] = await importFiles([{ name, data }], folderId);
  return m;
}
