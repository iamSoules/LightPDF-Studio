// Leseansicht für Word (.docx) und EPUB – Inhalte werden vor der Anzeige bereinigt.
import { useEffect, useMemo, useRef, useState } from "react";
import { ArrowLeft, Search, Minus, Plus, Moon, Sun, Coffee, Volume2, VolumeX, X, List } from "lucide-react";
import { db } from "../lib/db";
import { parseDocxFile, parseEpubFile } from "../lib/docformats";
import type { DocMeta, ParsedDocument, Settings } from "../types";
import { IconBtn, cx, useToast } from "./ui";
import { useT } from "../i18n";

/** Entfernt Skripte, Event-Handler und javascript:-Links */
export function sanitize(html: string) {
  const doc = new DOMParser().parseFromString(`<div>${html}</div>`, "text/html");
  doc.querySelectorAll("script,iframe,object,embed,link,meta,base,form,input,button,textarea,select,frame,frameset,noscript").forEach((n) => n.remove());
  doc.querySelectorAll("*").forEach((el) => {
    for (const a of Array.from(el.attributes)) {
      const n = a.name.toLowerCase();
      const v = a.value.trim().toLowerCase();
      if (n.startsWith("on") || ((n === "href" || n === "src" || n === "xlink:href" || n === "action") && (v.startsWith("javascript:") || (v.startsWith("data:") && !v.startsWith("data:image/")))))
        el.removeAttribute(a.name);
    }
    if (el.tagName === "A") {
      el.setAttribute("target", "_blank");
      el.setAttribute("rel", "noopener noreferrer");
    }
  });
  return doc.body.firstElementChild!.innerHTML;
}

