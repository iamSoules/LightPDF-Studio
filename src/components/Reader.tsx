import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowLeft, Undo2, Redo2, Search, PanelLeft, Save, MoreVertical, Share2, Printer, Download, ZoomIn, ZoomOut, Hand, MousePointer2,
  Highlighter, Underline, Strikethrough, StickyNote, Type, PenLine, Brush, Square, Circle, Minus, MoveUpRight, Stamp, ImagePlus, Eraser,
  Check, X as XIcon, Dot, CalendarDays, Signature, CaseSensitive, LayoutGrid, RotateCw, RotateCcw, Trash2, FilePlus2, FileOutput,
  Combine, Scissors, Minimize2, Lock, Droplets, Hash, EyeOff, Images, FileText, Info, Moon, Sun, Coffee, Rows3, Columns3, LayoutPanelLeft,
  BookOpen, Maximize, Minimize, Volume2, VolumeX, MoveHorizontal, ScanLine, ChevronLeft, ChevronRight, Eye, Copy, FormInput, Layers,
} from "lucide-react";
import Viewer, { type ViewerHandle } from "./Viewer";
import AnnotationOverlay, { viewRectToPdf, type ToolOpts } from "./AnnotationOverlay";
import SidePanel, { type PanelKind } from "./SidePanel";
import Organizer, { type PageOp } from "./Organizer";
import { SignatureDialog, StampDialog, aspectOf, type PlacedImage } from "./SignatureDialog";
import { ToolDialog, type ToolKind, type ToolResult, fmtSize } from "./ToolDialogs";
import { Btn, IconBtn, Menu, Modal, Range, Swatches, cx, useConfirm, useMenu, useToast, type MenuItem } from "./ui";
import { db } from "../lib/db";
import * as ops from "../lib/pdfops";
import { openPdf, pageText, renderPageToCanvas, thumbnailDataUrl, type PDFDocumentProxy } from "../lib/pdfjs";
import { canPrint, isAndroid, isDesktop, isMac, pickFiles, printImages, saveAs, share, writeFile } from "../lib/platform";
import type { Annotation, DocMeta, MarkupType, Rect, Settings, Tool } from "../types";
import { useT } from "../i18n";
import { hashBytes, importFiles } from "../lib/library";

type Tab = "view" | "comment" | "fill" | "pages" | "tools";

const MARKUP: Tool[] = ["highlight", "underline", "strike"];
const DEFAULT_COLORS: Partial<Record<Tool, string>> = {
  highlight: "#f6c945",
  underline: "#2f6bd6",
  strike: "#d64545",
  ink: "#d64545",
  highlighter: "#f6c945",
  rect: "#d64545",
  ellipse: "#2f6bd6",
  line: "#1f1f1f",
  arrow: "#d64545",
  text: "#1f1f1f",
  note: "#f6c945",
};

// ------------------------------------------------------------------ Verlauf für Rückgängig/Wiederholen
function useHistory(initial: Annotation[]) {
  const [state, setState] = useState({ past: [] as Annotation[][], present: initial, future: [] as Annotation[][] });
  const set = useCallback((next: Annotation[] | ((a: Annotation[]) => Annotation[]), commit = true) => {
    setState((s) => {
      const value = typeof next === "function" ? next(s.present) : next;
      if (!commit) return { ...s, present: value };
      return { past: [...s.past.slice(-80), s.present], present: value, future: [] };
    });
  }, []);
  // Live-Änderungen (z. B. beim Ziehen) ohne Verlaufseintrag, Abschluss mit Eintrag des Zustands vor dem Ziehen
  const live = useRef<Annotation[] | null>(null);
  const change = useCallback((fn: (a: Annotation[]) => Annotation[], commit: boolean) => {
    setState((s) => {
      if (!commit) {
        if (!live.current) live.current = s.present;
        return { ...s, present: fn(s.present) };
      }
      const before = live.current ?? s.present;
      live.current = null;
      return { past: [...s.past.slice(-80), before], present: fn(s.present), future: [] };
    });
  }, []);
  const undo = useCallback(() => setState((s) => (s.past.length ? { past: s.past.slice(0, -1), present: s.past[s.past.length - 1], future: [s.present, ...s.future] } : s)), []);
  const redo = useCallback(() => setState((s) => (s.future.length ? { past: [...s.past, s.present], present: s.future[0], future: s.future.slice(1) } : s)), []);
  const reset = useCallback((a: Annotation[]) => setState({ past: [], present: a, future: [] }), []);
  return { annots: state.present, set, change, undo, redo, reset, canUndo: state.past.length > 0, canRedo: state.future.length > 0 };
}

/** Rechtecke einer Textauswahl auf Seiten verteilen und zeilenweise zusammenfassen */
function selectionRects(viewer: ViewerHandle): { page: number; rects: Rect[]; text: string }[] {
  const sel = getSelection();
  if (!sel || sel.isCollapsed || !sel.rangeCount) return [];
  const container = viewer.container();
  if (!container || !container.contains(sel.anchorNode)) return [];
  const byPage = new Map<number, Rect[]>();
  for (let i = 0; i < sel.rangeCount; i++) {
    for (const r of Array.from(sel.getRangeAt(i).getClientRects())) {
      if (r.width < 1 || r.height < 1) continue;
      const hit = viewer.pageAtClient(r.left + r.width / 2, r.top + r.height / 2);
      if (!hit) continue;
      const { geom: g, rect: pr } = hit;
      const sx = g.width / pr.width,
        sy = g.height / pr.height;
      const v: [number, number, number, number] = [(r.left - pr.left) * sx, (r.top - pr.top) * sy, (r.right - pr.left) * sx, (r.bottom - pr.top) * sy];
      const list = byPage.get(hit.page) ?? [];
      list.push(viewRectToPdf(g, v));
      byPage.set(hit.page, list);
    }
  }
  const text = sel.toString().replace(/\s+/g, " ").trim();
  return [...byPage.entries()].map(([page, rects]) => {
    // überlappende Rechtecke derselben Zeile zusammenfassen
    rects.sort((a, b) => b[3] - a[3] || a[0] - b[0]);
    const merged: Rect[] = [];
    for (const r of rects) {
      const m = merged.find((x) => {
        const ov = Math.min(x[3], r[3]) - Math.max(x[1], r[1]);
        const hMin = Math.min(x[3] - x[1], r[3] - r[1]);
        const vertical = Math.min(x[2], r[2]) - Math.max(x[0], r[0]);
        const wMin = Math.min(x[2] - x[0], r[2] - r[0]);
        return (ov > hMin * 0.5 && r[0] <= x[2] + 3 && r[2] >= x[0] - 3) || (vertical > wMin * 0.5 && r[1] <= x[3] + 3 && r[3] >= x[1] - 3 && ov > hMin * 0.5);
      });
      if (m) {
        m[0] = Math.min(m[0], r[0]);
        m[1] = Math.min(m[1], r[1]);
        m[2] = Math.max(m[2], r[2]);
        m[3] = Math.max(m[3], r[3]);
      } else merged.push([...r] as Rect);
    }
    return { page, rects: merged, text };
  });
}

/** Seitennummern der Kommentare nach Seitenoperationen anpassen */
function remap(annots: Annotation[], fn: (page: number) => number | null): Annotation[] {
  const out: Annotation[] = [];
  for (const a of annots) {
    const p = fn(a.page);
    if (p) out.push({ ...a, page: p });
  }
  return out;
}

