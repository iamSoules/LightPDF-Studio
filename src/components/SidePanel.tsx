import { memo, useEffect, useRef, useState } from "react";
import { ChevronRight, Search, ChevronUp, ChevronDown, Highlighter, PenLine, Square, Circle, Minus, MoveUpRight, Type, StickyNote, Image as ImageIcon, EyeOff, Underline, Strikethrough, Trash2, MessageSquare, X, Bookmark } from "lucide-react";
import type { PDFDocumentProxy } from "../lib/pdfjs";
import { renderPageToCanvas } from "../lib/pdfjs";
import type { Annotation } from "../types";
import { cx, Empty } from "./ui";
import { useT } from "../i18n";

export type PanelKind = "thumbs" | "outline" | "comments" | "search";

// ------------------------------------------------------------------ Miniaturen (werden erst bei Sichtbarkeit gerendert)
const thumbCache = new WeakMap<PDFDocumentProxy, Map<string, string>>();

export const Thumb = memo(function Thumb({ doc, page, width, rotation = 0, version = 0 }: { doc: PDFDocumentProxy; page: number; width: number; rotation?: number; version?: number }) {
  const ref = useRef<HTMLDivElement>(null);
  const key = `${page}:${width}:${rotation}:${version}`;
  const [src, setSrc] = useState<string | null>(() => thumbCache.get(doc)?.get(key) ?? null);
  const [ratio, setRatio] = useState(1.414);
  useEffect(() => {
    let alive = true;
    doc.getPage(page).then((p) => {
      const vp = p.getViewport({ scale: 1, rotation: (p.rotate + rotation) % 360 });
      if (alive) setRatio(vp.height / vp.width);
    });
    return () => {
      alive = false;
    };
  }, [doc, page, rotation]);
  useEffect(() => {
    if (src) return;
    let alive = true;
    const io = new IntersectionObserver(
      async ([e]) => {
        if (!e.isIntersecting) return;
        io.disconnect();
        try {
          const c = await renderPageToCanvas(doc, page, width, { rotation, dpr: 1.5 });
          const url = c.toDataURL("image/jpeg", 0.8);
          let m = thumbCache.get(doc);
          if (!m) thumbCache.set(doc, (m = new Map()));
          m.set(key, url);
          if (alive) setSrc(url);
        } catch {
          /* Dokument geschlossen */
        }
      },
      { rootMargin: "300px" },
    );
    if (ref.current) io.observe(ref.current);
    return () => {
      alive = false;
      io.disconnect();
    };
  }, [doc, page, width, rotation, key, src]);
  return (
    <div ref={ref} className="thumb-img" style={{ aspectRatio: `1 / ${ratio}` }}>
      {src ? <img src={src} alt="" draggable={false} /> : <div className="thumb-skeleton" />}
    </div>
  );
});

function Thumbs({ doc, page, total, onGo, annotPages }: { doc: PDFDocumentProxy; page: number; total: number; onGo: (p: number) => void; annotPages: Set<number> }) {
  const listRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    listRef.current?.querySelector(`[data-page="${page}"]`)?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [page]);
  return (
    <div className="thumbs" ref={listRef}>
      {Array.from({ length: total }, (_, i) => i + 1).map((n) => (
        <button key={n} data-page={n} className={cx("thumb", n === page && "is-active")} onClick={() => onGo(n)}>
          <Thumb doc={doc} page={n} width={130} />
          <span className="thumb-no">
            {n}
            {annotPages.has(n) && <i className="thumb-dot" />}
          </span>
        </button>
      ))}
    </div>
  );
}

// ------------------------------------------------------------------ Inhaltsverzeichnis
interface OutlineNode {
  title: string;
  dest: unknown;
  url?: string | null;
  items: OutlineNode[];
  bold?: boolean;
}

function OutlineTree({ nodes, onDest, depth = 0 }: { nodes: OutlineNode[]; onDest: (d: unknown, url?: string | null) => void; depth?: number }) {
  return (
    <ul className="outline">
      {nodes.map((n, i) => (
        <OutlineItem key={i} n={n} onDest={onDest} depth={depth} />
      ))}
    </ul>
  );
}

function OutlineItem({ n, onDest, depth }: { n: OutlineNode; onDest: (d: unknown, url?: string | null) => void; depth: number }) {
  const [open, setOpen] = useState(depth < 1);
  return (
    <li>
      <div className="outline-row" style={{ paddingLeft: 6 + depth * 14 }}>
        {n.items.length ? (
          <button className={cx("outline-toggle", open && "open")} onClick={() => setOpen(!open)} aria-label="+">
            <ChevronRight size={14} />
          </button>
        ) : (
          <span className="outline-toggle" />
        )}
        <button className={cx("outline-title", n.bold && "bold")} onClick={() => onDest(n.dest, n.url)}>
          {n.title}
        </button>
      </div>
      {open && n.items.length > 0 && <OutlineTree nodes={n.items} onDest={onDest} depth={depth + 1} />}
    </li>
  );
}

