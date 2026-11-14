import { useEffect, useRef, useState } from "react";
import { PenLine, Type, Image as ImageIcon, Trash2, Eraser, Stamp } from "lucide-react";
import { Btn, Modal, Segmented, Swatches, cx } from "./ui";
import { db, uid } from "../lib/db";
import { pickFiles } from "../lib/platform";
import type { SavedSignature } from "../types";
import { useT } from "../i18n";
import "@fontsource/dancing-script/600.css";
import "@fontsource/caveat/600.css";
import "@fontsource/great-vibes/400.css";

export interface PlacedImage {
  src: string;
  aspect: number;
  kind: "signature" | "image" | "stamp";
}

/** Transparente Ränder abschneiden */
function trimCanvas(c: HTMLCanvasElement): HTMLCanvasElement {
  const ctx = c.getContext("2d")!;
  const { width, height } = c;
  const data = ctx.getImageData(0, 0, width, height).data;
  let x1 = width,
    y1 = height,
    x2 = 0,
    y2 = 0;
  for (let y = 0; y < height; y++)
    for (let x = 0; x < width; x++)
      if (data[(y * width + x) * 4 + 3] > 8) {
        if (x < x1) x1 = x;
        if (y < y1) y1 = y;
        if (x > x2) x2 = x;
        if (y > y2) y2 = y;
      }
  if (x2 <= x1 || y2 <= y1) return c;
  const pad = 6;
  const out = document.createElement("canvas");
  out.width = x2 - x1 + pad * 2;
  out.height = y2 - y1 + pad * 2;
  out.getContext("2d")!.drawImage(c, x1 - pad, y1 - pad, out.width, out.height, 0, 0, out.width, out.height);
  return out;
}

export const aspectOf = (src: string) =>
  new Promise<number>((r) => {
    const i = new Image();
    i.onload = () => r(i.width / i.height || 1);
    i.onerror = () => r(1);
    i.src = src;
  });

const FONTS = [
  { f: "'Dancing Script'", label: "Dancing" },
  { f: "'Great Vibes'", label: "Vibes" },
  { f: "'Caveat'", label: "Caveat" },
];

