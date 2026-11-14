import { createContext, useCallback, useContext, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { X, Check, AlertTriangle, Loader2 } from "lucide-react";
import { tr } from "../i18n";

export const cx = (...c: (string | false | null | undefined)[]) => c.filter(Boolean).join(" ");

// ------------------------------------------------------------------ Schaltflächen
export function Btn({
  children,
  onClick,
  kind = "soft",
  disabled,
  className,
  type = "button",
  title,
}: {
  children: ReactNode;
  onClick?: () => void;
  kind?: "primary" | "soft" | "ghost" | "danger";
  disabled?: boolean;
  className?: string;
  type?: "button" | "submit";
  title?: string;
}) {
  return (
    <button type={type} title={title} disabled={disabled} onClick={onClick} className={cx("btn", "btn-" + kind, className)}>
      {children}
    </button>
  );
}

export function IconBtn({
  icon,
  label,
  onClick,
  active,
  disabled,
  className,
  badge,
  showLabel,
  tip,
  shortcut,
}: {
  icon: ReactNode;
  label: string;
  /** kurze Erklärung im Tooltip */
  tip?: string;
  /** Tastenkürzel im Tooltip, z. B. "Strg+Z" */
  shortcut?: string;
  onClick?: (e: React.MouseEvent) => void;
  active?: boolean;
  disabled?: boolean;
  className?: string;
  badge?: ReactNode;
  showLabel?: boolean;
}) {
  // "Rückgängig (Strg+Z)" → Beschriftung + Tastenkürzel getrennt anzeigen
  const m = !shortcut ? label.match(/^(.*?)\s*\(((?:Strg|Ctrl|⌘|Umschalt|Shift)?[^()]{1,12})\)$/) : null;
  if (m) {
    label = m[1];
    shortcut = m[2];
  }
  if (shortcut && /Mac/.test(navigator.platform)) shortcut = shortcut.replace(/Strg\+|Ctrl\+/, "⌘");
  return (
    <button
      type="button"
      aria-label={label}
      data-tip={label}
      data-tip-desc={tip}
      data-tip-key={shortcut}
      disabled={disabled}
      onClick={onClick}
      className={cx("iconbtn", active && "is-active", showLabel && "with-label", className)}
    >
      {icon}
      {showLabel && <span className="iconbtn-label">{label}</span>}
      {badge != null && <span className="iconbtn-badge">{badge}</span>}
    </button>
  );
}

// ------------------------------------------------------------------ Dialog (auf dem Handy als Sheet von unten)
export function Modal({
  open,
  onClose,
  title,
  children,
  footer,
  wide,
  icon,
}: {
  open: boolean;
  onClose: () => void;
  title: string;
  children: ReactNode;
  footer?: ReactNode;
  wide?: boolean;
  icon?: ReactNode;
}) {
  const [closing, setClosing] = useState(false);
  const close = useCallback(() => {
    setClosing(true);
    setTimeout(() => {
      setClosing(false);
      onClose();
    }, 160);
  }, [onClose]);
  useEffect(() => {
    if (!open) return;
    const k = (e: KeyboardEvent) => e.key === "Escape" && close();
    addEventListener("keydown", k);
    return () => removeEventListener("keydown", k);
  }, [open, close]);
  if (!open) return null;
  return createPortal(
    <div className={cx("modal-backdrop", closing && "closing")} onMouseDown={(e) => e.target === e.currentTarget && close()}>
      <div className={cx("modal", wide && "modal-wide")} role="dialog" aria-label={title}>
        <div className="modal-grip" />
        <header className="modal-head">
          {icon && <span className="modal-icon">{icon}</span>}
          <h2>{title}</h2>
          <button className="iconbtn" aria-label={tr("Schließen")} onClick={close}>
            <X size={18} />
          </button>
        </header>
        <div className="modal-body">{children}</div>
        {footer && <footer className="modal-foot">{footer}</footer>}
      </div>
    </div>,
    document.body,
  );
}

// ------------------------------------------------------------------ Popover-Menü
export interface MenuItem {
  label: string;
  icon?: ReactNode;
  onClick?: () => void;
  danger?: boolean;
  disabled?: boolean;
  hint?: string;
  divider?: boolean;
}

export function Menu({ anchor, items, onClose, align = "right" }: { anchor: HTMLElement | null; items: MenuItem[]; onClose: () => void; align?: "left" | "right" }) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ top: number; left: number } | null>(null);
  useLayoutEffect(() => {
    if (!anchor || !ref.current) return;
    const r = anchor.getBoundingClientRect();
    const m = ref.current.getBoundingClientRect();
    let left = align === "right" ? r.right - m.width : r.left;
    left = Math.max(8, Math.min(left, innerWidth - m.width - 8));
    let top = r.bottom + 6;
    if (top + m.height > innerHeight - 8) top = Math.max(8, r.top - m.height - 6);
    setPos({ top, left });
  }, [anchor, align]);
  useEffect(() => {
    const down = (e: PointerEvent) => {
      if (!ref.current?.contains(e.target as Node) && !anchor?.contains(e.target as Node)) onClose();
    };
    const k = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    addEventListener("pointerdown", down, true);
    addEventListener("keydown", k);
    return () => {
      removeEventListener("pointerdown", down, true);
      removeEventListener("keydown", k);
    };
  }, [anchor, onClose]);
  if (!anchor) return null;
  return createPortal(
    <div ref={ref} className="menu" style={pos ? { top: pos.top, left: pos.left } : { visibility: "hidden", top: 0, left: 0 }}>
      {items.map((it, i) =>
        it.divider ? (
          <div key={i} className="menu-divider" />
        ) : (
          <button
            key={i}
            disabled={it.disabled}
            className={cx("menu-item", it.danger && "danger")}
            onClick={() => {
              onClose();
              it.onClick?.();
            }}
          >
            <span className="menu-icon">{it.icon}</span>
            <span className="menu-label">{it.label}</span>
            {it.hint && <span className="menu-hint">{it.hint}</span>}
          </button>
        ),
      )}
    </div>,
    document.body,
  );
}

