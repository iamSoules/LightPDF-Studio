import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import { Trash2, MessageSquare, Copy } from "lucide-react";
import type { Annotation, Pt, Rect, Tool } from "../types";
import type { PageGeom } from "./Viewer";
import { arrowHead } from "../lib/annotio";
import { uid } from "../lib/db";
import { cx, PALETTE } from "./ui";
import { tr } from "../i18n";

export interface ToolOpts {
  color: string;
  width: number;
  opacity: number;
  fontSize: number;
  fill: string | null;
  presetText?: string; // ✓ ✗ ● Datum
  pendingImage?: { src: string; kind: "signature" | "image" | "stamp"; aspect: number } | null;
}

interface Props {
  page: number;
  geom: PageGeom;
  annots: Annotation[];
  tool: Tool;
  opts: ToolOpts;
  author: string;
  selectedId: string | null;
  editingId: string | null;
  onSelect: (id: string | null) => void;
  onEdit: (id: string | null) => void;
  onChange: (a: Annotation, commit: boolean) => void;
  onAdd: (a: Annotation) => void;
  onDelete: (id: string) => void;
  onPlaced: () => void;
}

type VRect = [number, number, number, number]; // Overlay-Koordinaten, normalisiert

const DRAW_TOOLS: Tool[] = ["ink", "highlighter", "rect", "ellipse", "line", "arrow", "redact", "text", "note", "eraser", "signature", "image", "stamp"];

function normV(a: Pt, b: Pt): VRect {
  return [Math.min(a[0], b[0]), Math.min(a[1], b[1]), Math.max(a[0], b[0]), Math.max(a[1], b[1])];
}

export function pdfRectToView(g: PageGeom, r: Rect): VRect {
  return normV(g.toView([r[0], r[1]]), g.toView([r[2], r[3]]));
}

export function viewRectToPdf(g: PageGeom, r: VRect): Rect {
  const a = g.toPdf(r[0], r[1]);
  const b = g.toPdf(r[2], r[3]);
  return [Math.min(a[0], b[0]), Math.min(a[1], b[1]), Math.max(a[0], b[0]), Math.max(a[1], b[1])];
}

/** Begrenzungsrechteck eines Kommentars in Overlay-Koordinaten */
function viewBounds(g: PageGeom, a: Annotation): VRect {
  switch (a.type) {
    case "highlight":
    case "underline":
    case "strike": {
      const rs = a.rects.map((r) => pdfRectToView(g, r));
      return [Math.min(...rs.map((r) => r[0])), Math.min(...rs.map((r) => r[1])), Math.max(...rs.map((r) => r[2])), Math.max(...rs.map((r) => r[3]))];
    }
    case "ink": {
      const pts = a.paths.flat().map((p) => g.toView(p));
      const w = a.width / 2;
      return [Math.min(...pts.map((p) => p[0])) - w, Math.min(...pts.map((p) => p[1])) - w, Math.max(...pts.map((p) => p[0])) + w, Math.max(...pts.map((p) => p[1])) + w];
    }
    case "line":
    case "arrow":
      return normV(g.toView(a.from), g.toView(a.to));
    case "note": {
      const [x, y] = g.toView(a.at);
      return [x, y, x + 20, y + 20];
    }
    default:
      return pdfRectToView(g, a.rect);
  }
}

/** Kommentar um einen Overlay-Vektor verschieben */
function moveBy(g: PageGeom, a: Annotation, dx: number, dy: number): Annotation {
  const mv = (p: Pt): Pt => {
    const v = g.toView(p);
    return g.toPdf(v[0] + dx, v[1] + dy);
  };
  const mvR = (r: Rect): Rect => {
    const v = pdfRectToView(g, r);
    return viewRectToPdf(g, [v[0] + dx, v[1] + dy, v[2] + dx, v[3] + dy]);
  };
  switch (a.type) {
    case "highlight":
    case "underline":
    case "strike":
      return { ...a, rects: a.rects.map(mvR) };
    case "ink":
      return { ...a, paths: a.paths.map((p) => p.map(mv)) };
    case "line":
    case "arrow":
      return { ...a, from: mv(a.from), to: mv(a.to) };
    case "note":
      return { ...a, at: mv(a.at) };
    default:
      return { ...a, rect: mvR(a.rect) };
  }
}

