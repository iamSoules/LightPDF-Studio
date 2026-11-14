import { useEffect, useState } from "react";
import { Droplets, Hash, Lock, Minimize2, Info, Images, Scissors, Eye, EyeOff } from "lucide-react";
import { Btn, Field, Modal, Range, Segmented, Swatches } from "./ui";
import type { Meta, Position, ProtectOpts, StampTextOpts, WatermarkOpts } from "../lib/pdfops";
import { useT } from "../i18n";

export type ToolKind = "watermark" | "numbers" | "protect" | "compress" | "meta" | "images" | "split";

export interface ToolResult {
  watermark?: WatermarkOpts;
  numbers?: StampTextOpts;
  protect?: ProtectOpts;
  unprotect?: boolean;
  compress?: "lossless" | "medium" | "strong";
  meta?: Partial<Meta>;
  images?: { fmt: "png" | "jpeg"; dpi: number; range: string };
  split?: { mode: "every" | "ranges"; every: number; ranges: string };
}

export function ToolDialog({
  kind,
  onClose,
  onRun,
  total,
  meta,
  encrypted,
  size,
}: {
  kind: ToolKind | null;
  onClose: () => void;
  onRun: (r: ToolResult) => void;
  total: number;
  meta?: Meta | null;
  encrypted?: boolean;
  size: number;
}) {
  const t = useT();
  // Wasserzeichen
  const [wm, setWm] = useState<WatermarkOpts>({ text: t("VERTRAULICH"), size: 64, color: "#c62828", opacity: 0.22, angle: 45 });
  // Seitenzahlen
  const [num, setNum] = useState<StampTextOpts>({ template: t("Seite {n} von {total}"), position: "bc", size: 10, color: "#333333", start: 1, margin: 28, skipFirst: false, name: "" });
  // Schutz
  const [pw, setPw] = useState("");
  const [pw2, setPw2] = useState("");
  const [showPw, setShowPw] = useState(false);
  const [perm, setPerm] = useState({ allowPrint: true, allowCopy: false, allowModify: false });
  // Komprimieren
  const [level, setLevel] = useState<"lossless" | "medium" | "strong">("lossless");
  // Metadaten
  const [m, setM] = useState<Partial<Meta>>({});
  useEffect(() => setM(meta ?? {}), [meta, kind]);
  // Bilder
  const [img, setImg] = useState<{ fmt: "png" | "jpeg"; dpi: number; range: string }>({ fmt: "png", dpi: 150, range: "" });
  // Teilen
  const [sp, setSp] = useState<{ mode: "every" | "ranges"; every: number; ranges: string }>({ mode: "every", every: 1, ranges: "" });

  if (!kind) return null;
  const run = (r: ToolResult) => {
    onRun(r);
    onClose();
  };
  const cancel = (
    <Btn kind="ghost" onClick={onClose}>
      {t("Abbrechen")}
    </Btn>
  );
  const positions: { value: Position; label: string }[] = [
    { value: "tl", label: "↖" },
    { value: "tc", label: "↑" },
    { value: "tr", label: "↗" },
    { value: "bl", label: "↙" },
    { value: "bc", label: "↓" },
    { value: "br", label: "↘" },
  ];

  switch (kind) {
    case "watermark":
      return (
        <Modal open onClose={onClose} icon={<Droplets size={18} />} title={t("Wasserzeichen")} footer={<>{cancel}<Btn kind="primary" disabled={!wm.text.trim()} onClick={() => run({ watermark: wm })}>{t("Hinzufügen")}</Btn></>}>
          <div className="wm-preview">
            <div style={{ color: wm.color, opacity: wm.opacity, transform: `rotate(${-wm.angle}deg)`, fontSize: wm.size / 3.2 }}>{wm.text}</div>
          </div>
          <Field label={t("Text")}>
            <input className="input" value={wm.text} onChange={(e) => setWm({ ...wm, text: e.target.value })} onKeyDown={(e) => e.stopPropagation()} />
          </Field>
          <Field label={t("Schriftgröße")}>
            <Range value={wm.size} min={16} max={160} onChange={(v) => setWm({ ...wm, size: v })} suffix=" pt" />
          </Field>
          <Field label={t("Deckkraft")}>
            <Range value={Math.round(wm.opacity * 100)} min={5} max={100} onChange={(v) => setWm({ ...wm, opacity: v / 100 })} suffix=" %" />
          </Field>
          <Field label={t("Winkel")}>
            <Segmented value={String(wm.angle)} onChange={(v) => setWm({ ...wm, angle: +v })} options={[0, 30, 45, 60, 90].map((a) => ({ value: String(a), label: a + "°" }))} />
          </Field>
          <Field label={t("Farbe")}>
            <Swatches value={wm.color} onChange={(c) => setWm({ ...wm, color: c })} colors={["#c62828", "#1f5fbf", "#2e7d32", "#555555", "#000000", "#e65100"]} />
          </Field>
        </Modal>
      );
    case "numbers":
      return (
        <Modal open onClose={onClose} icon={<Hash size={18} />} title={t("Seitenzahlen, Kopf- & Fußzeile")} footer={<>{cancel}<Btn kind="primary" disabled={!num.template.trim()} onClick={() => run({ numbers: num })}>{t("Hinzufügen")}</Btn></>}>
          <Field label={t("Text")} hint={t("Platzhalter: {n} Seite, {total} Seitenzahl gesamt, {date} Datum, {name} Name")}>
            <input className="input" value={num.template} onChange={(e) => setNum({ ...num, template: e.target.value })} onKeyDown={(e) => e.stopPropagation()} />
          </Field>
          <div className="chips chips-small">
            {[t("Seite {n} von {total}"), "{n}", "- {n} -", "{n} / {total}", "{date}"].map((tp) => (
              <button key={tp} className="chip" onClick={() => setNum({ ...num, template: tp })}>
                {tp}
              </button>
            ))}
          </div>
          <Field label={t("Position")}>
            <Segmented value={num.position} onChange={(v) => setNum({ ...num, position: v })} options={positions} />
          </Field>
          <div className="grid2">
            <Field label={t("Schriftgröße")}>
              <Range value={num.size} min={6} max={24} onChange={(v) => setNum({ ...num, size: v })} suffix=" pt" />
            </Field>
            <Field label={t("Beginnen mit")}>
              <input className="input" type="number" min={0} value={num.start} onChange={(e) => setNum({ ...num, start: +e.target.value || 1 })} />
            </Field>
          </div>
          <label className="check">
            <input type="checkbox" checked={num.skipFirst} onChange={(e) => setNum({ ...num, skipFirst: e.target.checked })} /> {t("Erste Seite (Deckblatt) auslassen")}
          </label>
        </Modal>
      );
    case "protect":
      if (encrypted)
        return (
          <Modal open onClose={onClose} icon={<Lock size={18} />} title={t("Kennwortschutz")} footer={<>{cancel}<Btn kind="danger" onClick={() => run({ unprotect: true })}>{t("Schutz entfernen")}</Btn></>}>
            <p className="muted">{t("Dieses Dokument ist mit einem Kennwort geschützt. Du kannst den Schutz entfernen – danach lässt es sich ohne Kennwort öffnen.")}</p>
          </Modal>
        );
      return (
        <Modal
          open
          onClose={onClose}
          icon={<Lock size={18} />}
          title={t("Mit Kennwort schützen")}
          footer={
            <>
              {cancel}
              <Btn kind="primary" disabled={pw.length < 4 || pw !== pw2} onClick={() => run({ protect: { userPassword: pw, ...perm } })}>
                {t("Schützen")}
              </Btn>
            </>
          }
        >
          <p className="muted">{t("Das Dokument wird mit AES-Verschlüsselung gespeichert und lässt sich nur mit dem Kennwort öffnen.")}</p>
          <Field label={t("Kennwort")}>
            <div className="input-wrap">
              <input className="input" type={showPw ? "text" : "password"} value={pw} onChange={(e) => setPw(e.target.value)} onKeyDown={(e) => e.stopPropagation()} autoFocus />
              <button className="iconbtn iconbtn-sm" onClick={() => setShowPw(!showPw)} aria-label="Anzeigen">
                {showPw ? <EyeOff size={15} /> : <Eye size={15} />}
              </button>
            </div>
          </Field>
          <Field label={t("Kennwort wiederholen")} hint={pw2 && pw !== pw2 ? t("Die Kennwörter stimmen nicht überein.") : pw && pw.length < 4 ? t("Mindestens 4 Zeichen.") : undefined}>
            <input className="input" type={showPw ? "text" : "password"} value={pw2} onChange={(e) => setPw2(e.target.value)} onKeyDown={(e) => e.stopPropagation()} />
          </Field>
          <div className="field-label">{t("Erlaubt nach dem Öffnen")}</div>
          <label className="check">
            <input type="checkbox" checked={perm.allowPrint} onChange={(e) => setPerm({ ...perm, allowPrint: e.target.checked })} /> {t("Drucken")}
          </label>
          <label className="check">
            <input type="checkbox" checked={perm.allowCopy} onChange={(e) => setPerm({ ...perm, allowCopy: e.target.checked })} /> {t("Text kopieren")}
          </label>
          <label className="check">
            <input type="checkbox" checked={perm.allowModify} onChange={(e) => setPerm({ ...perm, allowModify: e.target.checked })} /> {t("Ändern & kommentieren")}
          </label>
          <p className="field-hint">{t("Wichtig: Ein vergessenes Kennwort lässt sich nicht wiederherstellen.")}</p>
        </Modal>
      );
    case "compress":
      return (
        <Modal open onClose={onClose} icon={<Minimize2 size={18} />} title={t("PDF komprimieren")} footer={<>{cancel}<Btn kind="primary" onClick={() => run({ compress: level })}>{t("Komprimieren")}</Btn></>}>
          <p className="muted">{t("Aktuelle Größe: {0}", fmtSize(size))}</p>
          {(
            [
              ["lossless", t("Optimieren (verlustfrei)"), t("Räumt die Dateistruktur auf. Text bleibt durchsuchbar.")],
              ["medium", t("Mittel (150 dpi)"), t("Seiten werden als Bilder neu aufgebaut – deutlich kleiner, Text nicht mehr markierbar.")],
              ["strong", t("Stark (96 dpi)"), t("Kleinste Datei, sichtbar geringere Qualität. Ideal für E-Mail.")],
            ] as const
          ).map(([v, title, desc]) => (
            <label key={v} className={"choice" + (level === v ? " is-active" : "")}>
              <input type="radio" checked={level === v} onChange={() => setLevel(v)} />
              <div>
                <b>{title}</b>
                <span>{desc}</span>
              </div>
            </label>
          ))}
        </Modal>
      );
    case "meta":
      return (
        <Modal open onClose={onClose} icon={<Info size={18} />} title={t("Dokumenteigenschaften")} footer={<>{cancel}<Btn kind="primary" onClick={() => run({ meta: m })}>{t("Speichern")}</Btn></>}>
          {(["title", "author", "subject", "keywords"] as const).map((k) => (
            <Field key={k} label={{ title: t("Titel"), author: t("Autor"), subject: t("Thema"), keywords: t("Stichwörter") }[k]}>
              <input className="input" value={(m as any)[k] ?? ""} onChange={(e) => setM({ ...m, [k]: e.target.value })} onKeyDown={(e) => e.stopPropagation()} />
            </Field>
          ))}
          <div className="meta-info">
            <div><span>{t("Seiten")}</span><b>{total}</b></div>
            <div><span>{t("Größe")}</span><b>{fmtSize(size)}</b></div>
            {meta?.creator && <div><span>{t("Erstellt mit")}</span><b>{meta.creator}</b></div>}
            {meta?.producer && <div><span>{t("PDF-Erzeuger")}</span><b>{meta.producer}</b></div>}
            {meta?.created && <div><span>{t("Erstellt")}</span><b>{meta.created}</b></div>}
            {meta?.modified && <div><span>{t("Geändert")}</span><b>{meta.modified}</b></div>}
            <div><span>{t("Kennwortschutz")}</span><b>{encrypted ? t("Ja") : t("Nein")}</b></div>
          </div>
        </Modal>
      );
    case "images":
      return (
        <Modal open onClose={onClose} icon={<Images size={18} />} title={t("Als Bilder exportieren")} footer={<>{cancel}<Btn kind="primary" onClick={() => run({ images: img })}>{t("Exportieren")}</Btn></>}>
          <Field label={t("Format")}>
            <Segmented value={img.fmt} onChange={(v) => setImg({ ...img, fmt: v })} options={[{ value: "png", label: "PNG" }, { value: "jpeg", label: "JPG" }]} />
          </Field>
          <Field label={t("Auflösung")}>
            <Segmented value={String(img.dpi)} onChange={(v) => setImg({ ...img, dpi: +v })} options={[72, 150, 300].map((d) => ({ value: String(d), label: d + " dpi" }))} />
          </Field>
          <Field label={t("Seiten")} hint={t("Leer = alle {0} Seiten. Beispiel: 1-3, 5", total)}>
            <input className="input" value={img.range} placeholder={`1-${total}`} onChange={(e) => setImg({ ...img, range: e.target.value })} onKeyDown={(e) => e.stopPropagation()} />
          </Field>
          <p className="field-hint">{t("Die Bilder werden als ZIP-Archiv gespeichert.")}</p>
        </Modal>
      );
    case "split":
      return (
        <Modal open onClose={onClose} icon={<Scissors size={18} />} title={t("PDF teilen")} footer={<>{cancel}<Btn kind="primary" disabled={sp.mode === "ranges" && !sp.ranges.trim()} onClick={() => run({ split: sp })}>{t("Teilen")}</Btn></>}>
          <label className={"choice" + (sp.mode === "every" ? " is-active" : "")}>
            <input type="radio" checked={sp.mode === "every"} onChange={() => setSp({ ...sp, mode: "every" })} />
            <div>
              <b>{t("Alle N Seiten")}</b>
              <span className="row-gap">
                {t("Neue Datei alle")}
                <input className="input input-sm" type="number" min={1} max={total} value={sp.every} onChange={(e) => setSp({ ...sp, mode: "every", every: Math.max(1, +e.target.value || 1) })} />
                {t("Seiten → {0} Dateien", Math.ceil(total / sp.every))}
              </span>
            </div>
          </label>
          <label className={"choice" + (sp.mode === "ranges" ? " is-active" : "")}>
            <input type="radio" checked={sp.mode === "ranges"} onChange={() => setSp({ ...sp, mode: "ranges" })} />
            <div>
              <b>{t("Nach Bereichen")}</b>
              <input className="input" value={sp.ranges} placeholder="1-3, 4-7, 8-" onFocus={() => setSp({ ...sp, mode: "ranges" })} onChange={(e) => setSp({ ...sp, mode: "ranges", ranges: e.target.value })} onKeyDown={(e) => e.stopPropagation()} />
            </div>
          </label>
          <p className="field-hint">{t("Die Teile werden als neue Dokumente in deiner Bibliothek abgelegt.")}</p>
        </Modal>
      );
  }
}

export function fmtSize(n: number) {
  if (n < 1024) return n + " B";
  if (n < 1024 * 1024) return (n / 1024).toFixed(0) + " KB";
  return (n / 1024 / 1024).toFixed(1).replace(".", ",") + " MB";
}