export function useMenu() {
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);
  return {
    anchor,
    open: (e: React.MouseEvent) => setAnchor(anchor ? null : (e.currentTarget as HTMLElement)),
    close: () => setAnchor(null),
  };
}

// ------------------------------------------------------------------ Hinweise (Toasts) & Fortschritt
type Toast = { id: number; text: string; kind: "ok" | "err" | "info"; action?: { label: string; run: () => void } };
const ToastCtx = createContext<{
  toast: (text: string, kind?: Toast["kind"], action?: Toast["action"]) => void;
  busy: (text: string | null, progress?: number) => void;
}>({ toast: () => {}, busy: () => {} });

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [busy, setBusy] = useState<{ text: string; progress?: number } | null>(null);
  const toast = useCallback((text: string, kind: Toast["kind"] = "ok", action?: Toast["action"]) => {
    const id = Date.now() + Math.random();
    setToasts((t) => [...t.slice(-2), { id, text, kind, action }]);
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), action ? 6000 : 3200);
  }, []);
  const setBusyCb = useCallback((text: string | null, progress?: number) => setBusy(text ? { text, progress } : null), []);
  return (
    <ToastCtx.Provider value={{ toast, busy: setBusyCb }}>
      {children}
      {createPortal(
        <div className="toasts">
          {toasts.map((t) => (
            <div key={t.id} className={cx("toast", "toast-" + t.kind)}>
              {t.kind === "err" ? <AlertTriangle size={16} /> : <Check size={16} />}
              <span>{t.text}</span>
              {t.action && (
                <button
                  className="toast-action"
                  onClick={() => {
                    t.action!.run();
                    setToasts((x) => x.filter((y) => y.id !== t.id));
                  }}
                >
                  {t.action.label}
                </button>
              )}
            </div>
          ))}
        </div>,
        document.body,
      )}
      {busy &&
        createPortal(
          <div className="busy">
            <div className="busy-card">
              <Loader2 className="spin" size={22} />
              <span>{busy.text}</span>
              {busy.progress != null && (
                <div className="busy-bar">
                  <div style={{ width: `${Math.round(busy.progress * 100)}%` }} />
                </div>
              )}
            </div>
          </div>,
          document.body,
        )}
    </ToastCtx.Provider>
  );
}

export const useToast = () => useContext(ToastCtx);

// ------------------------------------------------------------------ Formularelemente
export function Field({ label, children, hint }: { label: string; children: ReactNode; hint?: string }) {
  return (
    <label className="field">
      <span className="field-label">{label}</span>
      {children}
      {hint && <span className="field-hint">{hint}</span>}
    </label>
  );
}

export function Segmented<T extends string>({
  value,
  options,
  onChange,
  small,
}: {
  value: T;
  options: { value: T; label: ReactNode; title?: string }[];
  onChange: (v: T) => void;
  small?: boolean;
}) {
  return (
    <div className={cx("seg", small && "seg-small")}>
      {options.map((o) => (
        <button key={o.value} data-tip={o.title} aria-label={o.title} className={cx(o.value === value && "is-active")} onClick={() => onChange(o.value)}>
          {o.label}
        </button>
      ))}
    </div>
  );
}

export const PALETTE = ["#f6c945", "#9bd37a", "#7cc4f0", "#f29bc0", "#f0905a", "#c58cf2", "#d64545", "#2f6bd6", "#3d5639", "#1f1f1f", "#ffffff"];

export function Swatches({ value, onChange, colors = PALETTE }: { value: string; onChange: (c: string) => void; colors?: string[] }) {
  return (
    <div className="swatches">
      {colors.map((c) => (
        <button
          key={c}
          aria-label={c}
          className={cx("swatch", c.toLowerCase() === value.toLowerCase() && "is-active")}
          style={{ background: c }}
          onClick={() => onChange(c)}
        />
      ))}
      <label className="swatch swatch-custom" title={tr("Eigene Farbe")}>
        <input type="color" value={value} onChange={(e) => onChange(e.target.value)} />
      </label>
    </div>
  );
}