const pct = (v: number, total: number) => `${(v / total) * 100}%`;

export default function AnnotationOverlay(p: Props) {
  const { geom: g, tool, opts } = p;
  const root = useRef<HTMLDivElement>(null);
  const [draft, setDraft] = useState<Annotation | null>(null);
  const drag = useRef<{ kind: string; start: Pt; orig: Annotation; handle?: string } | null>(null);
  const drawing = DRAW_TOOLS.includes(tool);
  const W = g.width,
    H = g.height;

  // Pointer → Overlay-Koordinaten
  const toV = (e: { clientX: number; clientY: number }): Pt => {
    const r = root.current!.getBoundingClientRect();
    return [((e.clientX - r.left) / r.width) * W, ((e.clientY - r.top) / r.height) * H];
  };
  const base = () => ({ id: uid(), page: p.page, color: opts.color, opacity: opts.opacity, createdAt: Date.now(), author: p.author || undefined });

  // ------------------------------------------------------------------ Zeichnen
  const onPointerDown = (e: React.PointerEvent) => {
    if (!drawing || e.button !== 0) return;
    const v = toV(e);
    const pt = g.toPdf(...v);
    if (tool === "eraser") {
      erase(e);
      (e.target as HTMLElement).setPointerCapture?.(e.pointerId);
      drag.current = { kind: "erase", start: v, orig: null as any };
      return;
    }
    e.preventDefault();
    if (tool === "text") {
      const preset = opts.presetText;
      const size = preset ? opts.fontSize * 1.2 : opts.fontSize;
      const w = preset ? size * Math.max(1.2, preset.length * 0.62) + 4 : Math.max(120, size * 14);
      const h = size * 1.45;
      const r = viewRectToPdf(g, [v[0], v[1] - h / 2, v[0] + w, v[1] + h / 2]);
      const a: Annotation = { ...base(), type: "text", rect: r, text: preset ?? "", fontSize: size, bold: !!preset && preset.length === 1 };
      p.onAdd(a);
      if (!preset) p.onEdit(a.id);
      p.onPlaced();
      return;
    }
    if (tool === "note") {
      const a: Annotation = { ...base(), type: "note", at: g.toPdf(v[0] - 4, v[1] - 4), text: "" };
      p.onAdd(a);
      p.onEdit(a.id);
      p.onPlaced();
      return;
    }
    if (tool === "signature" || tool === "image" || tool === "stamp") {
      const img = opts.pendingImage;
      if (!img) return;
      const w = img.kind === "signature" ? 170 : img.kind === "stamp" ? 150 : 220;
      const h = w / img.aspect;
      const a: Annotation = { ...base(), opacity: 1, type: "image", rect: viewRectToPdf(g, [v[0] - w / 2, v[1] - h / 2, v[0] + w / 2, v[1] + h / 2]), src: img.src, kind: img.kind };
      p.onAdd(a);
      p.onSelect(a.id);
      p.onPlaced();
      return;
    }
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    let d: Annotation | null = null;
    if (tool === "ink" || tool === "highlighter") {
      const hl = tool === "highlighter";
      d = { ...base(), type: "ink", paths: [[pt]], width: hl ? Math.max(8, opts.width * 4) : opts.width, highlighter: hl, opacity: hl ? 0.45 : opts.opacity };
    } else if (tool === "rect" || tool === "ellipse") d = { ...base(), type: tool, rect: [pt[0], pt[1], pt[0], pt[1]], width: opts.width, fill: opts.fill };
    else if (tool === "line" || tool === "arrow") d = { ...base(), type: tool, from: pt, to: pt, width: opts.width };
    else if (tool === "redact") d = { ...base(), color: "#000000", opacity: 1, type: "redact", rect: [pt[0], pt[1], pt[0], pt[1]] };
    drag.current = { kind: "draw", start: v, orig: d! };
    setDraft(d);
  };

  const onPointerMove = (e: React.PointerEvent) => {
    const dr = drag.current;
    if (!dr) return;
    if (dr.kind === "erase") return erase(e);
    if (dr.kind !== "draw" || !draft) return;
    const v = toV(e);
    const pt = g.toPdf(...v);
    if (draft.type === "ink") {
      const path = draft.paths[0];
      const last = g.toView(path[path.length - 1]);
      if (Math.hypot(v[0] - last[0], v[1] - last[1]) < 1.2) return;
      // Gerade Linien beim Textmarker mit Umschalt
      const pts = e.shiftKey && draft.highlighter ? [path[0], pt] : [...path, pt];
      setDraft({ ...draft, paths: [pts] });
    } else if (draft.type === "rect" || draft.type === "ellipse" || draft.type === "redact") {
      let end = v;
      if (e.shiftKey) {
        const s = Math.max(Math.abs(v[0] - dr.start[0]), Math.abs(v[1] - dr.start[1]));
        end = [dr.start[0] + Math.sign(v[0] - dr.start[0]) * s, dr.start[1] + Math.sign(v[1] - dr.start[1]) * s];
      }
      setDraft({ ...draft, rect: viewRectToPdf(g, normV(dr.start, end)) });
    } else if (draft.type === "line" || draft.type === "arrow") {
      let end = v;
      if (e.shiftKey) {
        const ang = Math.round(Math.atan2(v[1] - dr.start[1], v[0] - dr.start[0]) / (Math.PI / 4)) * (Math.PI / 4);
        const len = Math.hypot(v[0] - dr.start[0], v[1] - dr.start[1]);
        end = [dr.start[0] + Math.cos(ang) * len, dr.start[1] + Math.sin(ang) * len];
      }
      setDraft({ ...draft, to: g.toPdf(...end) });
    }
  };

  const onPointerUp = () => {
    const dr = drag.current;
    drag.current = null;
    if (!dr || dr.kind !== "draw" || !draft) return;
    setDraft(null);
    const vb = viewBounds(g, draft);
    const big = vb[2] - vb[0] > 3 || vb[3] - vb[1] > 3;
    if (draft.type === "ink" || big) p.onAdd(draft);
  };

  const erase = (e: { clientX: number; clientY: number }) => {
    for (const el of document.elementsFromPoint(e.clientX, e.clientY)) {
      const id = (el as HTMLElement).dataset?.ann;
      if (id) {
        p.onDelete(id);
        return;
      }
    }
  };

  // ------------------------------------------------------------------ Auswählen, verschieben, Größe ändern
  const startMove = (e: React.PointerEvent, a: Annotation, handle?: string) => {
    if (e.button !== 0) return;
    e.stopPropagation();
    e.preventDefault();
    p.onSelect(a.id);
    drag.current = { kind: handle ? "resize" : "move", start: toV(e), orig: a, handle };
    const move = (ev: PointerEvent) => {
      const dr = drag.current;
      if (!dr) return;
      const v = toV(ev);
      const dx = v[0] - dr.start[0],
        dy = v[1] - dr.start[1];
      if (dr.kind === "move") p.onChange(moveBy(g, dr.orig, dx, dy), false);
      else p.onChange(resize(dr.orig, dr.handle!, v, ev.shiftKey), false);
    };
    const up = (ev: PointerEvent) => {
      removeEventListener("pointermove", move);
      removeEventListener("pointerup", up);
      const dr = drag.current;
      drag.current = null;
      if (!dr) return;
      const v = toV(ev);
      const dx = v[0] - dr.start[0],
        dy = v[1] - dr.start[1];
      if (Math.abs(dx) < 0.5 && Math.abs(dy) < 0.5) {
        if (a.type === "text" || a.type === "note") p.onEdit(a.id);
        return;
      }
      p.onChange(dr.kind === "move" ? moveBy(g, dr.orig, dx, dy) : resize(dr.orig, dr.handle!, v, ev.shiftKey), true);
    };
    addEventListener("pointermove", move);
    addEventListener("pointerup", up);
  };

  const resize = (a: Annotation, handle: string, v: Pt, keepRatio: boolean): Annotation => {
    if (a.type === "line" || a.type === "arrow") {
      const pt = g.toPdf(...v);
      return handle === "from" ? { ...a, from: pt } : { ...a, to: pt };
    }
    if (!("rect" in a)) return a;
    const r = pdfRectToView(g, a.rect);
    let [x1, y1, x2, y2] = r;
    if (handle.includes("w")) x1 = Math.min(v[0], x2 - 4);
    if (handle.includes("e")) x2 = Math.max(v[0], x1 + 4);
    if (handle.includes("n")) y1 = Math.min(v[1], y2 - 4);
    if (handle.includes("s")) y2 = Math.max(v[1], y1 + 4);
    if ((keepRatio || a.type === "image") && handle.length === 2) {
      const ratio = (r[2] - r[0]) / (r[3] - r[1]);
      const w = x2 - x1;
      const h = w / ratio;
      if (handle.includes("n")) y1 = y2 - h;
      else y2 = y1 + h;
    }
    const rect = viewRectToPdf(g, [x1, y1, x2, y2]);
    if (a.type === "text" && handle.length === 2 && keepRatio) return { ...a, rect, fontSize: a.fontSize * ((x2 - x1) / (r[2] - r[0])) };
    return { ...a, rect };
  };

  // Entf-Taste
  useEffect(() => {
    if (!p.selectedId || !p.annots.some((a) => a.id === p.selectedId)) return;
    const k = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement;
      if (t.tagName === "TEXTAREA" || t.tagName === "INPUT") return;
      if (e.key === "Delete" || e.key === "Backspace") {
        e.preventDefault();
        p.onDelete(p.selectedId!);
      }
    };
    addEventListener("keydown", k);
    return () => removeEventListener("keydown", k);
  }, [p.selectedId, p.annots]);

  // ------------------------------------------------------------------ Darstellung
  const shapes = [...p.annots, ...(draft ? [draft] : [])];
  const selected = p.annots.find((a) => a.id === p.selectedId) ?? null;
  const interactive = tool === "select" || tool === "hand";

  return (
    <div
      ref={root}
      className={cx("lp-overlay", drawing && "is-drawing", tool === "eraser" && "is-erasing", "tool-" + tool)}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      style={{ "--ov-h": H } as CSSProperties}
    >
      <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" className="lp-svg">
        {shapes.map((a) => (
          <Shape key={a.id} a={a} g={g} interactive={interactive} onDown={(e) => startMove(e, a)} />
        ))}
      </svg>
      {shapes.map((a) =>
        a.type === "text" ? (
          <TextBox
            key={a.id}
            a={a}
            g={g}
            editing={p.editingId === a.id}
            interactive={interactive}
            onDown={(e) => startMove(e, a)}
            onCommit={(text, hView) => {
              p.onEdit(null);
              if (!text.trim()) return p.onDelete(a.id);
              const vr = pdfRectToView(g, a.rect);
              const rect = viewRectToPdf(g, [vr[0], vr[1], vr[2], vr[1] + Math.max(hView, a.fontSize * 1.3)]);
              p.onChange({ ...a, text, rect }, true);
            }}
          />
        ) : a.type === "note" ? (
          <Note
            key={a.id}
            a={a}
            g={g}
            open={p.editingId === a.id}
            interactive={interactive}
            onDown={(e) => startMove(e, a)}
            onClose={(text) => {
              p.onEdit(null);
              if (text !== a.text) p.onChange({ ...a, text }, true);
            }}
            onDelete={() => p.onDelete(a.id)}
          />
        ) : a.type === "image" ? (
          <img
            key={a.id}
            data-ann={a.id}
            src={a.src}
            draggable={false}
            className={cx("lp-img", interactive && "is-interactive")}
            style={rectStyle(pdfRectToView(g, a.rect), W, H)}
            onPointerDown={interactive ? (e) => startMove(e, a) : undefined}
            alt=""
          />
        ) : null,
      )}
      {selected && interactive && p.editingId !== selected.id && (
        <Selection
          a={selected}
          g={g}
          onHandle={(e, h) => startMove(e, selected, h)}
          onMoveStart={(e) => startMove(e, selected)}
          onDelete={() => p.onDelete(selected.id)}
          onColor={(c) => p.onChange({ ...selected, color: c } as Annotation, true)}
          onNote={() => {
            const n = prompt(tr("Kommentar"), selected.note ?? (selected.type === "text" || selected.type === "note" ? selected.text : ""));
            if (n === null) return;
            if (selected.type === "note" || selected.type === "text") p.onChange({ ...selected, text: n }, true);
            else p.onChange({ ...selected, note: n || undefined }, true);
          }}
          onDuplicate={() => {
            const copy = moveBy(g, { ...selected, id: uid(), createdAt: Date.now() }, 12, 12);
            p.onAdd(copy);
            p.onSelect(copy.id);
          }}
        />
      )}
    </div>
  );
}

