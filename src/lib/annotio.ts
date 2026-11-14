// Kommentare ⇄ echte PDF-Annotationen (sichtbar in Acrobat, Preview, Chrome …).
// Jede von uns geschriebene Annotation trägt /LPData (JSON), damit LightPDF sie beim
// erneuten Öffnen wieder als bearbeitbares Objekt einlesen kann.
import {
  PDFDocument,
  PDFName,
  PDFArray,
  PDFDict,
  PDFHexString,
  PDFString,
  PDFRef,
  PDFNumber,
  StandardFonts,
  pushGraphicsState,
  popGraphicsState,
  setGraphicsState,
  setFillingRgbColor,
  setStrokingRgbColor,
  setLineWidth,
  setLineCap,
  setLineJoin,
  LineCapStyle,
  LineJoinStyle,
  moveTo,
  lineTo,
  closePath,
  fill,
  stroke,
  rectangle,
  drawObject,
  concatTransformationMatrix,
  beginText,
  endText,
  setFontAndSize,
  showText,
  moveText,
  drawEllipse,
  rgb,
  type PDFFont,
  type PDFOperator,
  type PDFPage,
} from "@cantoo/pdf-lib";
import type { Annotation, Pt, Rect } from "../types";

const LP = PDFName.of("LPData");

export function hexToRgb(hex: string): [number, number, number] {
  const h = hex.replace("#", "");
  const n = parseInt(h.length === 3 ? h.split("").map((c) => c + c).join("") : h.slice(0, 6), 16);
  return [((n >> 16) & 255) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255];
}

const pdfDate = (t: number) => {
  const d = new Date(t);
  const p = (n: number) => String(n).padStart(2, "0");
  return `D:${d.getUTCFullYear()}${p(d.getUTCMonth() + 1)}${p(d.getUTCDate())}${p(d.getUTCHours())}${p(d.getUTCMinutes())}${p(d.getUTCSeconds())}Z`;
};

export function annBounds(a: Annotation): Rect {
  switch (a.type) {
    case "highlight":
    case "underline":
    case "strike": {
      const xs = a.rects.flatMap((r) => [r[0], r[2]]);
      const ys = a.rects.flatMap((r) => [r[1], r[3]]);
      return [Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys)];
    }
    case "ink": {
      const pts = a.paths.flat();
      const w = a.width / 2 + 1;
      return [
        Math.min(...pts.map((p) => p[0])) - w,
        Math.min(...pts.map((p) => p[1])) - w,
        Math.max(...pts.map((p) => p[0])) + w,
        Math.max(...pts.map((p) => p[1])) + w,
      ];
    }
    case "line":
    case "arrow": {
      const w = a.width * 4 + 2;
      return [
        Math.min(a.from[0], a.to[0]) - w,
        Math.min(a.from[1], a.to[1]) - w,
        Math.max(a.from[0], a.to[0]) + w,
        Math.max(a.from[1], a.to[1]) + w,
      ];
    }
    case "note":
      return [a.at[0], a.at[1] - 20, a.at[0] + 20, a.at[1]];
    case "rect":
    case "ellipse": {
      const w = a.width / 2;
      return [a.rect[0] - w, a.rect[1] - w, a.rect[2] + w, a.rect[3] + w];
    }
    default:
      return a.rect;
  }
}