export function Range({ value, min, max, step = 1, onChange, suffix }: { value: number; min: number; max: number; step?: number; onChange: (v: number) => void; suffix?: string }) {
  return (
    <div className="range">
      <input type="range" min={min} max={max} step={step} value={value} onChange={(e) => onChange(+e.target.value)} />
      <span className="range-val">
        {Number.isInteger(step) ? value : value.toFixed(1)}
        {suffix}
      </span>
    </div>
  );
}

export function Empty({ icon, title, text, action }: { icon: ReactNode; title: string; text?: string; action?: ReactNode }) {
  return (
    <div className="empty">
      <div className="empty-icon">{icon}</div>
      <h3>{title}</h3>
      {text && <p>{text}</p>}
      {action}
    </div>
  );
}

/** Kleines Bestätigungsfenster per Promise */
export function useConfirm() {
  const [state, setState] = useState<{ title: string; text: string; ok: string; danger?: boolean; resolve: (v: boolean) => void } | null>(null);
  const confirm = (title: string, text: string, ok = "OK", danger = false) =>
    new Promise<boolean>((resolve) => setState({ title, text, ok, danger, resolve }));
  const node = state ? (
    <Modal
      open
      title={state.title}
      onClose={() => {
        state.resolve(false);
        setState(null);
      }}
      footer={
        <>
          <Btn
            kind="ghost"
            onClick={() => {
              state.resolve(false);
              setState(null);
            }}
          >
            {tr("Abbrechen")}
          </Btn>
          <Btn
            kind={state.danger ? "danger" : "primary"}
            onClick={() => {
              state.resolve(true);
              setState(null);
            }}
          >
            {state.ok}
          </Btn>
        </>
      }
    >
      <p className="muted">{state.text}</p>
    </Modal>
  ) : null;
  return { confirm, node };
}

// ------------------------------------------------------------------ Tooltips
// Eine einzige, schwebende Ebene über allem – wird nicht von scrollbaren Leisten abgeschnitten.
// Zeigt sich bei Mauszeigern für jedes Element mit data-tip oder für Symbol-Schaltflächen mit aria-label.
export function TooltipLayer() {
  const [tip, setTip] = useState<{ label: string; desc?: string; key?: string; x: number; y: number; below: boolean } | null>(null);
  const timer = useRef<number>(0);
  const current = useRef<HTMLElement | null>(null);
  const boxRef = useRef<HTMLDivElement>(null);
  const [shift, setShift] = useState(0);
  useEffect(() => {
    const target = (e: Event) => (e.target as HTMLElement)?.closest?.("[data-tip], button[aria-label]") as HTMLElement | null;
    const hide = () => {
      clearTimeout(timer.current);
      current.current = null;
      setTip(null);
    };
    const over = (e: PointerEvent) => {
      if (e.pointerType !== "mouse") return;
      const el = target(e);
      if (el === current.current) return;
      clearTimeout(timer.current);
      current.current = el;
      if (!el) return setTip(null);
      const label = el.dataset.tip || el.getAttribute("aria-label") || "";
      if (!label) return setTip(null);
      const delay = tipVisible ? 60 : 450;
      timer.current = window.setTimeout(() => {
        if (current.current !== el || !el.isConnected) return;
        const r = el.getBoundingClientRect();
        const below = r.bottom + 70 < innerHeight;
        setShift(0);
        setTip({ label, desc: el.dataset.tipDesc, key: el.dataset.tipKey, x: r.left + r.width / 2, y: below ? r.bottom + 8 : r.top - 8, below });
      }, delay);
    };
    addEventListener("pointerover", over, true);
    addEventListener("pointerdown", hide, true);
    addEventListener("scroll", hide, true);
    addEventListener("keydown", hide, true);
    addEventListener("blur", hide);
    return () => {
      removeEventListener("pointerover", over, true);
      removeEventListener("pointerdown", hide, true);
      removeEventListener("scroll", hide, true);
      removeEventListener("keydown", hide, true);
      removeEventListener("blur", hide);
    };
  }, []);
  tipVisible = !!tip;
  // am Bildschirmrand nicht abschneiden
  useLayoutEffect(() => {
    const b = boxRef.current?.getBoundingClientRect();
    if (!b) return;
    if (b.left < 8) setShift(8 - b.left);
    else if (b.right > innerWidth - 8) setShift(innerWidth - 8 - b.right);
  }, [tip]);
  if (!tip) return null;
  return createPortal(
    <div
      ref={boxRef}
      className={cx("tooltip", tip.below ? "tooltip-below" : "tooltip-above")}
      style={{ left: tip.x + shift, top: tip.y }}
      role="tooltip"
    >
      <div className="tooltip-head">
        <b>{tip.label}</b>
        {tip.key && <kbd>{tip.key}</kbd>}
      </div>
      {tip.desc && <span>{tip.desc}</span>}
    </div>,
    document.body,
  );
}
let tipVisible = false;