export function SignatureDialog({ open, onClose, onPick, kind }: { open: boolean; onClose: () => void; onPick: (img: PlacedImage) => void; kind: "signature" | "initials" }) {
  const t = useT();
  const [mode, setMode] = useState<"draw" | "type" | "image">("draw");
  const [saved, setSaved] = useState<SavedSignature[]>([]);
  const [color, setColor] = useState("#1d2a6b");
  const [typed, setTyped] = useState("");
  const [font, setFont] = useState(0);
  const [hasInk, setHasInk] = useState(false);
  const [remember, setRemember] = useState(true);
  const canvas = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    if (open) db.listSignatures().then((s) => setSaved(s.filter((x) => x.kind === kind).sort((a, b) => b.createdAt - a.createdAt)));
  }, [open, kind]);

  // Zeichenfläche
  useEffect(() => {
    if (!open || mode !== "draw") return;
    const c = canvas.current;
    if (!c) return;
    const dpr = Math.max(2, devicePixelRatio || 1);
    const r = c.getBoundingClientRect();
    c.width = r.width * dpr;
    c.height = r.height * dpr;
    const ctx = c.getContext("2d")!;
    ctx.scale(dpr, dpr);
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    setHasInk(false);
    let last: { x: number; y: number; w: number } | null = null;
    let mid: { x: number; y: number } | null = null;
    const pos = (e: PointerEvent) => {
      const b = c.getBoundingClientRect();
      return { x: e.clientX - b.left, y: e.clientY - b.top };
    };
    const down = (e: PointerEvent) => {
      c.setPointerCapture(e.pointerId);
      const p = pos(e);
      last = { ...p, w: 2.6 };
      mid = p;
      ctx.fillStyle = color;
      ctx.beginPath();
      ctx.arc(p.x, p.y, 1.3, 0, Math.PI * 2);
      ctx.fill();
      setHasInk(true);
    };
    const move = (e: PointerEvent) => {
      if (!last || !mid) return;
      const p = pos(e);
      const d = Math.hypot(p.x - last.x, p.y - last.y);
      if (d < 0.8) return;
      // Strichstärke nach Geschwindigkeit (wirkt wie ein echter Stift)
      const pressure = e.pressure && e.pointerType === "pen" ? e.pressure : 0.5;
      const w = Math.max(1.2, Math.min(4.2, (pressure * 2 + 2.6 - d * 0.05) * 0.5 + last.w * 0.5));
      const m = { x: (last.x + p.x) / 2, y: (last.y + p.y) / 2 };
      ctx.strokeStyle = color;
      ctx.lineWidth = w;
      ctx.beginPath();
      ctx.moveTo(mid.x, mid.y);
      ctx.quadraticCurveTo(last.x, last.y, m.x, m.y);
      ctx.stroke();
      mid = m;
      last = { ...p, w };
    };
    const up = () => {
      last = null;
      mid = null;
    };
    c.addEventListener("pointerdown", down);
    c.addEventListener("pointermove", move);
    c.addEventListener("pointerup", up);
    c.addEventListener("pointercancel", up);
    return () => {
      c.removeEventListener("pointerdown", down);
      c.removeEventListener("pointermove", move);
      c.removeEventListener("pointerup", up);
      c.removeEventListener("pointercancel", up);
    };
  }, [open, mode, color]);

  const clear = () => {
    const c = canvas.current!;
    c.getContext("2d")!.clearRect(0, 0, c.width, c.height);
    setHasInk(false);
  };

  const finish = async (src: string) => {
    if (remember) {
      const s: SavedSignature = { id: uid(), src, kind, createdAt: Date.now() };
      await db.putSignature(s);
    }
    onPick({ src, aspect: await aspectOf(src), kind: "signature" });
    onClose();
  };

  const create = async () => {
    if (mode === "draw") {
      if (!hasInk) return;
      return finish(trimCanvas(canvas.current!).toDataURL("image/png"));
    }
    if (mode === "type") {
      if (!typed.trim()) return;
      await document.fonts.load(`600 80px ${FONTS[font].f}`);
      const c = document.createElement("canvas");
      const ctx = c.getContext("2d")!;
      ctx.font = `600 120px ${FONTS[font].f}`;
      const w = ctx.measureText(typed).width;
      c.width = w + 60;
      c.height = 200;
      ctx.font = `600 120px ${FONTS[font].f}`;
      ctx.fillStyle = color;
      ctx.textBaseline = "middle";
      ctx.fillText(typed, 30, 100);
      return finish(trimCanvas(c).toDataURL("image/png"));
    }
  };

  const fromImage = async () => {
    const [f] = await pickFiles("image/*", false);
    if (!f) return;
    const blob = new Blob([f.data]);
    const bmp = await createImageBitmap(blob);
    const c = document.createElement("canvas");
    const scale = Math.min(1, 900 / bmp.width);
    c.width = bmp.width * scale;
    c.height = bmp.height * scale;
    const ctx = c.getContext("2d")!;
    ctx.drawImage(bmp, 0, 0, c.width, c.height);
    // Weißen Hintergrund (eingescannte Unterschrift) transparent machen
    const img = ctx.getImageData(0, 0, c.width, c.height);
    for (let i = 0; i < img.data.length; i += 4) {
      const l = (img.data[i] + img.data[i + 1] + img.data[i + 2]) / 3;
      if (l > 225) img.data[i + 3] = 0;
      else if (l > 180) img.data[i + 3] = Math.round(((225 - l) / 45) * 255);
    }
    ctx.putImageData(img, 0, 0);
    await finish(trimCanvas(c).toDataURL("image/png"));
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      icon={<PenLine size={18} />}
      title={kind === "initials" ? t("Initialen") : t("Unterschrift")}
      wide
      footer={
        mode !== "image" ? (
          <>
            <label className="check">
              <input type="checkbox" checked={remember} onChange={(e) => setRemember(e.target.checked)} /> {t("Für später speichern")}
            </label>
            <span className="grow" />
            <Btn kind="ghost" onClick={onClose}>
              {t("Abbrechen")}
            </Btn>
            <Btn kind="primary" onClick={create} disabled={mode === "draw" ? !hasInk : !typed.trim()}>
              {t("Übernehmen")}
            </Btn>
          </>
        ) : undefined
      }
    >
      {saved.length > 0 && (
        <div className="sig-saved">
          <div className="field-label">{t("Gespeichert – zum Einfügen antippen")}</div>
          <div className="sig-list">
            {saved.map((s) => (
              <div key={s.id} className="sig-item">
                <button
                  onClick={async () => {
                    onPick({ src: s.src, aspect: await aspectOf(s.src), kind: "signature" });
                    onClose();
                  }}
                >
                  <img src={s.src} alt="" />
                </button>
                <button
                  className="sig-del"
                  aria-label={t("Löschen")}
                  onClick={async () => {
                    await db.deleteSignature(s.id);
                    setSaved((x) => x.filter((y) => y.id !== s.id));
                  }}
                >
                  <Trash2 size={13} />
                </button>
              </div>
            ))}
          </div>
        </div>
      )}
      <Segmented
        value={mode}
        onChange={setMode}
        options={[
          { value: "draw", label: <><PenLine size={15} /> {t("Zeichnen")}</> },
          { value: "type", label: <><Type size={15} /> {t("Tippen")}</> },
          { value: "image", label: <><ImageIcon size={15} /> {t("Bild")}</> },
        ]}
      />
      <div className="sig-color">
        <Swatches value={color} onChange={setColor} colors={["#1d2a6b", "#111111", "#1f5fbf", "#3d5639", "#8b1e1e"]} />
      </div>
      {mode === "draw" && (
        <div className="sig-pad">
          <canvas ref={canvas} />
          <div className="sig-line" />
          <span className="sig-hint">{t("Hier unterschreiben")}</span>
          {hasInk && (
            <button className="iconbtn sig-clear" onClick={clear} aria-label={t("Leeren")}>
              <Eraser size={16} />
            </button>
          )}
        </div>
      )}
      {mode === "type" && (
        <div className="sig-type">
          <input autoFocus className="input" value={typed} placeholder={kind === "initials" ? "M. M." : "Max Mustermann"} onChange={(e) => setTyped(e.target.value)} onKeyDown={(e) => e.stopPropagation()} />
          <div className="sig-fonts">
            {FONTS.map((f, i) => (
              <button key={f.label} className={cx("sig-font", i === font && "is-active")} style={{ fontFamily: f.f, color }} onClick={() => setFont(i)}>
                {typed || (kind === "initials" ? "M. M." : "Max Mustermann")}
              </button>
            ))}
          </div>
        </div>
      )}
      {mode === "image" && (
        <div className="sig-image">
          <p className="muted">{t("Foto oder Scan deiner Unterschrift wählen. Weißer Hintergrund wird automatisch entfernt.")}</p>
          <Btn kind="primary" onClick={fromImage}>
            <ImageIcon size={16} /> {t("Bild auswählen …")}
          </Btn>
        </div>
      )}
    </Modal>
  );
}