function Outline({ doc, onDest }: { doc: PDFDocumentProxy; onDest: (d: unknown, url?: string | null) => void }) {
  const t = useT();
  const [nodes, setNodes] = useState<OutlineNode[] | null>(null);
  useEffect(() => {
    doc.getOutline().then((o) => setNodes((o as OutlineNode[]) ?? []));
  }, [doc]);
  if (!nodes) return null;
  if (!nodes.length) return <Empty icon={<Bookmark size={22} />} title={t("Kein Inhaltsverzeichnis")} text={t("Dieses Dokument enthält keine Lesezeichen.")} />;
  return <OutlineTree nodes={nodes} onDest={onDest} />;
}

// ------------------------------------------------------------------ Kommentare
export const ANN_ICON: Record<string, any> = {
  highlight: Highlighter,
  underline: Underline,
  strike: Strikethrough,
  ink: PenLine,
  rect: Square,
  ellipse: Circle,
  line: Minus,
  arrow: MoveUpRight,
  text: Type,
  note: StickyNote,
  image: ImageIcon,
  redact: EyeOff,
};

function Comments({ annots, selectedId, onPick, onDelete, onNote }: { annots: Annotation[]; selectedId: string | null; onPick: (a: Annotation) => void; onDelete: (id: string) => void; onNote: (a: Annotation, note: string) => void }) {
  const t = useT();
  const [filter, setFilter] = useState("");
  const names: Record<string, string> = {
    highlight: t("Markierung"),
    underline: t("Unterstrichen"),
    strike: t("Durchgestrichen"),
    ink: t("Freihand"),
    rect: t("Rechteck"),
    ellipse: t("Ellipse"),
    line: t("Linie"),
    arrow: t("Pfeil"),
    text: t("Textfeld"),
    note: t("Notiz"),
    image: t("Bild / Unterschrift"),
    redact: t("Schwärzung (ausstehend)"),
  };
  const list = annots
    .filter((a) => !filter || a.type === filter)
    .sort((a, b) => a.page - b.page || a.createdAt - b.createdAt);
  if (!annots.length) return <Empty icon={<MessageSquare size={22} />} title={t("Noch keine Kommentare")} text={t("Markiere Text oder nutze die Werkzeuge unter „Kommentieren“.")} />;
  const types = [...new Set(annots.map((a) => a.type))];
  let lastPage = 0;
  return (
    <div className="comments">
      {types.length > 1 && (
        <div className="chips chips-small">
          <button className={cx("chip", !filter && "is-active")} onClick={() => setFilter("")}>
            {t("Alle")} {annots.length}
          </button>
          {types.map((ty) => {
            const I = ANN_ICON[ty];
            return (
              <button key={ty} className={cx("chip", filter === ty && "is-active")} onClick={() => setFilter(ty)} title={names[ty]}>
                <I size={13} /> {annots.filter((a) => a.type === ty).length}
              </button>
            );
          })}
        </div>
      )}
      {list.map((a) => {
        const I = ANN_ICON[a.type];
        const header = a.page !== lastPage ? <div className="comments-page">{t("Seite {0}", a.page)}</div> : null;
        lastPage = a.page;
        const body = a.type === "note" || a.type === "text" ? a.text : "text" in a ? a.text : undefined;
        return (
          <div key={a.id}>
            {header}
            <div className={cx("comment", a.id === selectedId && "is-active")} onClick={() => onPick(a)}>
              <span className="comment-icon" style={{ color: a.type === "redact" ? "#333" : a.color }}>
                <I size={15} />
              </span>
              <div className="comment-main">
                <div className="comment-head">
                  <b>{names[a.type]}</b>
                  <span>{new Date(a.createdAt).toLocaleDateString()}</span>
                </div>
                {body && <div className="comment-quote">{body}</div>}
                {a.type !== "note" && a.type !== "text" && (
                  <textarea
                    className="comment-note"
                    rows={1}
                    placeholder={t("Kommentar hinzufügen …")}
                    defaultValue={a.note ?? ""}
                    onClick={(e) => e.stopPropagation()}
                    onBlur={(e) => e.target.value !== (a.note ?? "") && onNote(a, e.target.value)}
                    onKeyDown={(e) => e.stopPropagation()}
                  />
                )}
              </div>
              <button
                className="comment-del"
                aria-label={t("Löschen")}
                onClick={(e) => {
                  e.stopPropagation();
                  onDelete(a.id);
                }}
              >
                <Trash2 size={14} />
              </button>
            </div>
          </div>
        );
      })}
    </div>
  );
}