function rectStyle(r: VRect, W: number, H: number): CSSProperties {
  return { left: pct(r[0], W), top: pct(r[1], H), width: pct(r[2] - r[0], W), height: pct(r[3] - r[1], H) };
}

function Shape({ a, g, interactive, onDown }: { a: Annotation; g: PageGeom; interactive: boolean; onDown: (e: React.PointerEvent) => void }) {
  const common = {
    "data-ann": a.id,
    className: interactive ? "lp-hit" : undefined,
    onPointerDown: interactive ? onDown : undefined,
  } as any;
  switch (a.type) {
    case "highlight":
      return (
        <g {...common} style={{ mixBlendMode: "multiply" }} opacity={a.opacity * 0.55}>
          {a.rects.map((r, i) => {
            const v = pdfRectToView(g, r);
            return <rect key={i} x={v[0]} y={v[1]} width={v[2] - v[0]} height={v[3] - v[1]} fill={a.color} rx={1} />;
          })}
        </g>
      );
    case "underline":
    case "strike":
      return (
        <g {...common} opacity={a.opacity}>
          {a.rects.map((r, i) => {
            const v = pdfRectToView(g, r);
            const h = v[3] - v[1];
            const vertical = g.rotation % 180 !== 0;
            if (vertical) {
              const w = v[2] - v[0];
              const x = a.type === "underline" ? (g.rotation === 90 ? v[0] + w * 0.08 : v[2] - w * 0.08) : v[0] + w * 0.5;
              return (
                <g key={i}>
                  <rect x={v[0]} y={v[1]} width={w} height={h} fill="transparent" />
                  <line x1={x} x2={x} y1={v[1]} y2={v[3]} stroke={a.color} strokeWidth={Math.max(0.8, w * 0.07)} />
                </g>
              );
            }
            const y = a.type === "underline" ? v[3] - h * 0.08 : v[1] + h * 0.55;
            return (
              <g key={i}>
                <rect x={v[0]} y={v[1]} width={v[2] - v[0]} height={h} fill="transparent" />
                <line x1={v[0]} x2={v[2]} y1={y} y2={y} stroke={a.color} strokeWidth={Math.max(0.8, h * 0.07)} />
              </g>
            );
          })}
        </g>
      );
    case "ink": {
      const d = a.paths
        .map((path) =>
          path
            .map((pt, i) => {
              const v = g.toView(pt);
              return (i ? "L" : "M") + v[0].toFixed(2) + " " + v[1].toFixed(2);
            })
            .join(" ") + (path.length === 1 ? " l0.01 0" : ""),
        )
        .join(" ");
      return (
        <g {...common} style={a.highlighter ? { mixBlendMode: "multiply" } : undefined}>
          <path d={d} fill="none" stroke="transparent" strokeWidth={Math.max(a.width, 10)} strokeLinecap="round" />
          <path d={d} fill="none" stroke={a.color} strokeOpacity={a.opacity} strokeWidth={a.width} strokeLinecap="round" strokeLinejoin="round" />
        </g>
      );
    }
    case "rect":
    case "redact": {
      const v = pdfRectToView(g, a.rect);
      const redact = a.type === "redact";
      return (
        <rect
          {...common}
          x={v[0]}
          y={v[1]}
          width={v[2] - v[0]}
          height={v[3] - v[1]}
          fill={redact ? "rgba(0,0,0,.55)" : (a.fill ?? "transparent")}
          stroke={redact ? "#d33" : a.color}
          strokeDasharray={redact ? "4 3" : undefined}
          strokeWidth={redact ? 1.2 : a.width}
          opacity={redact ? 1 : a.opacity}
          pointerEvents={interactive ? "all" : undefined}
        />
      );
    }
    case "ellipse": {
      const v = pdfRectToView(g, a.rect);
      return (
        <ellipse
          {...common}
          cx={(v[0] + v[2]) / 2}
          cy={(v[1] + v[3]) / 2}
          rx={(v[2] - v[0]) / 2}
          ry={(v[3] - v[1]) / 2}
          fill={a.fill ?? "transparent"}
          stroke={a.color}
          strokeWidth={a.width}
          opacity={a.opacity}
          pointerEvents={interactive ? "all" : undefined}
        />
      );
    }
    case "line":
    case "arrow": {
      const f = g.toView(a.from),
        t = g.toView(a.to);
      const head = a.type === "arrow" ? arrowHead(a.from, a.to, a.width).map((p) => g.toView(p)) : null;
      return (
        <g {...common} opacity={a.opacity} stroke={a.color} strokeWidth={a.width} strokeLinecap="round" strokeLinejoin="round" fill="none">
          <line x1={f[0]} y1={f[1]} x2={t[0]} y2={t[1]} stroke="transparent" strokeWidth={Math.max(10, a.width + 6)} />
          <line x1={f[0]} y1={f[1]} x2={t[0]} y2={t[1]} />
          {head && <polyline points={`${head[0][0]},${head[0][1]} ${t[0]},${t[1]} ${head[1][0]},${head[1][1]}`} />}
        </g>
      );
    }
    default:
      return null;
  }
}