export default function Reader({
  meta: initialMeta,
  settings,
  setSettings,
  onClose,
  onOpenDoc,
}: {
  meta: DocMeta;
  settings: Settings;
  setSettings: (s: Partial<Settings>) => void;
  onClose: () => void;
  onOpenDoc: (id: string) => void;
}) {
  const t = useT();
  const { toast, busy } = useToast();
  const { confirm, node: confirmNode } = useConfirm();
  const viewer = useRef<ViewerHandle>(null);
  const rootRef = useRef<HTMLDivElement>(null);
  const [meta, setMeta] = useState(initialMeta);
  const metaRef = useRef(meta);
  metaRef.current = meta;
  const [base, setBase] = useState<Uint8Array | null>(null);
  const baseRef = useRef<Uint8Array | null>(null);
  baseRef.current = base;
  const [password, setPassword] = useState<string | undefined>();
  const [pwAsk, setPwAsk] = useState<{ wrong: boolean } | null>(null);
  const [doc, setDoc] = useState<PDFDocumentProxy | null>(null);
  const [page, setPage] = useState(meta.lastPage || 1);
  const [total, setTotal] = useState(meta.pages || 0);
  const [scale, setScale] = useState(1);
  const [version, setVersion] = useState(0);
  const H = useHistory([]);
  const [loaded, setLoaded] = useState(false);
  const mobile = useMedia("(max-width: 760px)");

  const [tab, setTab] = useState<Tab>("view");
  const [tool, setToolRaw] = useState<Tool>("select");
  const [colors, setColors] = useState<Partial<Record<Tool, string>>>(DEFAULT_COLORS);
  const [width, setWidth] = useState(2);
  const [opacity, setOpacity] = useState(1);
  const [fontSize, setFontSize] = useState(12);
  const [fill, setFill] = useState<string | null>(null);
  const [preset, setPreset] = useState<string | undefined>();
  const [pending, setPending] = useState<PlacedImage | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [panel, setPanel] = useState<PanelKind | null>(() => (innerWidth > 1100 ? "thumbs" : null));
  const [findResult, setFindResult] = useState({ current: 0, total: 0, state: -1 });
  const [selPop, setSelPop] = useState<{ x: number; y: number; data: ReturnType<typeof selectionRects> } | null>(null);
  const [dialog, setDialog] = useState<null | "signature" | "initials" | "stamp" | "goto" | "unsaved">(null);
  const [toolDialog, setToolDialog] = useState<ToolKind | null>(null);
  const [pdfMeta, setPdfMeta] = useState<ops.Meta | null>(null);
  const [organize, setOrganize] = useState(false);
  const [pageUndo, setPageUndo] = useState<{ base: Uint8Array; annots: Annotation[] }[]>([]);
  const [dirty, setDirty] = useState(false);
  const [immersive, setImmersive] = useState(false);
  const [speaking, setSpeaking] = useState(false);
  const [showForms, setShowForms] = useState(true);
  const moreMenu = useMenu();
  const saveMenu = useMenu();

  const color = colors[tool] ?? "#d64545";
  const setTool = (tl: Tool) => {
    setToolRaw(tl);
    setSelectedId(null);
    setPreset(undefined);
    if (!["signature", "image", "stamp"].includes(tl)) setPending(null);
    if (tl === "highlighter") setOpacity(0.45);
    else if (opacity < 0.5) setOpacity(1);
  };

  // ------------------------------------------------------------------ Laden
  useEffect(() => {
    (async () => {
      const [file, annots] = await Promise.all([db.getFile(meta.id), db.getAnnots(meta.id)]);
      if (!file) {
        toast(t("Datei nicht gefunden"), "err");
        onClose();
        return;
      }
      setBase(new Uint8Array(file));
      H.reset(annots);
      setLoaded(true);
    })();
  }, []);

  // Kommentare automatisch sichern
  useEffect(() => {
    if (!loaded) return;
    const id = setTimeout(() => db.putAnnots(meta.id, H.annots), 350);
    return () => clearTimeout(id);
  }, [H.annots, loaded]);
  const firstAnnots = useRef(true);
  useEffect(() => {
    if (!loaded) return;
    if (firstAnnots.current) {
      firstAnnots.current = false;
      return;
    }
    setDirty(true);
  }, [H.annots]);

  // Lesezeichen: zuletzt gelesene Seite
  useEffect(() => {
    if (!loaded || !total) return;
    const id = setTimeout(() => {
      const m = { ...metaRef.current, lastPage: page, openedAt: Date.now(), pages: total };
      setMeta(m);
      db.putDoc(m);
    }, 500);
    return () => clearTimeout(id);
  }, [page, total, loaded]);

  const persistBase = async (b: Uint8Array, annots: Annotation[], js?: PDFDocumentProxy) => {
    await db.putFile(meta.id, b);
    await db.putAnnots(meta.id, annots);
    const m: DocMeta = { ...metaRef.current, size: b.length };
    try {
      const d = js ?? (await openPdf(b, password));
      m.pages = d.numPages;
      m.thumb = await thumbnailDataUrl(d);
      if (!js) d.destroy();
    } catch {
      /* Vorschau optional */
    }
    setMeta(m);
    await db.putDoc(m);
  };

  /** Aktuelle Basis inkl. Formularwerten */
  const currentBase = async () => {
    const fb = await viewer.current?.formBytes();
    if (fb) {
      // Formularwerte übernehmen, ohne den Viewer neu zu laden (Scrollposition bleibt)
      baseRef.current = fb;
      await db.putFile(meta.id, fb);
      return fb;
    }
    return baseRef.current!;
  };

  const buildPdf = async (o: { flatten?: boolean; noAnnots?: boolean } = {}) => {
    const b = await currentBase();
    return ops.exportPdf(b, o.noAnnots ? [] : H.annots, { password, flatten: o.flatten });
  };

  // ------------------------------------------------------------------ Speichern & Exportieren
  const fileName = (suffix = "", ext = "pdf") => meta.name.replace(/\.pdf$/i, "") + suffix + "." + ext;

  const save = async (as = false) => {
    try {
      busy(t("Speichere …"));
      const bytes = await buildPdf();
      busy(null);
      if (!as && isDesktop && meta.path) {
        await writeFile(meta.path, bytes);
        const m = { ...metaRef.current, diskHash: hashBytes(bytes) } as DocMeta;
        setMeta(m);
        db.putDoc(m);
        setDirty(false);
        toast(t("Gespeichert"));
        return true;
      }
      const res = await saveAs(fileName(), bytes);
      if (!res) return false;
      if (res.path) {
        const name = res.path.split(/[\\/]/).pop()!;
        const m = { ...metaRef.current, path: res.path, name, diskHash: hashBytes(bytes) } as DocMeta;
        setMeta(m);
        db.putDoc(m);
      }
      setDirty(false);
      toast(isAndroid ? t("Gespeichert in {0}", res.label) : isDesktop ? t("Gespeichert") : t("Heruntergeladen"));
      return true;
    } catch (e) {
      busy(null);
      toast(t("Speichern fehlgeschlagen: {0}", (e as Error).message), "err");
      return false;
    }
  };

  const doShare = async () => {
    busy(t("Bereite vor …"));
    try {
      const bytes = await buildPdf();
      busy(null);
      await share(fileName(), bytes);
    } catch (e) {
      busy(null);
      if ((e as Error).name !== "AbortError") toast((e as Error).message, "err");
    }
  };

  const doPrint = async () => {
    try {
      busy(t("Bereite Druck vor …"), 0);
      const bytes = await buildPdf();
      const js = await openPdf(bytes, password);
      const urls: string[] = [];
      for (let i = 1; i <= js.numPages; i++) {
        const p = await js.getPage(i);
        const w = p.getViewport({ scale: 1, rotation: p.rotate }).width;
        const c = await renderPageToCanvas(js, i, (w * 150) / 72, { dpr: 1 });
        urls.push(c.toDataURL("image/jpeg", 0.92));
        busy(t("Bereite Druck vor …"), i / js.numPages);
      }
      js.destroy();
      busy(null);
      await printImages(urls);
    } catch (e) {
      busy(null);
      toast((e as Error).message, "err");
    }
  };

  const exportAs = async (kind: "flat" | "clean" | "text") => {
    try {
      busy(t("Exportiere …"));
      if (kind === "text") {
        const txt = await ops.exportText(doc!);
        busy(null);
        await saveAs(fileName("", "txt"), new TextEncoder().encode(txt));
      } else {
        const bytes = await buildPdf(kind === "flat" ? { flatten: true } : { noAnnots: true });
        busy(null);
        await saveAs(fileName(kind === "flat" ? " (fixiert)" : " (ohne Kommentare)"), bytes);
      }
    } catch (e) {
      busy(null);
      toast((e as Error).message, "err");
    }
  };

  const back = async () => {
    window.speechSynthesis?.cancel();
    await currentBase().catch(() => {});
    if (dirty && isDesktop && meta.path) {
      setDialog("unsaved");
      return;
    }
    onClose();
  };
  const backRef = useRef(back);
  backRef.current = back;
  useEffect(() => {
    (window as any).handleAndroidHardwareBack = () => {
      if (organize) return setOrganize(false), true;
      if (dialog || toolDialog) return true;
      if (panel && mobile) return setPanel(null), true;
      backRef.current();
      return true;
    };
    return () => {
      delete (window as any).handleAndroidHardwareBack;
    };
  }, [organize, dialog, toolDialog, panel, mobile]);

  // ------------------------------------------------------------------ Seiten- und Dokumentoperationen
  const applyBase = async (nb: Uint8Array, na: Annotation[], msg?: string) => {
    setPageUndo((u) => [...u.slice(-10), { base: baseRef.current!, annots: H.annots }]);
    const keepPage = viewer.current?.currentPage() ?? page;
    setBase(nb);
    baseRef.current = nb;
    H.reset(na);
    setVersion((v) => v + 1);
    setDirty(true);
    setPage(Math.min(keepPage, 99999));
    await persistBase(nb, na);
    if (msg) toast(msg);
  };

  const run = async (label: string, fn: () => Promise<void>) => {
    try {
      busy(label);
      await fn();
    } catch (e) {
      toast(t("Fehler: {0}", (e as Error).message), "err");
      console.error(e);
    } finally {
      busy(null);
    }
  };

  const pageOp = (o: PageOp) =>
    run(t("Bearbeite Seiten …"), async () => {
      const b = await currentBase();
      const a = H.annots;
      switch (o.op) {
        case "rotate":
          return applyBase(await ops.rotatePages(b, o.pages, o.delta, password), a);
        case "delete": {
          const del = new Set(o.pages.map((i) => i + 1));
          const nb = await ops.deletePages(b, o.pages, password);
          return applyBase(nb, remap(a, (p) => (del.has(p) ? null : p - [...del].filter((d) => d < p).length)), t("{0} Seite(n) gelöscht", o.pages.length));
        }
        case "order": {
          const nb = await ops.reorderPages(b, o.order, password);
          return applyBase(nb, remap(a, (p) => { const i = o.order.indexOf(p - 1); return i < 0 ? null : i + 1; }));
        }
        case "duplicate": {
          const order: number[] = [];
          for (let i = 0; i < total; i++) {
            order.push(i);
            if (o.pages.includes(i)) order.push(i);
          }
          const nb = await ops.reorderPages(b, order, password);
          return applyBase(nb, remap(a, (p) => order.indexOf(p - 1) + 1), t("Dupliziert"));
        }
        case "blank": {
          const nb = await ops.insertBlank(b, o.at, password);
          return applyBase(nb, remap(a, (p) => (p > o.at ? p + 1 : p)), t("Leere Seite eingefügt"));
        }
        case "file": {
          const files = await pickFiles(".pdf,application/pdf", true);
          if (!files.length) return;
          let nb = b;
          let at = o.at;
          let added = 0;
          for (const f of files) {
            const n = (await ops.load(f.data)).getPageCount();
            nb = await ops.insertPdf(nb, f.data, at, password);
            at += n;
            added += n;
          }
          return applyBase(nb, remap(a, (p) => (p > o.at ? p + added : p)), t("{0} Seite(n) eingefügt", added));
        }
        case "images": {
          const files = await pickFiles("image/*", true);
          if (!files.length) return;
          const pdf = await ops.imagesToPdf(files.map((f) => new Blob([f.data], { type: mimeOf(f.name) })), "a4");
          const nb = await ops.insertPdf(b, pdf, o.at, password);
          return applyBase(nb, remap(a, (p) => (p > o.at ? p + files.length : p)), t("{0} Bild(er) eingefügt", files.length));
        }
        case "extract": {
          const nb = await ops.extractPages(b, o.pages, password);
          const [m] = await importFiles([{ name: fileName(` (Seiten ${compactRanges(o.pages)})`), data: nb }]);
          toast(t("Als neues Dokument gespeichert"), "ok", m ? { label: t("Öffnen"), run: () => onOpenDoc(m.id) } : undefined);
          return;
        }
      }
    });

  const undoPageOp = async () => {
    const last = pageUndo[pageUndo.length - 1];
    if (!last) return;
    setPageUndo((u) => u.slice(0, -1));
    setBase(last.base);
    baseRef.current = last.base;
    H.reset(last.annots);
    setVersion((v) => v + 1);
    await persistBase(last.base, last.annots);
  };

  const runTool = (r: ToolResult) =>
    run(t("Bitte warten …"), async () => {
      const b = await currentBase();
      const a = H.annots;
      if (r.watermark) return applyBase(await ops.watermark(b, r.watermark, password), a, t("Wasserzeichen hinzugefügt"));
      if (r.numbers) return applyBase(await ops.stampText(b, { ...r.numbers, name: meta.name }, password), a, t("Hinzugefügt"));
      if (r.meta) {
        await applyBase(await ops.setMeta(b, r.meta, password), a, t("Eigenschaften gespeichert"));
        return;
      }
      if (r.protect) {
        const wasPdfA = await ops.isPdfA(b, password);
        const nb = await ops.protect(b, r.protect, password);
        if (wasPdfA)
          toast(t("Hinweis: Das Dokument war ein PDF/A-Archivdokument. Der PDF/A-Standard erlaubt keine Verschlüsselung – die Kennzeichnung wurde deshalb entfernt."), "info");
        setPassword(r.protect.userPassword);
        const m = { ...metaRef.current, encrypted: true };
        setMeta(m);
        await db.putDoc(m);
        return applyBase(nb, a, t("Dokument ist jetzt kennwortgeschützt"));
      }
      if (r.unprotect && password) {
        const nb = await ops.removePassword(b, password);
        setPassword(undefined);
        const m = { ...metaRef.current, encrypted: false };
        setMeta(m);
        await db.putDoc(m);
        return applyBase(nb, a, t("Kennwortschutz entfernt"));
      }
      if (r.compress) {
        const nb = await ops.compress(b, r.compress, password, (i, n) => busy(t("Komprimiere Seite {0} von {1} …", i, n), i / n));
        if (nb === b) return void toast(t("Die Datei ist bereits optimal – keine Verkleinerung möglich."), "info");
        await applyBase(nb, a);
        toast(t("Von {0} auf {1} verkleinert", fmtSize(b.length), fmtSize(nb.length)));
        return;
      }
      if (r.images && doc) {
        const groups = r.images.range.trim() ? ops.parseRanges(r.images.range, total).flat() : Array.from({ length: total }, (_, i) => i);
        const js = await openPdf(await buildPdf(), password);
        const zip = await ops.exportImages(js, r.images.fmt, r.images.dpi, groups.map((i) => i + 1), (i, n) => busy(t("Exportiere Seite {0} von {1} …", i, n), i / n));
        js.destroy();
        busy(null);
        await saveAs(fileName(" (Bilder)", "zip"), zip);
        return;
      }
      if (r.split) {
        const groups =
          r.split.mode === "every"
            ? Array.from({ length: Math.ceil(total / r.split.every) }, (_, k) => Array.from({ length: Math.min(r.split!.every, total - k * r.split!.every) }, (_, j) => k * r.split!.every + j))
            : ops.parseRanges(r.split.ranges, total);
        if (!groups.length) return void toast(t("Ungültige Bereiche"), "err");
        const full = await ops.exportPdf(b, a, { password });
        const parts = await ops.split(full, groups, password);
        await importFiles(parts.map((p, i) => ({ name: fileName(` – Teil ${i + 1}`), data: p })));
        toast(t("{0} Dokumente erstellt", parts.length));
      }
    });

  const applyRedactions = async () => {
    const red = H.annots.filter((a) => a.type === "redact") as Extract<Annotation, { type: "redact" }>[];
    if (!red.length) return void toast(t("Markiere zuerst Bereiche mit dem Schwärzen-Werkzeug."), "info");
    const ok = await confirm(
      t("Schwärzungen anwenden?"),
      t("{0} Bereich(e) werden dauerhaft entfernt. Betroffene Seiten werden dazu als Bild neu aufgebaut – der Text darunter ist danach wirklich gelöscht. Das lässt sich nach dem Speichern nicht rückgängig machen.", red.length),
      t("Schwärzen"),
      true,
    );
    if (!ok) return;
    await run(t("Schwärze …"), async () => {
      const byPage = new Map<number, Rect[]>();
      red.forEach((r) => byPage.set(r.page, [...(byPage.get(r.page) ?? []), r.rect]));
      const nb = await ops.applyRedactions(await currentBase(), byPage, password);
      await applyBase(nb, H.annots.filter((a) => a.type !== "redact"), t("{0} Bereich(e) geschwärzt", red.length));
      setTool("select");
    });
  };

  const flattenForm = () =>
    run(t("Bitte warten …"), async () => {
      const b = await currentBase();
      if (!(await ops.hasForm(b, password))) return void toast(t("Dieses Dokument hat keine Formularfelder."), "info");
      await applyBase(await ops.flattenForm(b, password), H.annots, t("Formular fixiert – Felder sind jetzt fester Inhalt."));
    });

  // ------------------------------------------------------------------ Kommentare
  const add = (a: Annotation) => {
    H.set((l) => [...l, a]);
  };
  const change = (a: Annotation, commit: boolean) => H.change((l) => l.map((x) => (x.id === a.id ? a : x)), commit);
  const remove = (id: string) => {
    H.set((l) => l.filter((x) => x.id !== id));
    if (selectedId === id) setSelectedId(null);
  };

  const addMarkup = (type: MarkupType, data = selectionRects(viewer.current!)) => {
    if (!data.length) return false;
    const c = colors[type] ?? DEFAULT_COLORS[type]!;
    const now = Date.now();
    H.set((l) => [
      ...l,
      ...data.map((d, i) => ({ id: Math.random().toString(36).slice(2) + i, page: d.page, color: c, opacity: 1, createdAt: now, author: settings.author || undefined, type, rects: d.rects, text: d.text }) as Annotation),
    ]);
    getSelection()?.removeAllRanges();
    setSelPop(null);
    return true;
  };

  // Textauswahl → Markieren (direkt mit Werkzeug oder über das Popup)
  useEffect(() => {
    const onUp = (e: PointerEvent) => {
      const v = viewer.current;
      if (!v || !v.container()?.contains(e.target as Node)) return;
      setTimeout(() => {
        const data = selectionRects(v);
        if (!data.length) return setSelPop(null);
        if (MARKUP.includes(tool)) addMarkup(tool as MarkupType, data);
        else if (tool === "select") {
          const r = getSelection()!.getRangeAt(0).getBoundingClientRect();
          setSelPop({ x: r.left + r.width / 2, y: r.top, data });
        }
      }, 10);
    };
    const onSel = () => {
      if (getSelection()?.isCollapsed) setSelPop(null);
    };
    addEventListener("pointerup", onUp);
    document.addEventListener("selectionchange", onSel);
    return () => {
      removeEventListener("pointerup", onUp);
      document.removeEventListener("selectionchange", onSel);
    };
  }, [tool, colors, settings.author]);

  // ------------------------------------------------------------------ Tastenkürzel
  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      const tg = e.target as HTMLElement;
      if (tg.tagName === "INPUT" || tg.tagName === "TEXTAREA" || tg.isContentEditable) return;
      if (organize) return;
      const mod = e.ctrlKey || e.metaKey;
      const key = e.key.toLowerCase();
      if (mod && key === "z") {
        e.preventDefault();
        e.shiftKey ? H.redo() : H.undo();
      } else if (mod && key === "y") {
        e.preventDefault();
        H.redo();
      } else if (mod && key === "s") {
        e.preventDefault();
        save(e.shiftKey);
      } else if (mod && key === "f") {
        e.preventDefault();
        setPanel("search");
      } else if (mod && key === "p" && canPrint) {
        e.preventDefault();
        doPrint();
      } else if (mod && (key === "+" || key === "=")) {
        e.preventDefault();
        viewer.current?.zoom(1.2);
      } else if (mod && key === "-") {
        e.preventDefault();
        viewer.current?.zoom(1 / 1.2);
      } else if (mod && key === "0") {
        e.preventDefault();
        viewer.current?.setZoom("page-width");
      } else if (mod) return;
      else if (key === "escape") {
        if (immersive) setImmersive(false);
        else if (selectedId) setSelectedId(null);
        else setTool("select");
      } else if (key === "v") setTool("select");
      else if (key === "h") setTool("hand");
      else if (key === "pagedown" || (key === "arrowright" && settings.scrollMode !== "vertical")) viewer.current?.goTo(page + 1);
      else if (key === "pageup" || (key === "arrowleft" && settings.scrollMode !== "vertical")) viewer.current?.goTo(page - 1);
      else if (key === "home" && !e.shiftKey) viewer.current?.goTo(1);
      else if (key === "end" && !e.shiftKey) viewer.current?.goTo(total);
    };
    addEventListener("keydown", k);
    return () => removeEventListener("keydown", k);
  });

  // ------------------------------------------------------------------ Vorlesen
  const speak = async (from: number) => {
    const synth = window.speechSynthesis;
    if (!synth || !doc) return toast(t("Vorlesen wird auf diesem Gerät nicht unterstützt."), "info");
    synth.cancel();
    setSpeaking(true);
    for (let p = from; p <= doc.numPages; p++) {
      const text = (await pageText(doc, p)).trim();
      if (!text) continue;
      viewer.current?.goTo(p);
      const ok = await new Promise<boolean>((resolve) => {
        const u = new SpeechSynthesisUtterance(text);
        u.lang = settings.lang === "de" ? "de-DE" : "en-US";
        u.rate = 1;
        u.onend = () => resolve(true);
        u.onerror = () => resolve(false);
        synth.speak(u);
      });
      if (!ok || !synth.speaking && !ok) break;
      if (!speakingRef.current) break;
    }
    setSpeaking(false);
  };
  const speakingRef = useRef(false);
  speakingRef.current = speaking;

  // ------------------------------------------------------------------ Vollbild / Präsentation
  const toggleImmersive = async () => {
    const next = !immersive;
    setImmersive(next);
    try {
      if (next && !isAndroid) await rootRef.current?.requestFullscreen?.();
      else if (document.fullscreenElement) await document.exitFullscreen();
    } catch {
      /* nicht unterstützt */
    }
  };
  useEffect(() => {
    const f = () => !document.fullscreenElement && setImmersive(false);
    document.addEventListener("fullscreenchange", f);
    return () => document.removeEventListener("fullscreenchange", f);
  }, []);

  // ------------------------------------------------------------------ Werkzeugleisten
  const toolOpts: ToolOpts = { color, width, opacity, fontSize, fill, presetText: preset, pendingImage: pending };

  const pickImage = async () => {
    const [f] = await pickFiles("image/*", false);
    if (!f) return;
    const blob = new Blob([f.data], { type: mimeOf(f.name) });
    const bmp = await createImageBitmap(blob);
    const c = document.createElement("canvas");
    const s = Math.min(1, 1600 / Math.max(bmp.width, bmp.height));
    c.width = bmp.width * s;
    c.height = bmp.height * s;
    c.getContext("2d")!.drawImage(bmp, 0, 0, c.width, c.height);
    const png = /png|gif|webp/.test(blob.type);
    const src = c.toDataURL(png ? "image/png" : "image/jpeg", 0.9);
    setTool("image");
    setPending({ src, aspect: await aspectOf(src), kind: "image" });
    toast(t("Tippe auf die Seite, um das Bild zu platzieren"), "info");
  };

  type TB = { id: string; icon: any; label: string; active?: boolean; onClick: () => void; disabled?: boolean; danger?: boolean };
  const SEP = (id: string): TB => ({ id: "sep-" + id, icon: null, label: "", onClick: () => {} });
  // Kurze Erklärungen für die Tooltips
  const TIPS: Record<string, string> = {
    select: t("Text markieren und Kommentare auswählen, verschieben oder ändern"),
    hand: t("Seite mit der Maus ziehen, statt Text auszuwählen"),
    zo: t("Ansicht verkleinern"),
    zi: t("Ansicht vergrößern"),
    fw: t("Seite an die Fensterbreite anpassen"),
    fp: t("Ganze Seite im Fenster zeigen"),
    sv: t("Alle Seiten untereinander scrollen"),
    sh: t("Seiten nebeneinander, seitlich scrollen"),
    sp: t("Immer nur eine Seite zeigen"),
    dp: t("Zwei Seiten nebeneinander, wie ein Buch"),
    rv: t("Nur die Ansicht drehen – das Dokument bleibt unverändert"),
    rn: t("Normale Darstellung"),
    rs: t("Warme Papierfarbe, angenehm für lange Texte"),
    rd: t("Dunkle Seiten zum Lesen bei wenig Licht"),
    tts: t("Text ab der aktuellen Seite vorlesen lassen"),
    fs: t("Ohne Leisten lesen oder präsentieren (Esc beendet)"),
    highlight: t("Text auswählen – er wird farbig markiert"),
    underline: t("Text auswählen – er wird unterstrichen"),
    strike: t("Text auswählen – er wird durchgestrichen"),
    note: t("Klicken, um einen Notizzettel anzuheften"),
    text: t("Klicken und Text direkt auf die Seite schreiben"),
    ink: t("Freihand zeichnen oder schreiben"),
    highlighter: t("Wie ein Textmarker frei über die Seite malen (Umschalt = gerade Linie)"),
    rect: t("Rahmen aufziehen (Umschalt = Quadrat)"),
    ellipse: t("Kreis oder Ellipse aufziehen (Umschalt = Kreis)"),
    line: t("Gerade Linie ziehen (Umschalt = 45°-Schritte)"),
    arrow: t("Pfeil ziehen, um auf etwas zu zeigen"),
    stamp: t("Stempel wie „Genehmigt“ oder „Entwurf“ aufsetzen"),
    image: t("Ein Bild (Logo, Foto) auf der Seite platzieren"),
    eraser: t("Über Kommentare fahren, um sie zu löschen"),
    ftext: t("Formular ohne Felder ausfüllen: Text hineinschreiben"),
    check: t("Häkchen in Kästchen setzen – mehrfach klicken möglich"),
    cross: t("Kreuz in Kästchen setzen"),
    dot: t("Punkt für Auswahlfelder setzen"),
    date: t("Heutiges Datum einfügen"),
    sig: t("Unterschrift zeichnen, tippen oder als Bild einfügen"),
    ini: t("Kürzel / Initialen einfügen"),
    forms: t("Formularfelder farbig hinterlegen"),
    flat: t("Ausgefüllte Felder fest ins Dokument übernehmen"),
    org: t("Alle Seiten als Raster: sortieren, drehen, löschen, einfügen"),
    rl: t("Aktuelle Seite um 90° nach links drehen"),
    rr: t("Aktuelle Seite um 90° nach rechts drehen"),
    ins: t("Leere Seite hinter der aktuellen einfügen"),
    dup: t("Aktuelle Seite verdoppeln"),
    ext: t("Aktuelle Seite als eigenes PDF speichern"),
    del: t("Aktuelle Seite aus dem Dokument entfernen"),
    merge: t("Weitere PDFs hinten an dieses Dokument anhängen"),
    split: t("In mehrere PDFs aufteilen"),
    comp: t("Datei verkleinern, z. B. für E-Mails"),
    prot: t("Mit Kennwort verschlüsseln oder Schutz entfernen"),
    redact: t("Vertrauliches dauerhaft schwärzen"),
    wm: t("Text wie „Vertraulich“ quer über alle Seiten legen"),
    num: t("Seitenzahlen, Kopf- oder Fußzeile hinzufügen"),
    img: t("Seiten als PNG/JPG speichern"),
    txt: t("Den gesamten Text als .txt speichern"),
    meta: t("Titel, Autor, Stichwörter bearbeiten"),
    print: t("Dokument mit Kommentaren drucken"),
  };
  const isMacUi = isMac;
  /** "Auswählen (V)" → Beschriftung "Auswählen", Kürzel "V" */
  const splitKey = (label: string) => {
    const m = label.match(/^(.*?)\s*\((.+)\)$/);
    return m ? { label: m[1], key: m[2].replace("Strg", isMacUi ? "⌘" : "Strg") } : { label };
  };
  const T = (tl: Tool, icon: any, label: string): TB => ({ id: tl, icon, label, active: tool === tl, onClick: () => setTool(tool === tl ? "select" : tl) });
  const zoomPct = Math.round(scale * 100);

  const tabs: { id: Tab; label: string; icon: any }[] = [
    { id: "view", label: t("Ansehen"), icon: Eye },
    { id: "comment", label: t("Kommentieren"), icon: Highlighter },
    { id: "fill", label: t("Ausfüllen & Signieren"), icon: Signature },
    { id: "pages", label: t("Seiten"), icon: LayoutGrid },
    { id: "tools", label: t("Werkzeuge"), icon: Layers },
  ];

  const toolbars: Record<Tab, TB[]> = {
    view: [
      T("select", MousePointer2, t("Auswählen (V)")),
      T("hand", Hand, t("Hand (H)")),
      SEP("a"),
      { id: "zo", icon: ZoomOut, label: t("Verkleinern"), onClick: () => viewer.current?.zoom(1 / 1.2) },
      { id: "zi", icon: ZoomIn, label: t("Vergrößern"), onClick: () => viewer.current?.zoom(1.2) },
      { id: "fw", icon: MoveHorizontal, label: t("Seitenbreite"), onClick: () => viewer.current?.setZoom("page-width") },
      { id: "fp", icon: ScanLine, label: t("Ganze Seite"), onClick: () => viewer.current?.setZoom("page-fit") },
      SEP("b"),
      { id: "sv", icon: Rows3, label: t("Fortlaufend"), active: settings.scrollMode === "vertical", onClick: () => setSettings({ scrollMode: "vertical" }) },
      { id: "sh", icon: Columns3, label: t("Horizontal"), active: settings.scrollMode === "horizontal", onClick: () => setSettings({ scrollMode: "horizontal" }) },
      { id: "sp", icon: BookOpen, label: t("Einzelseite"), active: settings.scrollMode === "page", onClick: () => setSettings({ scrollMode: "page" }) },
      { id: "dp", icon: LayoutPanelLeft, label: t("Doppelseite"), active: settings.spread !== "none", onClick: () => setSettings({ spread: settings.spread === "none" ? "odd" : "none" }) },
      SEP("c"),
      { id: "rv", icon: RotateCw, label: t("Ansicht drehen"), onClick: () => viewer.current?.rotateView(90) },
      SEP("d"),
      { id: "rn", icon: Sun, label: t("Normal"), active: settings.reading === "normal", onClick: () => setSettings({ reading: "normal" }) },
      { id: "rs", icon: Coffee, label: t("Sepia"), active: settings.reading === "sepia", onClick: () => setSettings({ reading: "sepia" }) },
      { id: "rd", icon: Moon, label: t("Nachtmodus"), active: settings.reading === "night", onClick: () => setSettings({ reading: "night" }) },
      SEP("e"),
      { id: "tts", icon: speaking ? VolumeX : Volume2, label: speaking ? t("Vorlesen stoppen") : t("Vorlesen"), active: speaking, onClick: () => (speaking ? (setSpeaking(false), window.speechSynthesis?.cancel()) : speak(page)) },
      { id: "fs", icon: immersive ? Minimize : Maximize, label: t("Vollbild"), onClick: toggleImmersive },
    ],
    comment: [
      T("select", MousePointer2, t("Auswählen")),
      SEP("a"),
      T("highlight", Highlighter, t("Markieren")),
      T("underline", Underline, t("Unterstreichen")),
      T("strike", Strikethrough, t("Durchstreichen")),
      SEP("b"),
      T("note", StickyNote, t("Notiz")),
      T("text", Type, t("Textfeld")),
      SEP("c"),
      T("ink", PenLine, t("Stift")),
      T("highlighter", Brush, t("Freier Marker")),
      T("rect", Square, t("Rechteck")),
      T("ellipse", Circle, t("Ellipse")),
      T("line", Minus, t("Linie")),
      T("arrow", MoveUpRight, t("Pfeil")),
      SEP("d"),
      { id: "stamp", icon: Stamp, label: t("Stempel"), active: tool === "stamp", onClick: () => setDialog("stamp") },
      { id: "image", icon: ImagePlus, label: t("Bild"), active: tool === "image", onClick: pickImage },
      SEP("e"),
      T("eraser", Eraser, t("Radierer")),
    ],
    fill: [
      T("select", MousePointer2, t("Auswählen")),
      SEP("a"),
      { id: "ftext", icon: CaseSensitive, label: t("Text"), active: tool === "text" && !preset, onClick: () => { setTool("text"); setColors((c) => ({ ...c, text: "#111111" })); setFontSize(11); } },
      { id: "check", icon: Check, label: t("Häkchen"), active: preset === "✓", onClick: () => { setTool("text"); setPreset("✓"); } },
      { id: "cross", icon: XIcon, label: t("Kreuz"), active: preset === "✗", onClick: () => { setTool("text"); setPreset("✗"); } },
      { id: "dot", icon: Dot, label: t("Punkt"), active: preset === "●", onClick: () => { setTool("text"); setPreset("●"); } },
      { id: "date", icon: CalendarDays, label: t("Datum"), active: !!preset && preset.length > 2, onClick: () => { setTool("text"); setPreset(new Date().toLocaleDateString()); } },
      SEP("b"),
      { id: "sig", icon: Signature, label: t("Unterschreiben"), active: tool === "signature" && pending?.kind === "signature", onClick: () => setDialog("signature") },
      { id: "ini", icon: PenLine, label: t("Initialen"), onClick: () => setDialog("initials") },
      SEP("c"),
      { id: "forms", icon: FormInput, label: t("Felder hervorheben"), active: showForms, onClick: () => setShowForms(!showForms) },
      { id: "flat", icon: Layers, label: t("Formular fixieren"), onClick: flattenForm },
    ],
    pages: [
      { id: "org", icon: LayoutGrid, label: t("Seiten organisieren"), onClick: () => setOrganize(true) },
      SEP("a"),
      { id: "rl", icon: RotateCcw, label: t("Seite links drehen"), onClick: () => pageOp({ op: "rotate", pages: [page - 1], delta: -90 }) },
      { id: "rr", icon: RotateCw, label: t("Seite rechts drehen"), onClick: () => pageOp({ op: "rotate", pages: [page - 1], delta: 90 }) },
      { id: "ins", icon: FilePlus2, label: t("Leere Seite danach"), onClick: () => pageOp({ op: "blank", at: page }) },
      { id: "dup", icon: Copy, label: t("Seite duplizieren"), onClick: () => pageOp({ op: "duplicate", pages: [page - 1] }) },
      { id: "ext", icon: FileOutput, label: t("Seite extrahieren"), onClick: () => pageOp({ op: "extract", pages: [page - 1] }) },
      SEP("b"),
      { id: "del", icon: Trash2, label: t("Seite löschen"), disabled: total <= 1, danger: true, onClick: async () => (await confirm(t("Seite löschen?"), t("Seite {0} wird aus dem Dokument entfernt.", page), t("Löschen"), true)) && pageOp({ op: "delete", pages: [page - 1] }) },
    ],
    tools: [
      { id: "merge", icon: Combine, label: t("Dateien anhängen"), onClick: () => pageOp({ op: "file", at: total }) },
      { id: "split", icon: Scissors, label: t("Teilen"), onClick: () => setToolDialog("split") },
      SEP("a"),
      { id: "comp", icon: Minimize2, label: t("Komprimieren"), onClick: () => setToolDialog("compress") },
      { id: "prot", icon: Lock, label: password ? t("Kennwort entfernen") : t("Schützen"), onClick: () => setToolDialog("protect") },
      { id: "redact", icon: EyeOff, label: t("Schwärzen"), active: tool === "redact", onClick: () => { setTool(tool === "redact" ? "select" : "redact"); if (tool !== "redact") toast(t("Ziehe Rahmen über die zu schwärzenden Bereiche und tippe dann auf „Anwenden“."), "info"); } },
      SEP("b"),
      { id: "wm", icon: Droplets, label: t("Wasserzeichen"), onClick: () => setToolDialog("watermark") },
      { id: "num", icon: Hash, label: t("Seitenzahlen"), onClick: () => setToolDialog("numbers") },
      SEP("c"),
      { id: "img", icon: Images, label: t("Als Bilder"), onClick: () => setToolDialog("images") },
      { id: "txt", icon: FileText, label: t("Als Text"), onClick: () => exportAs("text") },
      { id: "meta", icon: Info, label: t("Eigenschaften"), onClick: async () => { setPdfMeta(await ops.getMeta(await currentBase(), password).catch(() => null)); setToolDialog("meta"); } },
      ...(canPrint ? [{ id: "print", icon: Printer, label: t("Drucken"), onClick: doPrint }] : []),
    ],
  };

  const redactCount = H.annots.filter((a) => a.type === "redact").length;
  const showProps = ["ink", "highlighter", "rect", "ellipse", "line", "arrow", "text", "note", ...MARKUP].includes(tool) && !preset;

  const moreItems: MenuItem[] = [
    { label: t("Speichern unter …"), icon: <Download size={16} />, onClick: () => save(true), hint: isDesktop ? "⇧⌘S" : undefined },
    ...(isAndroid || (navigator as any).canShare ? [{ label: t("Teilen"), icon: <Share2 size={16} />, onClick: doShare }] : []),
    ...(canPrint ? [{ label: t("Drucken"), icon: <Printer size={16} />, onClick: doPrint, hint: "⌘P" }] : []),
    { divider: true, label: "" },
    { label: t("Mit fixierten Kommentaren exportieren"), icon: <Layers size={16} />, onClick: () => exportAs("flat") },
    { label: t("Ohne Kommentare exportieren"), icon: <FileOutput size={16} />, onClick: () => exportAs("clean") },
    { label: t("Als Bilder exportieren"), icon: <Images size={16} />, onClick: () => setToolDialog("images") },
    { label: t("Als Text exportieren"), icon: <FileText size={16} />, onClick: () => exportAs("text") },
    { divider: true, label: "" },
    { label: t("Gehe zu Seite …"), icon: <BookOpen size={16} />, onClick: () => setDialog("goto") },
    { label: t("Eigenschaften"), icon: <Info size={16} />, onClick: () => toolbars.tools.find((x) => x.id === "meta")!.onClick() },
    {
      label: t("Alle Kommentare löschen"),
      icon: <Trash2 size={16} />,
      danger: true,
      disabled: !H.annots.length,
      onClick: async () => (await confirm(t("Alle Kommentare löschen?"), t("{0} Kommentare werden entfernt. Mit Rückgängig wiederherstellbar.", H.annots.length), t("Löschen"), true)) && H.set([]),
    },
  ];

  const overlayFor = useCallback(
    (n: number, g: any) => (
      <AnnotationOverlay
        page={n}
        geom={g}
        annots={H.annots.filter((a) => a.page === n)}
        tool={tool}
        opts={toolOpts}
        author={settings.author}
        selectedId={selectedId}
        editingId={editingId}
        onSelect={setSelectedId}
        onEdit={setEditingId}
        onChange={change}
        onAdd={add}
        onDelete={remove}
        onPlaced={() => {
          if (tool === "text" && preset) return; // Häkchen o. ä. mehrfach setzen
          if (tool === "text" || tool === "note" || tool === "signature" || tool === "image" || tool === "stamp") {
            setToolRaw("select");
            setPending(null);
          }
        }}
      />
    ),
    [H.annots, tool, color, width, opacity, fontSize, fill, preset, pending, selectedId, editingId, settings.author],
  );

  // ------------------------------------------------------------------ Darstellung
  const activeBar = toolbars[tab];
  return (
    <div ref={rootRef} className={cx("reader", immersive && "is-immersive", mobile && "is-mobile", !showForms && "hide-forms")}>
      {!immersive && (
        <header className="reader-top">
          <IconBtn icon={<ArrowLeft size={20} />} label={t("Zurück zur Bibliothek")} onClick={back} />
          <div className="reader-title">
            <b title={meta.name}>{meta.name.replace(/\.pdf$/i, "")}</b>
            <span>
              {total ? t("{0} Seiten", total) : ""}
              {meta.encrypted && " · 🔒"}
              {dirty && isDesktop && meta.path ? " · " + t("nicht gespeichert") : ""}
            </span>
          </div>
          <span className="grow" />
          <IconBtn icon={<Undo2 size={19} />} label={t("Rückgängig (Strg+Z)")} disabled={!H.canUndo} onClick={H.undo} />
          <IconBtn icon={<Redo2 size={19} />} label={t("Wiederholen (Strg+Y)")} disabled={!H.canRedo} onClick={H.redo} />
          {!mobile && <IconBtn icon={<Search size={19} />} label={t("Suchen (Strg+F)")} active={panel === "search"} onClick={() => setPanel(panel === "search" ? null : "search")} />}
          <IconBtn icon={<PanelLeft size={19} />} label={t("Seitenleiste")} tip={t("Miniaturen, Inhalt, Kommentare und Suche ein-/ausblenden")} active={!!panel} onClick={() => setPanel(panel ? null : "thumbs")} />
          {!mobile && <ThemeToggle settings={settings} setSettings={setSettings} />}
          {mobile ? (
            <IconBtn icon={isAndroid ? <Share2 size={19} /> : <Save size={19} />} label={isAndroid ? t("Teilen") : t("Speichern")} onClick={isAndroid ? doShare : () => save()} />
          ) : (
            <Btn kind="primary" onClick={() => save()} className="reader-save">
              <Save size={16} /> {t("Speichern")}
            </Btn>
          )}
          <IconBtn icon={<MoreVertical size={19} />} label={t("Mehr")} onClick={moreMenu.open} />
          {moreMenu.anchor && <Menu anchor={moreMenu.anchor} items={moreItems} onClose={moreMenu.close} />}
          {saveMenu.anchor && <Menu anchor={saveMenu.anchor} items={[]} onClose={saveMenu.close} />}
        </header>
      )}

      {!immersive && !mobile && (
        <nav className="ribbon">
          <div className="ribbon-tabs">
            {tabs.map((tb) => (
              <button key={tb.id} className={cx(tab === tb.id && "is-active")} onClick={() => (tb.id === "pages" && tab === "pages" ? setOrganize(true) : setTab(tb.id))}>
                {tb.label}
              </button>
            ))}
          </div>
          <div className="ribbon-tools">
            {activeBar.map((b) => {
              if (b.id.startsWith("sep-")) return <span key={b.id} className="ribbon-sep" />;
              const { label, key } = splitKey(b.label);
              return (
                <IconBtn
                  key={b.id}
                  icon={<b.icon size={19} />}
                  label={label}
                  tip={TIPS[b.id]}
                  shortcut={key}
                  showLabel
                  active={b.active}
                  disabled={b.disabled}
                  onClick={b.onClick}
                  className={b.danger ? "danger" : undefined}
                />
              );
            })}
            {tab === "view" && <span className="ribbon-zoom">{zoomPct} %</span>}
            {tab === "tools" && redactCount > 0 && (
              <Btn kind="danger" onClick={applyRedactions}>
                <EyeOff size={15} /> {t("{0} Schwärzung(en) anwenden", redactCount)}
              </Btn>
            )}
          </div>
        </nav>
      )}

      <div className="reader-body">
        {panel && !immersive && (
          <>
            {mobile && <div className="drawer-backdrop" onClick={() => setPanel(null)} />}
            <SidePanel
              kind={panel}
              setKind={setPanel}
              doc={doc}
              page={page}
              total={total}
              annots={H.annots}
              selectedId={selectedId}
              onGo={(p) => {
                viewer.current?.goTo(p);
                if (mobile) setPanel(null);
              }}
              onDest={(d, url) => {
                if (url) window.open(url, "_blank");
                else viewer.current?.goToDest(d);
                if (mobile) setPanel(null);
              }}
              onPick={(a) => {
                viewer.current?.goTo(a.page);
                setTool("select");
                setTimeout(() => setSelectedId(a.id), 30);
                if (mobile) setPanel(null);
              }}
              onDelete={remove}
              onNote={(a, note) => change({ ...a, note: note || undefined } as Annotation, true)}
              onFind={(q, o) => viewer.current?.find(q, o)}
              findResult={findResult}
              onCloseSearch={() => {
                viewer.current?.clearFind();
                setPanel(null);
              }}
            />
          </>
        )}
        <main className="reader-main">
          {base && (
            <Viewer
              key={version}
              ref={viewer}
              data={base}
              password={password}
              scrollMode={settings.scrollMode}
              spread={settings.spread}
              reading={settings.reading}
              startPage={page}
              panMode={tool === "hand"}
              onPassword={(wrong) => setPwAsk({ wrong })}
              onError={(e) => toast(t("Konnte das PDF nicht öffnen: {0}", e.message), "err")}
              onReady={(d) => {
                setDoc(d);
                setTotal(d.numPages);
                if (meta.encrypted !== !!password) {
                  const m = { ...metaRef.current, encrypted: !!password };
                  setMeta(m);
                  db.putDoc(m);
                }
              }}
              onPage={(p, n) => {
                setPage(p);
                setTotal(n);
              }}
              onScale={setScale}
              onFind={setFindResult}
              renderOverlay={overlayFor}
              onBackgroundTap={() => {
                setSelectedId(null);
                if (mobile && !immersive) return;
              }}
            />
          )}

          {selPop && tool === "select" && (
            <div className="sel-pop" style={{ left: selPop.x, top: selPop.y }} onPointerDown={(e) => e.preventDefault()}>
              <button onClick={() => addMarkup("highlight", selPop.data)} data-tip={t("Markieren")}>
                <Highlighter size={16} style={{ color: colors.highlight }} /> <span>{t("Markieren")}</span>
              </button>
              <button onClick={() => addMarkup("underline", selPop.data)} data-tip={t("Unterstreichen")}>
                <Underline size={16} />
              </button>
              <button onClick={() => addMarkup("strike", selPop.data)} data-tip={t("Durchstreichen")}>
                <Strikethrough size={16} />
              </button>
              <button
                onClick={() => {
                  navigator.clipboard?.writeText(getSelection()?.toString() ?? "");
                  toast(t("Kopiert"));
                  setSelPop(null);
                }}
                title={t("Kopieren")}
              >
                <Copy size={16} />
              </button>
            </div>
          )}

          {showProps && !immersive && (
            <div className={cx("propbar", mobile && "propbar-mobile")}>
              <Swatches value={color} onChange={(c) => setColors((x) => ({ ...x, [tool]: c }))} colors={tool === "highlight" || tool === "highlighter" ? ["#f6c945", "#9bd37a", "#7cc4f0", "#f29bc0", "#f0905a", "#c58cf2"] : undefined} />
              {["ink", "rect", "ellipse", "line", "arrow"].includes(tool) && (
                <div className="propbar-item">
                  <span>{t("Stärke")}</span>
                  <Range value={width} min={0.5} max={12} step={0.5} onChange={setWidth} />
                </div>
              )}
              {tool === "highlighter" && (
                <div className="propbar-item">
                  <span>{t("Stärke")}</span>
                  <Range value={width} min={1} max={8} step={0.5} onChange={setWidth} />
                </div>
              )}
              {["ink", "rect", "ellipse", "line", "arrow", "text"].includes(tool) && (
                <div className="propbar-item">
                  <span>{t("Deckkraft")}</span>
                  <Range value={Math.round(opacity * 100)} min={10} max={100} step={5} onChange={(v) => setOpacity(v / 100)} suffix="%" />
                </div>
              )}
              {tool === "text" && (
                <div className="propbar-item">
                  <span>{t("Größe")}</span>
                  <Range value={fontSize} min={6} max={48} onChange={setFontSize} suffix="pt" />
                </div>
              )}
              {(tool === "rect" || tool === "ellipse") && (
                <label className="check">
                  <input type="checkbox" checked={!!fill} onChange={(e) => setFill(e.target.checked ? color + "33" : null)} /> {t("Füllen")}
                </label>
              )}
            </div>
          )}

          {(tool === "signature" || tool === "image" || tool === "stamp") && pending && (
            <div className="place-hint">
              <img src={pending.src} alt="" />
              <span>{t("Tippe auf die Stelle, an der es erscheinen soll")}</span>
              <button className="iconbtn iconbtn-sm" onClick={() => setTool("select")} aria-label={t("Abbrechen")}>
                <XIcon size={15} />
              </button>
            </div>
          )}

          {mobile && tab === "tools" && redactCount > 0 && !immersive && (
            <div className="place-hint">
              <Btn kind="danger" onClick={applyRedactions}>
                <EyeOff size={15} /> {t("{0} Schwärzung(en) anwenden", redactCount)}
              </Btn>
            </div>
          )}

          <div className={cx("pagepill", immersive && "is-immersive")}>
            <button onClick={() => viewer.current?.goTo(page - 1)} disabled={page <= 1} aria-label={t("Vorherige Seite")}>
              <ChevronLeft size={16} />
            </button>
            <button className="pagepill-no" onClick={() => setDialog("goto")} data-tip={t("Gehe zu Seite …")} data-tip-desc={t("Seitenzahl eingeben und direkt springen")}>
              {page} / {total || "…"}
            </button>
            <button onClick={() => viewer.current?.goTo(page + 1)} disabled={page >= total} aria-label={t("Nächste Seite")}>
              <ChevronRight size={16} />
            </button>
            {!mobile && (
              <>
                <span className="pagepill-sep" />
                <button onClick={() => viewer.current?.zoom(1 / 1.2)} aria-label={t("Verkleinern")}>
                  <ZoomOut size={15} />
                </button>
                <button className="pagepill-zoom" onClick={() => viewer.current?.setZoom("page-width")} data-tip={t("Seitenbreite")} data-tip-desc={t("Zoom zurücksetzen: Seite an die Fensterbreite anpassen")}>
                  {zoomPct}%
                </button>
                <button onClick={() => viewer.current?.zoom(1.2)} aria-label={t("Vergrößern")}>
                  <ZoomIn size={15} />
                </button>
              </>
            )}
            {immersive && (
              <button onClick={toggleImmersive} aria-label={t("Vollbild beenden")}>
                <Minimize size={15} />
              </button>
            )}
          </div>
        </main>
      </div>

      {mobile && !immersive && (
        <footer className="mobile-bar">
          <div className="mobile-tools">
            {activeBar.filter((b) => !b.id.startsWith("sep-")).map((b) => (
              <IconBtn key={b.id} icon={<b.icon size={19} />} label={b.label.replace(/\s*\(.*\)$/, "")} active={b.active} disabled={b.disabled} onClick={b.onClick} showLabel className={b.danger ? "danger" : undefined} />
            ))}
          </div>
          <div className="mobile-tabs">
            {tabs.map((tb) => (
              <button key={tb.id} className={cx(tab === tb.id && "is-active")} onClick={() => (tb.id === "pages" && tab === "pages" ? setOrganize(true) : setTab(tb.id))}>
                <tb.icon size={19} />
                <span>{tb.id === "fill" ? t("Signieren") : tb.label}</span>
              </button>
            ))}
          </div>
        </footer>
      )}

      {organize && doc && (
        <Organizer doc={doc} total={total} onOp={pageOp} onClose={() => setOrganize(false)} onSplit={() => setToolDialog("split")} canUndo={pageUndo.length > 0} onUndo={undoPageOp} version={version} />
      )}

      <SignatureDialog
        open={dialog === "signature" || dialog === "initials"}
        kind={dialog === "initials" ? "initials" : "signature"}
        onClose={() => setDialog(null)}
        onPick={(img) => {
          setToolRaw("signature");
          setPending(img);
        }}
      />
      <StampDialog
        open={dialog === "stamp"}
        onClose={() => setDialog(null)}
        onPick={(img) => {
          setToolRaw("stamp");
          setPending(img);
        }}
      />
      <ToolDialog kind={toolDialog} onClose={() => setToolDialog(null)} onRun={runTool} total={total} meta={pdfMeta} encrypted={!!password} size={base?.length ?? 0} />

      {dialog === "goto" && <GotoDialog total={total} page={page} onClose={() => setDialog(null)} onGo={(p) => viewer.current?.goTo(p)} />}
      {dialog === "unsaved" && (
        <Modal
          open
          title={t("Änderungen speichern?")}
          onClose={() => setDialog(null)}
          footer={
            <>
              <Btn kind="ghost" onClick={() => (setDialog(null), onClose())}>
                {t("Nicht speichern")}
              </Btn>
              <Btn kind="primary" onClick={async () => (await save()) && onClose()}>
                {t("Speichern")}
              </Btn>
            </>
          }
        >
          <p className="muted">{t("„{0}“ enthält Änderungen, die noch nicht in die Datei geschrieben wurden. In der Bibliothek bleiben sie in jedem Fall erhalten.", meta.name)}</p>
        </Modal>
      )}
      {pwAsk && (
        <PasswordDialog
          wrong={pwAsk.wrong}
          name={meta.name}
          onCancel={() => {
            setPwAsk(null);
            onClose();
          }}
          onOk={(pw) => {
            setPwAsk(null);
            setPassword(pw);
          }}
        />
      )}
      {confirmNode}
    </div>
  );
}

