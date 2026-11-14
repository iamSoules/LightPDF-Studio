// pdf.js laden (Legacy-Build = läuft auch in älteren Android-WebViews)
import * as pdfjsLib from "pdfjs-dist/legacy/build/pdf.mjs";
import workerUrl from "pdfjs-dist/legacy/build/pdf.worker.min.mjs?url";
import type { PDFDocumentProxy } from "pdfjs-dist";

if (typeof window !== "undefined") pdfjsLib.GlobalWorkerOptions.workerSrc = workerUrl;
(globalThis as any).pdfjsLib = pdfjsLib; // pdf_viewer.mjs erwartet das globale Objekt

export { pdfjsLib };
export type { PDFDocumentProxy };

export function docParams() {
  const base = new URL("./pdfjs/", document.baseURI).href;
  return {
    cMapUrl: base + "cmaps/",
    cMapPacked: true,
    standardFontDataUrl: base + "standard_fonts/",
    isEvalSupported: false,
  };
}

let viewerMod: Promise<typeof import("pdfjs-dist/legacy/web/pdf_viewer.mjs")> | null = null;
export function loadViewer() {
  if (!viewerMod) viewerMod = import("pdfjs-dist/legacy/web/pdf_viewer.mjs");
  return viewerMod;
}

export class PasswordNeeded extends Error {
  constructor(public wrong: boolean) {
    super("password");
  }
}

/** Öffnet ein PDF. Bei Kennwortschutz wird PasswordNeeded geworfen. */
export async function openPdf(data: ArrayBuffer | Uint8Array, password?: string): Promise<PDFDocumentProxy> {
  const bytes = data instanceof Uint8Array ? data.slice() : new Uint8Array(data.slice(0));
  const task = pdfjsLib.getDocument({ ...docParams(), data: bytes, password });
  return await new Promise((resolve, reject) => {
    task.onPassword = (_cb: unknown, reason: number) => {
      task.destroy();
      reject(new PasswordNeeded(reason === pdfjsLib.PasswordResponses.INCORRECT_PASSWORD));
    };
    task.promise.then(resolve, reject);
  });
}

/** Rendert eine Seite in ein Canvas mit gegebener Breite (CSS-Pixel). */
export async function renderPageToCanvas(
  doc: PDFDocumentProxy,
  pageNo: number,
  width: number,
  opts: { rotation?: number; dpr?: number; withAnnotations?: boolean } = {},
): Promise<HTMLCanvasElement> {
  const page = await doc.getPage(pageNo);
  const rot = ((page.rotate + (opts.rotation ?? 0)) % 360 + 360) % 360;
  const v1 = page.getViewport({ scale: 1, rotation: rot });
  const dpr = opts.dpr ?? Math.min(2, window.devicePixelRatio || 1);
  const scale = (width / v1.width) * dpr;
  const vp = page.getViewport({ scale, rotation: rot });
  const canvas = document.createElement("canvas");
  canvas.width = Math.ceil(vp.width);
  canvas.height = Math.ceil(vp.height);
  const ctx = canvas.getContext("2d")!;
  ctx.fillStyle = "#fff";
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  await page.render({
    canvasContext: ctx,
    viewport: vp,
    annotationMode: opts.withAnnotations === false ? pdfjsLib.AnnotationMode.DISABLE : pdfjsLib.AnnotationMode.ENABLE,
  }).promise;
  return canvas;
}

export async function thumbnailDataUrl(doc: PDFDocumentProxy, width = 180): Promise<string> {
  const c = await renderPageToCanvas(doc, 1, width, { dpr: 1.5 });
  return c.toDataURL("image/jpeg", 0.72);
}

/** Text einer Seite (für Suche, Vorlesen, Textexport) */
export async function pageText(doc: PDFDocumentProxy, pageNo: number): Promise<string> {
  const page = await doc.getPage(pageNo);
  const tc = await page.getTextContent();
  let out = "";
  for (const it of tc.items as any[]) {
    if (!("str" in it)) continue;
    out += it.str + (it.hasEOL ? "\n" : "");
  }
  return out;
}