function TextBox({
  a,
  g,
  editing,
  interactive,
  onDown,
  onCommit,
}: {
  a: Extract<Annotation, { type: "text" }>;
  g: PageGeom;
  editing: boolean;
  interactive: boolean;
  onDown: (e: React.PointerEvent) => void;
  onCommit: (text: string, heightView: number) => void;
}) {
  const ref = useRef<HTMLTextAreaElement>(null);
  const [val, setVal] = useState(a.text);
  useEffect(() => setVal(a.text), [a.text]);
  const v = pdfRectToView(g, a.rect);
  const style: CSSProperties = {
    ...rectStyle(v, g.width, g.height),
    height: editing ? "auto" : rectStyle(v, g.width, g.height).height,
    minHeight: rectStyle(v, g.width, g.height).height,
    color: a.color,
    opacity: a.opacity,
    fontSize: `calc(var(--scale-factor, 1) * ${a.fontSize}px)`,
    fontWeight: a.bold ? 700 : 400,
  };
  const grow = () => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = el.scrollHeight + "px";
  };
  useLayoutEffect(() => {
    if (editing) {
      grow();
      ref.current?.focus();
      const l = ref.current?.value.length ?? 0;
      ref.current?.setSelectionRange(l, l);
    }
  }, [editing]);
  const commit = () => {
    const el = ref.current!;
    const host = el.closest(".lp-overlay") as HTMLElement;
    const hView = (el.scrollHeight / host.getBoundingClientRect().height) * g.height;
    onCommit(val, hView);
  };
  if (editing)
    return (
      <textarea
        ref={ref}
        className="lp-text is-editing"
        style={style}
        value={val}
        placeholder={tr("Text eingeben …")}
        onChange={(e) => {
          setVal(e.target.value);
          grow();
        }}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "Escape" || (e.key === "Enter" && (e.ctrlKey || e.metaKey))) (e.target as HTMLTextAreaElement).blur();
          e.stopPropagation();
        }}
        onPointerDown={(e) => e.stopPropagation()}
      />
    );
  return (
    <div data-ann={a.id} className={cx("lp-text", interactive && "is-interactive")} style={style} onPointerDown={interactive ? onDown : undefined}>
      {a.text}
    </div>
  );
}