function GotoDialog({ total, page, onClose, onGo }: { total: number; page: number; onClose: () => void; onGo: (p: number) => void }) {
  const t = useT();
  const [v, setV] = useState(String(page));
  const go = () => {
    const n = parseInt(v, 10);
    if (n >= 1 && n <= total) {
      onGo(n);
      onClose();
    }
  };
  return (
    <Modal open title={t("Gehe zu Seite")} onClose={onClose} footer={<Btn kind="primary" onClick={go}>{t("Los")}</Btn>}>
      <input
        className="input"
        type="number"
        min={1}
        max={total}
        autoFocus
        value={v}
        onChange={(e) => setV(e.target.value)}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === "Enter") go();
        }}
      />
      <p className="field-hint">{t("1 bis {0}", total)}</p>
    </Modal>
  );
}

export function PasswordDialog({ wrong, name, onOk, onCancel }: { wrong: boolean; name: string; onOk: (pw: string) => void; onCancel: () => void }) {
  const t = useT();
  const [pw, setPw] = useState("");
  return (
    <Modal
      open
      icon={<Lock size={18} />}
      title={t("Kennwort erforderlich")}
      onClose={onCancel}
      footer={
        <>
          <Btn kind="ghost" onClick={onCancel}>
            {t("Abbrechen")}
          </Btn>
          <Btn kind="primary" disabled={!pw} onClick={() => onOk(pw)}>
            {t("Öffnen")}
          </Btn>
        </>
      }
    >
      <p className="muted">{t("„{0}“ ist durch ein Kennwort geschützt.", name)}</p>
      <input
        className={cx("input", wrong && "input-error")}
        type="password"
        autoFocus
        value={pw}
        placeholder={t("Kennwort")}
        onChange={(e) => setPw(e.target.value)}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === "Enter" && pw) onOk(pw);
        }}
      />
      {wrong && <p className="field-hint error">{t("Falsches Kennwort – bitte erneut versuchen.")}</p>}
    </Modal>
  );
}