function dataUrlBytes(src: string): { bytes: Uint8Array; png: boolean } {
  const [head, b64] = src.split(",");
  const bin = atob(b64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return { bytes, png: head.includes("png") };
}

/** Text so umbrechen, dass er in die Breite passt */
export function wrapText(text: string, font: { widthOfTextAtSize(t: string, s: number): number }, size: number, width: number) {
  const lines: string[] = [];
  for (const para of text.split("\n")) {
    let line = "";
    for (const word of para.split(/(\s+)/)) {
      const test = line + word;
      if (line && font.widthOfTextAtSize(test.trimEnd(), size) > width) {
        lines.push(line.trimEnd());
        line = word.trimStart();
      } else line = test;
    }
    lines.push(line.trimEnd());
  }
  return lines;
}

/** Zeichen entfernen, die die Standardschrift (WinAnsi) nicht darstellen kann */
function safeText(font: PDFFont, s: string) {
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

type Ctx = {
  doc: PDFDocument;
  font: PDFFont;
  fontBold: PDFFont;
  gs: Map<string, PDFRef>;
};

function gstate(ctx: Ctx, opacity: number, multiply = false) {
  const key = opacity.toFixed(2) + (multiply ? "m" : "");
  let ref = ctx.gs.get(key);
  if (!ref) {
    ref = ctx.doc.context.register(
      ctx.doc.context.obj({ Type: "ExtGState", CA: opacity, ca: opacity, ...(multiply ? { BM: "Multiply" } : {}) }),
    );
    ctx.gs.set(key, ref);
  }
  return ref;
}

function appearance(ctx: Ctx, bbox: Rect, ops: PDFOperator[], resources: Record<string, unknown>) {
  const stream = ctx.doc.context.formXObject(ops, {
    BBox: bbox,
    Resources: resources,
  } as any);
  return ctx.doc.context.register(stream);
}

function pathOps(points: Pt[]) {
  const ops: PDFOperator[] = [];
  points.forEach((p, i) => ops.push(i === 0 ? moveTo(p[0], p[1]) : lineTo(p[0], p[1])));
  if (points.length === 1) ops.push(lineTo(points[0][0] + 0.01, points[0][1]));
  return ops;
}

function buildAnnot(ctx: Ctx, page: PDFPage, a: Annotation): PDFDict | null {
  const c = ctx.doc.context;
  const [r, g, b] = hexToRgb(a.color);
  const bbox = annBounds(a);
  const base: Record<string, unknown> = {
    Type: "Annot",
    Rect: bbox,
    C: [r, g, b],
    CA: a.opacity,
    F: 4, // drucken
    NM: PDFString.of("lp-" + a.id),
    M: PDFString.of(pdfDate(a.createdAt)),
    P: page.ref,
  };
  if (a.author) base.T = PDFHexString.fromText(a.author);
  const contents = a.type === "note" || a.type === "text" ? a.text : a.note ?? ("text" in a ? a.text : undefined);
  if (contents) base.Contents = PDFHexString.fromText(contents);

  let dict: Record<string, unknown>;
  let ap: PDFRef | null = null;
  const gs = gstate(ctx, a.opacity, a.type === "highlight" || (a.type === "ink" && !!a.highlighter));
  const res = { ExtGState: { GS0: gs } };

  switch (a.type) {
    case "highlight":
    case "underline":
    case "strike": {
      const quads: number[] = [];
      const ops: PDFOperator[] = [pushGraphicsState(), setGraphicsState("GS0")];
      for (const q of a.rects) {
        const [x1, y1, x2, y2] = q;
        quads.push(x1, y2, x2, y2, x1, y1, x2, y1);
        const h = y2 - y1;
        if (a.type === "highlight") ops.push(setFillingRgbColor(r, g, b), rectangle(x1, y1, x2 - x1, h), fill());
        else {
          const y = a.type === "underline" ? y1 + h * 0.08 : y1 + h * 0.45;
          ops.push(setStrokingRgbColor(r, g, b), setLineWidth(Math.max(0.6, h * 0.07)), moveTo(x1, y), lineTo(x2, y), stroke());
        }
      }
      ops.push(popGraphicsState());
      ap = appearance(ctx, bbox, ops, res);
      dict = { ...base, Subtype: a.type === "highlight" ? "Highlight" : a.type === "underline" ? "Underline" : "StrikeOut", QuadPoints: quads };
      break;
    }
    case "ink": {
      const ops: PDFOperator[] = [
        pushGraphicsState(),
        setGraphicsState("GS0"),
        setStrokingRgbColor(r, g, b),
        setLineWidth(a.width),
        setLineCap(LineCapStyle.Round),
        setLineJoin(LineJoinStyle.Round),
      ];
      for (const p of a.paths) ops.push(...pathOps(p), stroke());
      ops.push(popGraphicsState());
      ap = appearance(ctx, bbox, ops, res);
      dict = { ...base, Subtype: "Ink", InkList: a.paths.map((p) => p.flat()), BS: { W: a.width, S: "S" } };
      break;
    }
    case "rect":
    case "ellipse": {
      const [x1, y1, x2, y2] = a.rect;
      const ops: PDFOperator[] = [pushGraphicsState(), setGraphicsState("GS0")];
      const fillRgb = a.fill ? hexToRgb(a.fill) : null;
      if (a.type === "rect") {
        if (fillRgb) ops.push(setFillingRgbColor(...fillRgb), rectangle(x1, y1, x2 - x1, y2 - y1), fill());
        ops.push(setStrokingRgbColor(r, g, b), setLineWidth(a.width), rectangle(x1, y1, x2 - x1, y2 - y1), stroke());
      } else {
        ops.push(
          ...drawEllipse({
            x: (x1 + x2) / 2,
            y: (y1 + y2) / 2,
            xScale: (x2 - x1) / 2,
            yScale: (y2 - y1) / 2,
            borderWidth: a.width,
            borderColor: rgb(r, g, b),
            color: fillRgb ? rgb(...fillRgb) : undefined,
          } as any),
        );
      }
      ops.push(popGraphicsState());
      ap = appearance(ctx, bbox, ops, res);
      dict = {
        ...base,
        Subtype: a.type === "rect" ? "Square" : "Circle",
        BS: { W: a.width, S: "S" },
        ...(fillRgb ? { IC: fillRgb } : {}),
      };
      break;
    }
    case "line":
    case "arrow": {
      const ops: PDFOperator[] = [
        pushGraphicsState(),
        setGraphicsState("GS0"),
        setStrokingRgbColor(r, g, b),
        setLineWidth(a.width),
        setLineCap(LineCapStyle.Round),
        setLineJoin(LineJoinStyle.Round),
        moveTo(...a.from),
        lineTo(...a.to),
        stroke(),
      ];
      if (a.type === "arrow") {
        const [h1, h2] = arrowHead(a.from, a.to, a.width);
        ops.push(moveTo(...h1), lineTo(...a.to), lineTo(...h2), stroke());
      }
      ops.push(popGraphicsState());
      ap = appearance(ctx, bbox, ops, res);
      dict = {
        ...base,
        Subtype: "Line",
        L: [...a.from, ...a.to],
        BS: { W: a.width, S: "S" },
        LE: [PDFName.of("None"), PDFName.of(a.type === "arrow" ? "OpenArrow" : "None")],
      };
      break;
    }
    case "text": {
      const font = a.bold ? ctx.fontBold : ctx.font;
      const size = a.fontSize;
      const rot = upright(page, a.rect);
      const lines = wrapText(safeText(font, a.text), font, size, Math.max(10, rot.vw - 4));
      const lh = size * 1.22;
      const ops: PDFOperator[] = [pushGraphicsState(), setGraphicsState("GS0"), rot.cm, beginText(), setFillingRgbColor(r, g, b), setFontAndSize("F1", size)];
      ops.push(moveText(2, rot.vh - 2 - size * 0.9));
      lines.forEach((ln, i) => {
        if (i > 0) ops.push(moveText(0, -lh));
        ops.push(showText(font.encodeText(ln)));
      });
      ops.push(endText(), popGraphicsState());
      ap = appearance(ctx, a.rect, ops, { ...res, Font: { F1: font.ref } });
      const da = `/Helv ${size} Tf ${r.toFixed(3)} ${g.toFixed(3)} ${b.toFixed(3)} rg`;
      dict = { ...base, Subtype: "FreeText", DA: PDFString.of(da), Q: 0, BS: { W: 0 }, ...(rot.R ? { Rotate: rot.R } : {}) };
      break;
    }
    case "note": {
      const [x, y] = a.at;
      const ops: PDFOperator[] = [
        pushGraphicsState(),
        setFillingRgbColor(r, g, b),
        setStrokingRgbColor(r * 0.6, g * 0.6, b * 0.6),
        setLineWidth(0.8),
        moveTo(x + 1, y - 1),
        lineTo(x + 19, y - 1),
        lineTo(x + 19, y - 15),
        lineTo(x + 9, y - 15),
        lineTo(x + 5, y - 19),
        lineTo(x + 5, y - 15),
        lineTo(x + 1, y - 15),
        closePath(),
        fill(),
        moveTo(x + 1, y - 1),
        lineTo(x + 19, y - 1),
        lineTo(x + 19, y - 15),
        lineTo(x + 9, y - 15),
        lineTo(x + 5, y - 19),
        lineTo(x + 5, y - 15),
        lineTo(x + 1, y - 15),
        closePath(),
        stroke(),
        popGraphicsState(),
      ];
      ap = appearance(ctx, bbox, ops, {});
      dict = { ...base, Subtype: "Text", Name: "Comment", Open: false };
      break;
    }
    case "image": {
      // Bild wurde vorher eingebettet → siehe writeAnnotations()
      const img = (ctx as any).images.get(a.id) as PDFRef;
      if (!img) return null;
      const rot = upright(page, a.rect);
      const ops: PDFOperator[] = [
        pushGraphicsState(),
        setGraphicsState("GS0"),
        rot.cm,
        concatTransformationMatrix(rot.vw, 0, 0, rot.vh, 0, 0),
        drawObject("Im0"),
        popGraphicsState(),
      ];
      ap = appearance(ctx, a.rect, ops, { ...res, XObject: { Im0: img } });
      dict = { ...base, Subtype: "Stamp", Name: a.kind === "signature" ? "Signed" : "Draft" };
      break;
    }
    case "redact": {
      const [x1, y1, x2, y2] = a.rect;
      const ops: PDFOperator[] = [
        pushGraphicsState(),
        setStrokingRgbColor(0.85, 0.1, 0.1),
        setLineWidth(1),
        rectangle(x1, y1, x2 - x1, y2 - y1),
        stroke(),
        popGraphicsState(),
      ];
      ap = appearance(ctx, a.rect, ops, {});
      dict = { ...base, Subtype: "Redact", QuadPoints: [x1, y2, x2, y2, x1, y1, x2, y1], IC: [0, 0, 0] };
      break;
    }
  }
  const d = c.obj(dict as any) as unknown as PDFDict;
  if (ap) d.set(PDFName.of("AP"), c.obj({ N: ap }));
  d.set(LP, PDFHexString.fromText(JSON.stringify(a)));
  return d;
}

/**
 * Für Text und Bilder: Inhalt so ausrichten, dass er auf gedrehten Seiten aufrecht erscheint.
 * Liefert eine Transformation von sichtbaren lokalen Koordinaten (0..vw, 0..vh) in den Seitenraum.
 */
function upright(page: PDFPage, r: Rect) {
  const R = ((page.getRotation().angle % 360) + 360) % 360;
  const [x1, y1, x2, y2] = r;
  const w = x2 - x1,
    h = y2 - y1;
  const m: Record<number, number[]> = {
    0: [1, 0, 0, 1, x1, y1],
    90: [0, 1, -1, 0, x1 + w, y1],
    180: [-1, 0, 0, -1, x1 + w, y1 + h],
    270: [0, -1, 1, 0, x1, y1 + h],
  };
  const [ma, mb, mc, md, me, mf] = m[R] ?? m[0];
  return { R, vw: R % 180 ? h : w, vh: R % 180 ? w : h, cm: concatTransformationMatrix(ma, mb, mc, md, me, mf) };
}

export function arrowHead(from: Pt, to: Pt, width: number): [Pt, Pt] {
  const ang = Math.atan2(to[1] - from[1], to[0] - from[0]);
  const len = Math.max(8, width * 4);
  const s = 0.45;
  return [
    [to[0] - len * Math.cos(ang - s), to[1] - len * Math.sin(ang - s)],
    [to[0] - len * Math.cos(ang + s), to[1] - len * Math.sin(ang + s)],
  ];
}

/** Schreibt die Kommentare als echte Annotationen ins PDF */
export async function writeAnnotations(doc: PDFDocument, annots: Annotation[]) {
  if (!annots.length) return;
  const ctx: Ctx & { images: Map<string, PDFRef> } = {
    doc,
    font: await doc.embedFont(StandardFonts.Helvetica),
    fontBold: await doc.embedFont(StandardFonts.HelveticaBold),
    gs: new Map(),
    images: new Map(),
  };
  for (const a of annots) {
    if (a.type !== "image") continue;
    const { bytes, png } = dataUrlBytes(a.src);
    const img = png ? await doc.embedPng(bytes) : await doc.embedJpg(bytes);
    ctx.images.set(a.id, img.ref);
  }
  const pages = doc.getPages();
  for (const a of annots) {
    const page = pages[a.page - 1];
    if (!page) continue;
    const dict = buildAnnot(ctx, page, a);
    if (!dict) continue;
    const ref = doc.context.register(dict);
    let arr = page.node.lookupMaybe(PDFName.of("Annots"), PDFArray);
    if (!arr) {
      arr = doc.context.obj([]) as PDFArray;
      page.node.set(PDFName.of("Annots"), arr);
    }
    arr.push(ref);
  }
}

/** Liest eigene Annotationen aus dem PDF und entfernt sie dort (sie werden wieder bearbeitbar). */
export function extractOwnAnnotations(doc: PDFDocument): Annotation[] {
  const out: Annotation[] = [];
  doc.getPages().forEach((page, idx) => {
    const arr = page.node.lookupMaybe(PDFName.of("Annots"), PDFArray);
    if (!arr) return;
    const pageAnnots: Annotation[] = [];
    for (let i = arr.size() - 1; i >= 0; i--) {
      const d = arr.lookupMaybe(i, PDFDict);
      if (!d) continue;
      const lp = d.get(LP);
      if (!(lp instanceof PDFHexString || lp instanceof PDFString)) continue;
      try {
        const a = JSON.parse(lp.decodeText()) as Annotation;
        a.page = idx + 1;
        // Wurde der Kommentar in einem anderen Programm bearbeitet? Inhalt übernehmen
        const contents = d.get(PDFName.of("Contents"));
        if (contents instanceof PDFHexString || contents instanceof PDFString) {
          const t = contents.decodeText();
          const written = a.type === "note" || a.type === "text" ? a.text : (a.note ?? ("text" in a ? a.text : undefined));
          if (t !== (written ?? "")) {
            if (a.type === "note" || a.type === "text") a.text = t;
            else a.note = t || undefined;
          }
        }
        pageAnnots.unshift(a);
        const ref = arr.get(i);
        arr.remove(i);
        if (ref instanceof PDFRef) doc.context.delete(ref);
      } catch {
        /* fremde Daten ignorieren */
      }
    }
    out.push(...pageAnnots);
    if (arr.size() === 0) page.node.delete(PDFName.of("Annots"));
  });
  return out;
}

export { PDFNumber };