function Note({
  a,
  g,
  open,
  interactive,
  onDown,
  onClose,
  onDelete,
}: {
  a: Extract<Annotation, { type: "note" }>;
  g: PageGeom;
  open: boolean;
  interactive: boolean;
  onDown: (e: React.PointerEvent) => void;
  onClose: (text: string) => void;
  onDelete: () => void;
}) {
  const [val, setVal] = useState(a.text);
  useEffect(() => setVal(a.text), [a.text, open]);
  const [x, y] = g.toView(a.at);
  return (
    <>
      <div
        data-ann={a.id}
        className={cx("lp-note", interactive && "is-interactive")}
        style={{ left: pct(x, g.width), top: pct(y, g.height), background: a.color }}
        onPointerDown={interactive ? onDown : undefined}
        title={a.text}
      >
        <MessageSquare size={13} strokeWidth={2.4} />
      </div>
      {open && (
        <div className="lp-note-pop" style={{ left: pct(Math.min(x, g.width - 200), g.width), top: pct(y + 24, g.height) }} onPointerDown={(e) => e.stopPropagation()}>
          <div className="lp-note-head">
            <span>{a.author || tr("Notiz")}</span>
            <button onClick={onDelete} aria-label={tr("Löschen")}>
              <Trash2 size={14} />
            </button>
          </div>
          <textarea
            autoFocus
            value={val}
            placeholder={tr("Notiz schreiben …")}
            onChange={(e) => setVal(e.target.value)}
            onKeyDown={(e) => {
              e.stopPropagation();
              if (e.key === "Escape") onClose(val);
            }}
          />
          <div className="lp-note-foot">
            <button className="btn btn-primary btn-sm" onClick={() => onClose(val)}>
              {tr("Fertig")}
            </button>
          </div>
        </div>
      )}
    </>
  );
}