// ------------------------------------------------------------------ Suche
function SearchPanel({ onFind, result, onClose }: { onFind: (q: string, o: { again?: boolean; prev?: boolean; caseSensitive?: boolean; wholeWord?: boolean }) => void; result: { current: number; total: number; state: number }; onClose: () => void }) {
  const t = useT();
  const [q, setQ] = useState("");
  const [cs, setCs] = useState(false);
  const [ww, setWw] = useState(false);
  const inp = useRef<HTMLInputElement>(null);
  useEffect(() => inp.current?.focus(), []);
  useEffect(() => {
    const id = setTimeout(() => q && onFind(q, { caseSensitive: cs, wholeWord: ww }), 220);
    return () => clearTimeout(id);
  }, [q, cs, ww]);
  return (
    <div className="search-panel">
      <div className="search-box">
        <Search size={16} />
        <input
          ref={inp}
          value={q}
          placeholder={t("Im Dokument suchen …")}
          onChange={(e) => setQ(e.target.value)}
          onKeyDown={(e) => {
            e.stopPropagation();
            if (e.key === "Enter") onFind(q, { again: true, prev: e.shiftKey, caseSensitive: cs, wholeWord: ww });
            if (e.key === "Escape") onClose();
          }}
        />
        {q && (
          <button className="iconbtn iconbtn-sm" onClick={() => setQ("")} aria-label={t("Leeren")}>
            <X size={14} />
          </button>
        )}
      </div>
      <div className="search-row">
        <span className="muted">
          {!q ? t("Suchbegriff eingeben") : result.state === 1 ? t("Keine Treffer") : result.total ? t("{0} von {1}", result.current, result.total) : t("Suche …")}
        </span>
        <div className="row-gap">
          <button className="iconbtn iconbtn-sm" disabled={!result.total} onClick={() => onFind(q, { again: true, prev: true, caseSensitive: cs, wholeWord: ww })} aria-label={t("Vorheriger Treffer")}>
            <ChevronUp size={16} />
          </button>
          <button className="iconbtn iconbtn-sm" disabled={!result.total} onClick={() => onFind(q, { again: true, caseSensitive: cs, wholeWord: ww })} aria-label={t("Nächster Treffer")}>
            <ChevronDown size={16} />
          </button>
        </div>
      </div>
      <div className="search-opts">
        <label>
          <input type="checkbox" checked={cs} onChange={(e) => setCs(e.target.checked)} /> {t("Groß/Klein beachten")}
        </label>
        <label>
          <input type="checkbox" checked={ww} onChange={(e) => setWw(e.target.checked)} /> {t("Ganze Wörter")}
        </label>
      </div>
    </div>
  );
}

// ------------------------------------------------------------------ Panel-Gerüst
export default function SidePanel(props: {
  kind: PanelKind;
  setKind: (k: PanelKind | null) => void;
  doc: PDFDocumentProxy | null;
  page: number;
  total: number;
  annots: Annotation[];
  selectedId: string | null;
  onGo: (p: number) => void;
  onDest: (d: unknown, url?: string | null) => void;
  onPick: (a: Annotation) => void;
  onDelete: (id: string) => void;
  onNote: (a: Annotation, note: string) => void;
  onFind: (q: string, o: any) => void;
  findResult: { current: number; total: number; state: number };
  onCloseSearch: () => void;
}) {
  const t = useT();
  const tabs: { k: PanelKind; label: string }[] = [
    { k: "thumbs", label: t("Seiten") },
    { k: "outline", label: t("Inhalt") },
    { k: "comments", label: t("Kommentare") },
    { k: "search", label: t("Suche") },
  ];
  const annotPages = new Set(props.annots.map((a) => a.page));
  return (
    <aside className="sidepanel">
      <div className="sidepanel-tabs">
        {tabs.map((tb) => (
          <button key={tb.k} className={cx(props.kind === tb.k && "is-active")} onClick={() => props.setKind(tb.k)}>
            {tb.label}
            {tb.k === "comments" && props.annots.length > 0 && <span className="count">{props.annots.length}</span>}
          </button>
        ))}
        <button className="iconbtn iconbtn-sm sidepanel-close" onClick={() => props.setKind(null)} aria-label={t("Schließen")}>
          <X size={16} />
        </button>
      </div>
      <div className="sidepanel-body">
        {props.doc && props.kind === "thumbs" && <Thumbs doc={props.doc} page={props.page} total={props.total} onGo={props.onGo} annotPages={annotPages} />}
        {props.doc && props.kind === "outline" && <Outline doc={props.doc} onDest={props.onDest} />}
        {props.kind === "comments" && <Comments annots={props.annots} selectedId={props.selectedId} onPick={props.onPick} onDelete={props.onDelete} onNote={props.onNote} />}
        {props.kind === "search" && <SearchPanel onFind={props.onFind} result={props.findResult} onClose={props.onCloseSearch} />}
      </div>
    </aside>
  );
}
