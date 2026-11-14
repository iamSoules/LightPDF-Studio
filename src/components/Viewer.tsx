import { forwardRef, useEffect, useImperativeHandle, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { loadViewer, openPdf, PasswordNeeded, pdfjsLib, type PDFDocumentProxy } from "../lib/pdfjs";
import "pdfjs-dist/legacy/web/pdf_viewer.css";
import type { Pt } from "../types";

/** Geometrie einer Seite: Umrechnung PDF-Punkte ⇄ Overlay-Koordinaten (Viewport bei Maßstab 1) */
export interface PageGeom {
  page: number;
  width: number;
  height: number;
  rotation: number;
  toView: (p: Pt) => Pt;
  toPdf: (x: number, y: number) => Pt;
}

export interface ViewerHandle {
  doc: () => PDFDocumentProxy | null;
  goTo: (page: number) => void;
  goToDest: (dest: unknown) => void;
  zoom: (factor: number) => void;
  setZoom: (v: "page-width" | "page-fit" | "auto" | number) => void;
  rotateView: (delta: number) => void;
  find: (query: string, opts?: { again?: boolean; prev?: boolean; caseSensitive?: boolean; wholeWord?: boolean }) => void;
  clearFind: () => void;
  formBytes: () => Promise<Uint8Array | null>;
  geom: (page: number) => PageGeom | null;
  pageAtClient: (x: number, y: number) => { page: number; geom: PageGeom; rect: DOMRect } | null;
  scrollBy: (dx: number, dy: number) => void;
  container: () => HTMLDivElement | null;
  currentPage: () => number;
}

interface Props {
  data: Uint8Array;
  password?: string;
  scrollMode: "vertical" | "horizontal" | "wrapped" | "page";
  spread: "none" | "odd" | "even";
  reading: "normal" | "night" | "sepia";
  startPage?: number;
  panMode?: boolean;
  onPassword: (wrong: boolean) => void;
  onError: (e: Error) => void;
  onReady: (doc: PDFDocumentProxy) => void;
  onPage: (page: number, total: number) => void;
  onScale: (scale: number) => void;
  onFind?: (r: { current: number; total: number; state: number }) => void;
  renderOverlay: (page: number, geom: PageGeom) => ReactNode;
  onBackgroundTap?: () => void;
}

const SCROLL = { vertical: 0, horizontal: 1, wrapped: 2, page: 3 } as const;
const SPREAD = { none: 0, odd: 1, even: 2 } as const;

function makeGeom(pageView: any): PageGeom | null {
  const pdfPage = pageView?.pdfPage;
  if (!pdfPage) return null;
  const vp = pdfPage.getViewport({ scale: 1, rotation: pageView.viewport.rotation });
  return {
    page: pageView.id,
    width: vp.width,
    height: vp.height,
    rotation: vp.rotation,
    toView: (p) => vp.convertToViewportPoint(p[0], p[1]) as Pt,
    toPdf: (x, y) => vp.convertToPdfPoint(x, y) as Pt,
  };
}

const Viewer = forwardRef<ViewerHandle, Props>(function Viewer(p, ref) {
  const containerRef = useRef<HTMLDivElement>(null);
  const viewerElRef = useRef<HTMLDivElement>(null);
  const v = useRef<{ viewer: any; bus: any; link: any; find: any; doc: PDFDocumentProxy | null }>({
    viewer: null,
    bus: null,
    link: null,
    find: null,
    doc: null,
  });
  const hostEls = useRef(new Map<number, HTMLDivElement>());
  const [hosts, setHosts] = useState<Record<number, PageGeom>>({});
  const cb = useRef(p);
  cb.current = p;

  // ------------------------------------------------------------------ pdf.js einrichten
  useEffect(() => {
    let disposed = false;
    const container = containerRef.current!;
    (async () => {
      const { PDFViewer, EventBus, PDFLinkService, PDFFindController } = await loadViewer();
      if (disposed) return;
      const bus = new EventBus();
      const link = new PDFLinkService({ eventBus: bus, externalLinkTarget: 2 });
      const find = new PDFFindController({ eventBus: bus, linkService: link });
      const mobile = matchMedia("(pointer: coarse)").matches;
      const viewer = new PDFViewer({
        container,
        viewer: viewerElRef.current!,
        eventBus: bus,
        linkService: link,
        findController: find,
        textLayerMode: 1,
        annotationMode: pdfjsLib.AnnotationMode.ENABLE_FORMS,
        annotationEditorMode: pdfjsLib.AnnotationEditorType.DISABLE,
        maxCanvasPixels: mobile ? 2 ** 22 : 2 ** 25,
        removePageBorders: false,
        enableHWA: true,
      } as any);
      link.setViewer(viewer);
      v.current = { viewer, bus, link, find, doc: null };

      const mount = (pageView: any) => {
        const n = pageView.id as number;
        let el = hostEls.current.get(n);
        if (!el) {
          el = document.createElement("div");
          el.className = "lp-overlay-host";
          hostEls.current.set(n, el);
        }
        if (el.parentElement !== pageView.div) pageView.div.appendChild(el);
        const g = makeGeom(pageView);
        if (g)
          setHosts((h) => {
            const old = h[n];
            if (old && old.width === g.width && old.height === g.height && old.rotation === g.rotation) return h;
            return { ...h, [n]: g };
          });
      };
      bus.on("pagerender", (e: any) => mount(e.source));
      bus.on("pagerendered", (e: any) => mount(e.source));
      bus.on("pagechanging", (e: any) => cb.current.onPage(e.pageNumber, viewer.pagesCount));
      bus.on("scalechanging", (e: any) => cb.current.onScale(e.scale));
      bus.on("rotationchanging", () => setHosts({}));
      bus.on("updatefindmatchescount", (e: any) =>
        cb.current.onFind?.({ current: e.matchesCount.current, total: e.matchesCount.total, state: -1 }),
      );
      bus.on("updatefindcontrolstate", (e: any) =>
        cb.current.onFind?.({ current: e.matchesCount?.current ?? 0, total: e.matchesCount?.total ?? 0, state: e.state }),
      );
      bus.on("pagesinit", () => {
        viewer.currentScaleValue = innerWidth < 760 ? "page-width" : "auto";
        if (cb.current.startPage && cb.current.startPage > 1) viewer.currentPageNumber = Math.min(cb.current.startPage, viewer.pagesCount);
        cb.current.onPage(viewer.currentPageNumber, viewer.pagesCount);
      });

      try {
        const doc = await openPdf(p.data, p.password);
        if (disposed) {
          doc.destroy();
          return;
        }
        v.current.doc = doc;
        viewer.setDocument(doc);
        link.setDocument(doc, null);
        find.setDocument(doc);
        viewer.scrollMode = SCROLL[cb.current.scrollMode];
        viewer.spreadMode = SPREAD[cb.current.spread];
        cb.current.onReady(doc);
      } catch (e) {
        if (e instanceof PasswordNeeded) cb.current.onPassword(e.wrong);
        else cb.current.onError(e as Error);
      }
    })();
    return () => {
      disposed = true;
      const { viewer, doc } = v.current;
      try {
        viewer?.setDocument(null);
      } catch {}
      doc?.destroy();
      hostEls.current.clear();
      setHosts({});
    };
    // Neu laden, wenn sich die Daten ändern
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [p.data, p.password]);

  useEffect(() => {
    const viewer = v.current.viewer;
    if (!viewer?.pdfDocument) return;
    viewer.scrollMode = SCROLL[p.scrollMode];
    viewer.spreadMode = SPREAD[p.spread];
  }, [p.scrollMode, p.spread]);

  // ------------------------------------------------------------------ Zoom: Pinch, Strg+Rad, Doppeltipp
  useEffect(() => {
    const el = containerRef.current!;
    let pinch: { d: number } | null = null;
    let lastTap = 0;
    let acc = 1;
    // Kleine Faktoren aufsammeln (pdf.js rundet den Maßstab auf 2 Nachkommastellen)
    const apply = (factor: number, origin: [number, number], drawingDelay: number) => {
      const viewer = v.current.viewer;
      if (!viewer?.pdfDocument) return;
      acc *= factor;
      const cur = viewer.currentScale;
      const next = Math.round(cur * acc * 100) / 100;
      if (next === cur) return;
      viewer.updateScale({ scaleFactor: acc, origin, drawingDelay });
      acc = 1;
    };
    const dist = (t: TouchList) => Math.hypot(t[0].clientX - t[1].clientX, t[0].clientY - t[1].clientY);
    const center = (t: TouchList): [number, number] => [(t[0].clientX + t[1].clientX) / 2, (t[0].clientY + t[1].clientY) / 2];

    const onTouchStart = (e: TouchEvent) => {
      if (e.touches.length === 2) {
        pinch = { d: dist(e.touches) };
        acc = 1;
        e.preventDefault();
      } else if (e.touches.length === 1) {
        const now = Date.now();
        if (now - lastTap < 280 && !(e.target as HTMLElement).closest(".lp-overlay-host *")) {
          const viewer = v.current.viewer;
          if (viewer) {
            const factor = viewer.currentScale > 1.6 ? 1 / viewer.currentScale : 2;
            viewer.updateScale({ scaleFactor: factor, origin: [e.touches[0].clientX, e.touches[0].clientY], drawingDelay: 300 });
          }
          lastTap = 0;
        } else lastTap = now;
      }
    };
    const onTouchMove = (e: TouchEvent) => {
      if (!pinch || e.touches.length !== 2) return;
      e.preventDefault();
      const d = dist(e.touches);
      const factor = d / pinch.d;
      pinch.d = d;
      apply(factor, center(e.touches), 350);
    };
    const onTouchEnd = (e: TouchEvent) => {
      if (e.touches.length < 2) pinch = null;
    };
    const onWheel = (e: WheelEvent) => {
      if (!(e.ctrlKey || e.metaKey)) return;
      e.preventDefault();
      const factor = Math.exp(-e.deltaY / (e.deltaMode === 1 ? 20 : 300));
      apply(factor, [e.clientX, e.clientY], 250);
    };
    el.addEventListener("touchstart", onTouchStart, { passive: false });
    el.addEventListener("touchmove", onTouchMove, { passive: false });
    el.addEventListener("touchend", onTouchEnd);
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => {
      el.removeEventListener("touchstart", onTouchStart);
      el.removeEventListener("touchmove", onTouchMove);
      el.removeEventListener("touchend", onTouchEnd);
      el.removeEventListener("wheel", onWheel);
    };
  }, []);

  // ------------------------------------------------------------------ Hand-Werkzeug (mit der Maus ziehen)
  useEffect(() => {
    if (!p.panMode) return;
    const el = containerRef.current!;
    let drag: { x: number; y: number; l: number; t: number } | null = null;
    const down = (e: PointerEvent) => {
      if (e.pointerType !== "mouse" || e.button !== 0) return;
      drag = { x: e.clientX, y: e.clientY, l: el.scrollLeft, t: el.scrollTop };
      el.classList.add("lp-grabbing");
    };
    const move = (e: PointerEvent) => {
      if (!drag) return;
      el.scrollLeft = drag.l - (e.clientX - drag.x);
      el.scrollTop = drag.t - (e.clientY - drag.y);
    };
    const up = () => {
      drag = null;
      el.classList.remove("lp-grabbing");
    };
    el.addEventListener("pointerdown", down);
    addEventListener("pointermove", move);
    addEventListener("pointerup", up);
    return () => {
      el.removeEventListener("pointerdown", down);
      removeEventListener("pointermove", move);
      removeEventListener("pointerup", up);
    };
  }, [p.panMode]);

  // ------------------------------------------------------------------ Imperative API
  useImperativeHandle(ref, () => ({
    doc: () => v.current.doc,
    goTo: (n) => {
      const viewer = v.current.viewer;
      if (viewer?.pdfDocument) viewer.currentPageNumber = Math.max(1, Math.min(n, viewer.pagesCount));
    },
    goToDest: (dest) => v.current.link?.goToDestination(dest),
    zoom: (factor) => {
      const el = containerRef.current!;
      const r = el.getBoundingClientRect();
      v.current.viewer?.updateScale({ scaleFactor: factor, origin: [r.left + r.width / 2, r.top + r.height / 3], drawingDelay: 200 });
    },
    setZoom: (val) => {
      const viewer = v.current.viewer;
      if (!viewer?.pdfDocument) return;
      if (typeof val === "number") viewer.currentScale = val;
      else viewer.currentScaleValue = val;
    },
    rotateView: (d) => {
      const viewer = v.current.viewer;
      if (viewer?.pdfDocument) viewer.pagesRotation = (viewer.pagesRotation + d + 360) % 360;
    },
    find: (query, o = {}) => {
      v.current.bus?.dispatch("find", {
        source: null,
        type: o.again ? "again" : "",
        query,
        caseSensitive: !!o.caseSensitive,
        entireWord: !!o.wholeWord,
        highlightAll: true,
        findPrevious: !!o.prev,
        matchDiacritics: false,
      });
    },
    clearFind: () => v.current.bus?.dispatch("findbarclose", { source: null }),
    formBytes: async () => {
      const doc = v.current.doc;
      if (!doc || !(doc.annotationStorage as any).size) return null;
      return await doc.saveDocument();
    },
    geom: (n) => makeGeom(v.current.viewer?.getPageView(n - 1)),
    pageAtClient: (x, y) => {
      const viewer = v.current.viewer;
      if (!viewer) return null;
      for (let i = 0; i < viewer.pagesCount; i++) {
        const pv = viewer.getPageView(i);
        const r = pv.div.getBoundingClientRect();
        if (x >= r.left && x <= r.right && y >= r.top && y <= r.bottom) {
          const g = makeGeom(pv);
          if (g) return { page: i + 1, geom: g, rect: r };
        }
      }
      return null;
    },
    scrollBy: (dx, dy) => containerRef.current?.scrollBy(dx, dy),
    container: () => containerRef.current,
    currentPage: () => v.current.viewer?.currentPageNumber ?? 1,
  }));

  return (
    <div
      ref={containerRef}
      className={`lp-viewer-container reading-${p.reading} ${p.panMode ? "lp-pan" : ""}`}
      onClick={(e) => {
        if ((e.target as HTMLElement).classList.contains("lp-viewer-container") || (e.target as HTMLElement).classList.contains("pdfViewer"))
          p.onBackgroundTap?.();
      }}
    >
      <div ref={viewerElRef} className="pdfViewer" />
      {Object.entries(hosts).map(([n, g]) => {
        const el = hostEls.current.get(+n);
        return el ? createPortal(p.renderOverlay(+n, g), el, "ov" + n) : null;
      })}
    </div>
  );
});

export default Viewer;