// ------------------------------------------------------------------ Stempel
export const STAMPS: { text: string; color: string }[] = [
  { text: "GENEHMIGT", color: "#2e7d32" },
  { text: "ERLEDIGT", color: "#2e7d32" },
  { text: "ENTWURF", color: "#1f5fbf" },
  { text: "VERTRAULICH", color: "#c62828" },
  { text: "ABGELEHNT", color: "#c62828" },
  { text: "GEPRÜFT", color: "#6a1b9a" },
  { text: "KOPIE", color: "#555555" },
  { text: "DRINGEND", color: "#e65100" },
];

export function stampImage(text: string, color: string, withDate: boolean): PlacedImage {
  const c = document.createElement("canvas");
  const ctx = c.getContext("2d")!;
  const font = "800 64px Nunito Variable, Nunito, system-ui, sans-serif";
  ctx.font = font;
  const w = ctx.measureText(text).width;
  const date = withDate ? new Date().toLocaleDateString() : "";
  c.width = Math.ceil(w + 80);
  c.height = withDate ? 150 : 110;
  ctx.font = font;
  ctx.strokeStyle = color;
  ctx.fillStyle = color;
  ctx.lineWidth = 7;
  const r = 18;
  const W = c.width - 8,
    H = c.height - 8;
  ctx.beginPath();
  ctx.roundRect(4, 4, W, H, r);
  ctx.stroke();
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.fillText(text, c.width / 2, withDate ? 58 : c.height / 2 + 2);
  if (withDate) {
    ctx.font = "700 30px Nunito Variable, Nunito, system-ui, sans-serif";
    ctx.fillText(date, c.width / 2, 112);
  }
  return { src: c.toDataURL("image/png"), aspect: c.width / c.height, kind: "stamp" };
}

export function StampDialog({ open, onClose, onPick }: { open: boolean; onClose: () => void; onPick: (img: PlacedImage) => void }) {
  const t = useT();
  const [withDate, setWithDate] = useState(true);
  const [custom, setCustom] = useState("");
  const [color, setColor] = useState("#c62828");
  return (
    <Modal open={open} onClose={onClose} title={t("Stempel")} icon={<Stamp size={18} />}>
      <div className="stamps">
        {STAMPS.map((s) => (
          <button
            key={s.text}
            className="stamp-btn"
            style={{ color: s.color, borderColor: s.color }}
            onClick={() => {
              onPick(stampImage(t(s.text), s.color, withDate));
              onClose();
            }}
          >
            {t(s.text)}
          </button>
        ))}
      </div>
      <label className="check">
        <input type="checkbox" checked={withDate} onChange={(e) => setWithDate(e.target.checked)} /> {t("Datum hinzufügen")}
      </label>
      <div className="field-label" style={{ marginTop: 14 }}>
        {t("Eigener Stempel")}
      </div>
      <div className="row-gap">
        <input className="input grow" value={custom} placeholder={t("Text")} onChange={(e) => setCustom(e.target.value.toUpperCase())} onKeyDown={(e) => e.stopPropagation()} />
        <Btn
          kind="primary"
          disabled={!custom.trim()}
          onClick={() => {
            onPick(stampImage(custom.trim(), color, withDate));
            onClose();
          }}
        >
          {t("Einfügen")}
        </Btn>
      </div>
      <Swatches value={color} onChange={setColor} colors={["#c62828", "#2e7d32", "#1f5fbf", "#6a1b9a", "#e65100", "#555555"]} />
    </Modal>
  );
}