function Selection({
  a,
  g,
  onHandle,
  onMoveStart,
  onDelete,
  onColor,
  onNote,
  onDuplicate,
}: {
  a: Annotation;
  g: PageGeom;
  onHandle: (e: React.PointerEvent, h: string) => void;
  onMoveStart: (e: React.PointerEvent) => void;
  onDelete: () => void;
  onColor: (c: string) => void;
  onNote: () => void;
  onDuplicate: () => void;
}) {
  const b = viewBounds(g, a);
  const pad = 3;
  const box: VRect = [b[0] - pad, b[1] - pad, b[2] + pad, b[3] + pad];
  const resizable = "rect" in a;
  const isLine = a.type === "line" || a.type === "arrow";
  const movable = !(a.type === "highlight" || a.type === "underline" || a.type === "strike");
  const [showColors, setShowColors] = useState(false);
  return (
    <>
      {!isLine && (
        <div className={cx("lp-sel", movable && "is-movable")} style={rectStyle(box, g.width, g.height)} onPointerDown={movable ? onMoveStart : undefined}>
          {resizable &&
            ["nw", "n", "ne", "e", "se", "s", "sw", "w"].map((h) => (
              <span key={h} className={"lp-handle h-" + h} onPointerDown={(e) => onHandle(e, h)} />
            ))}
        </div>
      )}
      {isLine &&
        (["from", "to"] as const).map((h) => {
          const [x, y] = g.toView(a[h]);
          return <span key={h} className="lp-handle lp-handle-pt" style={{ left: pct(x, g.width), top: pct(y, g.height) }} onPointerDown={(e) => onHandle(e, h)} />;
        })}
      <div className="lp-sel-bar" style={{ left: pct((box[0] + box[2]) / 2, g.width), top: pct(box[1], g.height) }} onPointerDown={(e) => e.stopPropagation()}>
        {a.type !== "image" && a.type !== "redact" && (
          <button className="lp-sel-color" style={{ background: a.color }} onClick={() => setShowColors((s) => !s)} aria-label={tr("Farbe")} />
        )}
        <button onClick={onNote} aria-label={tr("Kommentar")} data-tip={tr("Kommentar")}>
          <MessageSquare size={15} />
        </button>
        {movable && (
          <button onClick={onDuplicate} aria-label={tr("Duplizieren")} data-tip={tr("Duplizieren")}>
            <Copy size={15} />
          </button>
        )}
        <button onClick={onDelete} aria-label={tr("Löschen")} data-tip={tr("Löschen")} className="danger">
          <Trash2 size={15} />
        </button>
        {showColors && (
          <div className="lp-sel-colors">
            {PALETTE.slice(0, 10).map((c) => (
              <button
                key={c}
                style={{ background: c }}
                onClick={() => {
                  onColor(c);
                  setShowColors(false);
                }}
              />
            ))}
          </div>
        )}
      </div>
    </>
  );
}
