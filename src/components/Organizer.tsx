import { useEffect, useState } from "react";
import { RotateCcw, RotateCw, Trash2, Copy, FilePlus2, FileUp, ImagePlus, FileOutput, ArrowLeft, ArrowRight, X, CheckSquare, Undo2, Scissors } from "lucide-react";
import type { PDFDocumentProxy } from "../lib/pdfjs";
import { Thumb } from "./SidePanel";
import { IconBtn, cx } from "./ui";
import { useT } from "../i18n";

export type PageOp =
  | { op: "rotate"; pages: number[]; delta: number }
  | { op: "delete"; pages: number[] }
  | { op: "order"; order: number[] }
  | { op: "duplicate"; pages: number[] }
  | { op: "blank"; at: number }
  | { op: "file"; at: number }
  | { op: "images"; at: number }
  | { op: "extract"; pages: number[] };

export default function Organizer({
  doc,
  total,
  onOp,
  onClose,
  onSplit,
  canUndo,
  onUndo,
  version,
}: {
  doc: PDFDocumentProxy;
  total: number;
  onOp: (op: PageOp) => void;
  onClose: () => void;
  onSplit: () => void;
  canUndo: boolean;
  onUndo: () => void;
  version: number;
}) {
  const t = useT();
  const [sel, setSel] = useState<Set<number>>(new Set()); // 0-basiert
  const [anchor, setAnchor] = useState<number | null>(null);
  const [dragFrom, setDragFrom] = useState<number | null>(null);
  const [dropAt, setDropAt] = useState<number | null>(null);
  const [size, setSize] = useState(innerWidth < 600 ? 120 : 160);
  useEffect(() => setSel(new Set()), [version]);

  const pages = [...sel].sort((a, b) => a - b);
  const has = pages.length > 0;
  const lastSel = has ? pages[pages.length - 1] : total - 1;

  const click = (i: number, e: React.MouseEvent) => {
    const s = new Set(sel);
    if (e.shiftKey && anchor != null) {
      for (let k = Math.min(anchor, i); k <= Math.max(anchor, i); k++) s.add(k);
    } else if (s.has(i)) s.delete(i);
    else s.add(i);
    setSel(s);
    setAnchor(i);
  };

  const move = (dir: -1 | 1) => {
    if (!has) return;
    const order = Array.from({ length: total }, (_, i) => i);
    const set = new Set(pages);
    if (dir < 0) {
      for (let i = 1; i < order.length; i++) if (set.has(order[i]) && !set.has(order[i - 1])) [order[i - 1], order[i]] = [order[i], order[i - 1]];
    } else {
      for (let i = order.length - 2; i >= 0; i--) if (set.has(order[i]) && !set.has(order[i + 1])) [order[i + 1], order[i]] = [order[i], order[i + 1]];
    }
    onOp({ op: "order", order });
  };

  const drop = (to: number) => {
    if (dragFrom == null) return;
    const moving = sel.has(dragFrom) ? pages : [dragFrom];
    const rest = Array.from({ length: total }, (_, i) => i).filter((i) => !moving.includes(i));
    const before = Array.from({ length: to }, (_, i) => i).filter((i) => !moving.includes(i)).length;
    const order = [...rest.slice(0, before), ...moving, ...rest.slice(before)];
    setDragFrom(null);
    setDropAt(null);
    if (order.some((v, i) => v !== i)) onOp({ op: "order", order });
  };

  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      if ((e.target as HTMLElement).tagName === "INPUT") return;
      if ((e.key === "Delete" || e.key === "Backspace") && has) onOp({ op: "delete", pages });
      if (e.key === "a" && (e.ctrlKey || e.metaKey)) {
        e.preventDefault();
        setSel(new Set(Array.from({ length: total }, (_, i) => i)));
      }
      if (e.key === "Escape") has ? setSel(new Set()) : onClose();
    };
    addEventListener("keydown", k);
    return () => removeEventListener("keydown", k);
  });

  return (
    <div className="organizer">
      <div className="org-bar">
        <IconBtn icon={<X size={18} />} label={t("Fertig")} onClick={onClose} />
        <div className="org-title">
          <b>{t("Seiten organisieren")}</b>
          <span>{has ? t("{0} ausgewählt", pages.length) : t("{0} Seiten · antippen zum Auswählen", total)}</span>
        </div>
        <span className="grow" />
        <IconBtn icon={<Undo2 size={18} />} label={t("Rückgängig")} disabled={!canUndo} onClick={onUndo} />
        <IconBtn
          icon={<CheckSquare size={18} />}
          label={has ? t("Auswahl aufheben") : t("Alle auswählen")}
          onClick={() => setSel(has ? new Set() : new Set(Array.from({ length: total }, (_, i) => i)))}
        />
        <input className="org-zoom" type="range" min={90} max={260} value={size} onChange={(e) => setSize(+e.target.value)} aria-label="Zoom" />
      </div>
      <div className="org-tools">
        <IconBtn showLabel icon={<RotateCcw size={18} />} label={t("Links drehen")} disabled={!has} onClick={() => onOp({ op: "rotate", pages, delta: -90 })} />
        <IconBtn showLabel icon={<RotateCw size={18} />} label={t("Rechts drehen")} disabled={!has} onClick={() => onOp({ op: "rotate", pages, delta: 90 })} />
        <IconBtn showLabel icon={<ArrowLeft size={18} />} label={t("Nach vorne")} disabled={!has} onClick={() => move(-1)} />
        <IconBtn showLabel icon={<ArrowRight size={18} />} label={t("Nach hinten")} disabled={!has} onClick={() => move(1)} />
        <IconBtn showLabel icon={<Copy size={18} />} label={t("Duplizieren")} disabled={!has} onClick={() => onOp({ op: "duplicate", pages })} />
        <IconBtn showLabel icon={<FileOutput size={18} />} label={t("Extrahieren")} disabled={!has} onClick={() => onOp({ op: "extract", pages })} />
        <IconBtn showLabel icon={<Trash2 size={18} />} label={t("Löschen")} disabled={!has || pages.length === total} onClick={() => onOp({ op: "delete", pages })} className="danger" />
        <span className="org-sep" />
        <IconBtn showLabel icon={<FilePlus2 size={18} />} label={t("Leere Seite")} onClick={() => onOp({ op: "blank", at: lastSel + 1 })} />
        <IconBtn showLabel icon={<FileUp size={18} />} label={t("PDF einfügen")} onClick={() => onOp({ op: "file", at: lastSel + 1 })} />
        <IconBtn showLabel icon={<ImagePlus size={18} />} label={t("Bilder einfügen")} onClick={() => onOp({ op: "images", at: lastSel + 1 })} />
        <IconBtn showLabel icon={<Scissors size={18} />} label={t("Teilen")} onClick={onSplit} />
      </div>
      <div className="org-grid" style={{ gridTemplateColumns: `repeat(auto-fill, minmax(${size}px, 1fr))` }}>
        {Array.from({ length: total }, (_, i) => (
          <div
            key={i + ":" + version}
            className={cx("org-page", sel.has(i) && "is-selected", dropAt === i && "drop-before", dragFrom === i && "is-dragging")}
            draggable
            onDragStart={(e) => {
              setDragFrom(i);
              e.dataTransfer.effectAllowed = "move";
            }}
            onDragOver={(e) => {
              e.preventDefault();
              const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
              setDropAt(e.clientX > r.left + r.width / 2 ? i + 1 : i);
            }}
            onDragEnd={() => {
              setDragFrom(null);
              setDropAt(null);
            }}
            onDrop={(e) => {
              e.preventDefault();
              if (dropAt != null) drop(dropAt);
            }}
            onClick={(e) => click(i, e)}
          >
            <div className="org-thumb">
              <Thumb doc={doc} page={i + 1} width={size} version={version} />
              <span className="org-check">{sel.has(i) ? "✓" : ""}</span>
            </div>
            <span className="org-no">{i + 1}</span>
          </div>
        ))}
        {dropAt === total && <div className="org-drop-end" />}
      </div>
    </div>
  );
}