/** Umschalter Hell ⇄ Dunkel */
export function ThemeToggle({ settings, setSettings, showLabel }: { settings: Settings; setSettings: (s: Partial<Settings>) => void; showLabel?: boolean }) {
  const t = useT();
  const dark = settings.theme === "dark" || (settings.theme === "system" && matchMedia("(prefers-color-scheme: dark)").matches);
  return (
    <IconBtn
      icon={dark ? <Sun size={19} /> : <Moon size={19} />}
      label={dark ? t("Heller Modus") : t("Dunkler Modus")}
      tip={t("Zwischen hellem und dunklem Design wechseln")}
      showLabel={showLabel}
      onClick={() => setSettings({ theme: dark ? "light" : "dark" })}
    />
  );
}

export function useMedia(q: string) {
  const [m, setM] = useState(() => matchMedia(q).matches);
  useEffect(() => {
    const mq = matchMedia(q);
    const f = () => setM(mq.matches);
    mq.addEventListener("change", f);
    return () => mq.removeEventListener("change", f);
  }, [q]);
  return m;
}

export function mimeOf(name: string) {
  const e = name.split(".").pop()!.toLowerCase();
  return { png: "image/png", jpg: "image/jpeg", jpeg: "image/jpeg", webp: "image/webp", gif: "image/gif", bmp: "image/bmp", heic: "image/heic" }[e] ?? "application/octet-stream";
}

function compactRanges(pages: number[]) {
  const s = [...pages].sort((a, b) => a - b).map((p) => p + 1);
  const out: string[] = [];
  for (let i = 0; i < s.length; i++) {
    let j = i;
    while (j + 1 < s.length && s[j + 1] === s[j] + 1) j++;
    out.push(i === j ? String(s[i]) : `${s[i]}-${s[j]}`);
    i = j;
  }
  return out.join(",");
}

export { useMemo };