export default function HtmlReader({ meta, settings, setSettings, onClose }: { meta: DocMeta; settings: Settings; setSettings: (s: Partial<Settings>) => void; onClose: () => void }) {
  const t = useT();
  const { toast } = useToast();
  const [doc, setDoc] = useState<ParsedDocument | null>(null);
  const [size, setSize] = useState(18);
  const [q, setQ] = useState("");
  const [showSearch, setShowSearch] = useState(false);
  const [showToc, setShowToc] = useState(false);
  const [page, setPage] = useState(meta.lastPage || 1);
  const [speaking, setSpeaking] = useState(false);
  const scroller = useRef<HTMLDivElement>(null);

  useEffect(() => {
    (async () => {
      const raw = await db.getFile(meta.id);
      if (!raw) return onClose();
      const file = new File([raw], meta.name);
      try {
        const d = meta.format === "epub" ? await parseEpubFile(file) : await parseDocxFile(file);
        d.pages = d.pages.map((p) => ({ ...p, htmlContent: sanitize(p.htmlContent ?? "") }));
        setDoc(d);
        await db.putDoc({ ...meta, pages: d.totalPages, openedAt: Date.now() });
        setTimeout(() => scroller.current?.querySelector(`[data-page="${meta.lastPage}"]`)?.scrollIntoView(), 50);
      } catch (e) {
        toast(t("Konnte das Dokument nicht öffnen: {0}", (e as Error).message), "err");
        onClose();
      }
    })();
    (window as any).handleAndroidHardwareBack = () => (onClose(), true);
    return () => {
      window.speechSynthesis?.cancel();
      delete (window as any).handleAndroidHardwareBack;
    };
  }, []);

  // aktuelle Seite merken
  useEffect(() => {
    const el = scroller.current;
    if (!el || !doc) return;
    const io = new IntersectionObserver(
      (entries) => {
        for (const e of entries) if (e.isIntersecting) setPage(+(e.target as HTMLElement).dataset.page!);
      },
      { root: el, threshold: 0.4 },
    );
    el.querySelectorAll("[data-page]").forEach((n) => io.observe(n));
    return () => io.disconnect();
  }, [doc]);
  useEffect(() => {
    const id = setTimeout(() => db.getDoc(meta.id).then((m) => m && db.putDoc({ ...m, lastPage: page })), 600);
    return () => clearTimeout(id);
  }, [page]);

  const html = useMemo(() => {
    if (!doc) return [];
    if (!q.trim()) return doc.pages.map((p) => p.htmlContent ?? "");
    const rx = new RegExp(`(${q.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")})`, "gi");
    return doc.pages.map((p) => (p.htmlContent ?? "").replace(/>([^<]+)</g, (_m, txt) => ">" + txt.replace(rx, "<mark>$1</mark>") + "<"));
  }, [doc, q]);
  const hits = useMemo(() => (q.trim() ? html.reduce((n, h) => n + (h.match(/<mark>/g)?.length ?? 0), 0) : 0), [html, q]);

  const speak = () => {
    const synth = window.speechSynthesis;
    if (!synth) return toast(t("Vorlesen wird auf diesem Gerät nicht unterstützt."), "info");
    if (speaking) {
      synth.cancel();
      return setSpeaking(false);
    }
    const text = doc!.pages.slice(page - 1).map((p) => p.textContent || new DOMParser().parseFromString(p.htmlContent ?? "", "text/html").body.textContent).join("\n");
    const u = new SpeechSynthesisUtterance(text);
    u.lang = settings.lang === "de" ? "de-DE" : "en-US";
    u.onend = () => setSpeaking(false);
    synth.speak(u);
    setSpeaking(true);
  };

  const chapters = doc?.pages.filter((p, i, a) => p.title && p.title !== a[i - 1]?.title) ?? [];

  return (
    <div className={cx("reader", "html-reader")}>
      <header className="reader-top">
        <IconBtn icon={<ArrowLeft size={20} />} label={t("Zurück")} onClick={onClose} />
        <div className="reader-title">
          <b>{doc?.title ?? meta.name}</b>
          <span>
            {meta.format.toUpperCase()} · {doc ? t("Seite {0} von {1}", page, doc.totalPages) : "…"}
          </span>
        </div>
        <span className="grow" />
        {chapters.length > 1 && <IconBtn icon={<List size={19} />} label={t("Inhalt")} active={showToc} onClick={() => setShowToc(!showToc)} />}
        <IconBtn icon={<Search size={19} />} label={t("Suchen")} active={showSearch} onClick={() => setShowSearch(!showSearch)} />
        <IconBtn icon={<Minus size={19} />} label={t("Schrift kleiner")} onClick={() => setSize((s) => Math.max(12, s - 2))} />
        <IconBtn icon={<Plus size={19} />} label={t("Schrift größer")} onClick={() => setSize((s) => Math.min(32, s + 2))} />
        <IconBtn
          icon={settings.reading === "night" ? <Moon size={19} /> : settings.reading === "sepia" ? <Coffee size={19} /> : <Sun size={19} />}
          label={t("Lesemodus")}
          onClick={() => setSettings({ reading: settings.reading === "normal" ? "sepia" : settings.reading === "sepia" ? "night" : "normal" })}
        />
        <IconBtn icon={speaking ? <VolumeX size={19} /> : <Volume2 size={19} />} label={t("Vorlesen")} active={speaking} onClick={speak} />
      </header>
      {showSearch && (
        <div className="html-search">
          <Search size={16} />
          <input autoFocus value={q} onChange={(e) => setQ(e.target.value)} placeholder={t("Im Dokument suchen …")} />
          <span className="muted">{q ? t("{0} Treffer", hits) : ""}</span>
          <button className="iconbtn iconbtn-sm" onClick={() => (setQ(""), setShowSearch(false))} aria-label={t("Schließen")}>
            <X size={15} />
          </button>
        </div>
      )}
      <div className="reader-body">
        {showToc && (
          <aside className="sidepanel">
            <div className="sidepanel-body">
              <ul className="outline">
                {chapters.map((c) => (
                  <li key={c.pageNumber}>
                    <button className="outline-title" onClick={() => scroller.current?.querySelector(`[data-page="${c.pageNumber}"]`)?.scrollIntoView({ behavior: "smooth" })}>
                      {c.title}
                    </button>
                  </li>
                ))}
              </ul>
            </div>
          </aside>
        )}
        <div ref={scroller} className={cx("html-scroll", "reading-" + settings.reading)}>
          {html.map((h, i) => (
            <article key={i} data-page={i + 1} className="html-page" style={{ fontSize: size }} dangerouslySetInnerHTML={{ __html: h }} />
          ))}
        </div>
      </div>
    </div>
  );
}
