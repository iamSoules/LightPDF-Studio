import { useEffect, useMemo, useRef, useState } from "react";
import {
  Search, Settings as SettingsIcon, Plus, FolderPlus, Star, MoreVertical, LayoutGrid, List, FileText, Trash2, Pencil, FolderInput, Share2,
  Download, Combine, ImagePlus, FilePlus2, Upload, CheckSquare, X, ArrowUp, ArrowDown, BookOpen, Lock, Folder as FolderIcon, Clock, ChevronDown,
  Camera, Sparkles, HardDrive, Info, Globe, Palette, User, Minimize2, PenLine, Scissors,
} from "lucide-react";
import type { DocMeta, Folder, Settings } from "../types";
import { db, uid } from "../lib/db";
import { importFiles, addPdf } from "../lib/library";
import * as ops from "../lib/pdfops";
import { fileFromDrop, isAndroid, isDesktop, pickFiles, saveAs, share } from "../lib/platform";
import { Btn, Empty, Field, IconBtn, Menu, Modal, Segmented, cx, useConfirm, useMenu, useToast, type MenuItem } from "./ui";
import { fmtSize } from "./ToolDialogs";
import { ThemeToggle } from "./Reader";
import { useT } from "../i18n";

const FOLDER_COLORS = ["#6d8a64", "#c98b4a", "#5b8cc9", "#b86aa8", "#c65f5f", "#8c7a5b"];
const ACCEPT = ".pdf,.docx,.epub,application/pdf,image/*";

export default function Library({
  docs,
  folders,
  settings,
  setSettings,
  reload,
  onOpen,
}: {
  docs: DocMeta[];
  folders: Folder[];
  settings: Settings;
  setSettings: (s: Partial<Settings>) => void;
  reload: () => Promise<void>;
  onOpen: (id: string) => void;
}) {
  const t = useT();
  const { toast, busy } = useToast();
  const { confirm, node: confirmNode } = useConfirm();
  const [q, setQ] = useState("");
  const [folder, setFolder] = useState<string>("all"); // all | starred | recent | <id>
  const [selecting, setSelecting] = useState(false);
  const [sel, setSel] = useState<Set<string>>(new Set());
  const [menuFor, setMenuFor] = useState<{ doc: DocMeta; anchor: HTMLElement } | null>(null);
  const [rename, setRename] = useState<DocMeta | null>(null);
  const [moveDocs, setMoveDocs] = useState<string[] | null>(null);
  const [newFolder, setNewFolder] = useState(false);
  const [mergeList, setMergeList] = useState<DocMeta[] | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [dragOver, setDragOver] = useState(false);
  const addMenu = useMenu();
  const sortMenu = useMenu();
  const folderMenu = useMenu();
  const [folderCtx, setFolderCtx] = useState<Folder | null>(null);

  // ------------------------------------------------------------------ Filtern & Sortieren
  const list = useMemo(() => {
    let l = docs;
    if (folder === "starred") l = l.filter((d) => d.starred);
    else if (folder === "recent") l = [...l].sort((a, b) => b.openedAt - a.openedAt).slice(0, 20);
    else if (folder !== "all") l = l.filter((d) => d.folderId === folder);
    if (q.trim()) {
      const s = q.toLowerCase();
      l = l.filter((d) => d.name.toLowerCase().includes(s));
    }
    const sorters: Record<Settings["sort"], (a: DocMeta, b: DocMeta) => number> = {
      recent: (a, b) => b.openedAt - a.openedAt,
      added: (a, b) => b.addedAt - a.addedAt,
      name: (a, b) => a.name.localeCompare(b.name, undefined, { numeric: true }),
      size: (a, b) => b.size - a.size,
    };
    return folder === "recent" ? l : [...l].sort(sorters[settings.sort]);
  }, [docs, folder, q, settings.sort]);

  const recent = useMemo(() => [...docs].sort((a, b) => b.openedAt - a.openedAt).slice(0, 8), [docs]);

  // ------------------------------------------------------------------ Import
  const doImport = async (files: { name: string; data: Uint8Array; path?: string }[], open = true) => {
    if (!files.length) return;
    const unsupported = files.filter((f) => !/\.(pdf|docx|epub|png|jpe?g|webp|gif|bmp|heic)$/i.test(f.name));
    if (unsupported.length) toast(t("{0} Datei(en) werden nicht unterstützt", unsupported.length), "err");
    try {
      busy(t("Importiere …"));
      const metas = await importFiles(files, folder.length > 10 ? folder : null, (i, n) => busy(t("Importiere {0} von {1} …", i, n), i / n));
      busy(null);
      await reload();
      if (open && metas.length === 1) onOpen(metas[0].id);
      else if (metas.length) toast(t("{0} Dokument(e) hinzugefügt", metas.length));
    } catch (e) {
      busy(null);
      toast(t("Import fehlgeschlagen: {0}", (e as Error).message), "err");
    }
  };

  const openFiles = async () => doImport(await pickFiles(ACCEPT, true));
  const fromImages = async () => {
    const files = await pickFiles("image/*", true);
    if (files.length) doImport(files);
  };
  const blank = async () => {
    const m = await addPdf(t("Neues Dokument") + ".pdf", await ops.blankPdf(1));
    await reload();
    onOpen(m.id);
  };

  // Drag & Drop
  useEffect(() => {
    const over = (e: DragEvent) => {
      if (e.dataTransfer?.types.includes("Files")) {
        e.preventDefault();
        setDragOver(true);
      }
    };
    const leave = (e: DragEvent) => {
      if (!e.relatedTarget) setDragOver(false);
    };
    const drop = async (e: DragEvent) => {
      e.preventDefault();
      setDragOver(false);
      const files = Array.from(e.dataTransfer?.files ?? []);
      doImport(await Promise.all(files.map(fileFromDrop)));
    };
    addEventListener("dragover", over);
    addEventListener("dragleave", leave);
    addEventListener("drop", drop);
    return () => {
      removeEventListener("dragover", over);
      removeEventListener("dragleave", leave);
      removeEventListener("drop", drop);
    };
  });

  // ------------------------------------------------------------------ Aktionen
  const update = async (d: DocMeta, patch: Partial<DocMeta>) => {
    await db.putDoc({ ...d, ...patch });
    await reload();
  };

  const exportDoc = async (d: DocMeta, how: "save" | "share") => {
    const raw = await db.getFile(d.id);
    if (!raw) return;
    let bytes = new Uint8Array(raw);
    if (d.format === "pdf") {
      const annots = await db.getAnnots(d.id);
      if (annots.length && !d.encrypted) bytes = await ops.exportPdf(bytes, annots);
    }
    if (how === "share") await share(d.name, bytes);
    else {
      const r = await saveAs(d.name, bytes);
      if (r) toast(isAndroid ? t("Gespeichert in {0}", r.label) : t("Gespeichert"));
    }
  };

  const remove = async (ids: string[]) => {
    const ok = await confirm(
      ids.length > 1 ? t("{0} Dokumente löschen?", ids.length) : t("Dokument löschen?"),
      t("Dokumente und Kommentare werden aus der Bibliothek entfernt. Dateien auf der Festplatte bleiben unberührt."),
      t("Löschen"),
      true,
    );
    if (!ok) return;
    for (const id of ids) await db.deleteDoc(id);
    setSel(new Set());
    setSelecting(false);
    await reload();
  };

  const docMenu = (d: DocMeta): MenuItem[] => [
    { label: t("Öffnen"), icon: <BookOpen size={16} />, onClick: () => onOpen(d.id) },
    { label: d.starred ? t("Markierung entfernen") : t("Markieren"), icon: <Star size={16} />, onClick: () => update(d, { starred: !d.starred }) },
    { label: t("Umbenennen"), icon: <Pencil size={16} />, onClick: () => setRename(d) },
    { label: t("In Ordner verschieben"), icon: <FolderInput size={16} />, onClick: () => setMoveDocs([d.id]) },
    { divider: true, label: "" },
    { label: isAndroid ? t("In Dokumente speichern") : t("Speichern unter …"), icon: <Download size={16} />, onClick: () => exportDoc(d, "save") },
    ...(isAndroid || (navigator as any).canShare ? [{ label: t("Teilen"), icon: <Share2 size={16} />, onClick: () => exportDoc(d, "share") }] : []),
    { divider: true, label: "" },
    { label: t("Löschen"), icon: <Trash2 size={16} />, danger: true, onClick: () => remove([d.id]) },
  ];

  const toggleSel = (id: string) => {
    const s = new Set(sel);
    s.has(id) ? s.delete(id) : s.add(id);
    setSel(s);
  };

  const merge = async (items: DocMeta[], name: string) => {
    const locked = items.filter((d) => d.encrypted);
    if (locked.length) {
      toast(t("„{0}“ ist kennwortgeschützt. Bitte zuerst öffnen und unter Werkzeuge → „Kennwort entfernen“ entsperren.", locked[0].name), "err");
      return;
    }
    try {
      busy(t("Führe zusammen …"));
      const parts: Uint8Array[] = [];
      for (const d of items) {
        const raw = await db.getFile(d.id);
        if (!raw) continue;
        const annots = await db.getAnnots(d.id);
        parts.push(annots.length ? await ops.exportPdf(new Uint8Array(raw), annots) : new Uint8Array(raw));
      }
      const pdf = await ops.merge(parts);
      const m = await addPdf(name.endsWith(".pdf") ? name : name + ".pdf", pdf);
      busy(null);
      setMergeList(null);
      setSelecting(false);
      setSel(new Set());
      await reload();
      toast(t("{0} Dateien zusammengeführt", items.length), "ok", { label: t("Öffnen"), run: () => onOpen(m.id) });
    } catch (e) {
      busy(null);
      toast(t("Fehler: {0}", (e as Error).message), "err");
    }
  };

  const mergeFromDisk = async () => {
    const files = await pickFiles(".pdf,application/pdf", true);
    if (files.length < 2) {
      if (files.length) toast(t("Bitte mindestens zwei PDFs auswählen."), "info");
      return;
    }
    const metas = await importFiles(files);
    await reload();
    setMergeList(metas);
  };

  const folderName = folder === "all" ? t("Alle Dokumente") : folder === "starred" ? t("Markiert") : folder === "recent" ? t("Zuletzt geöffnet") : folders.find((f) => f.id === folder)?.name ?? "";
  const sortLabels: Record<Settings["sort"], string> = { recent: t("Zuletzt geöffnet"), added: t("Hinzugefügt"), name: t("Name"), size: t("Größe") };

  const tools = [
    { icon: Upload, label: t("PDF öffnen"), desc: t("PDF, Word oder EPUB"), run: openFiles },
    { icon: Combine, label: t("Zusammenführen"), desc: t("Mehrere PDFs zu einem"), run: mergeFromDisk },
    { icon: ImagePlus, label: t("Bilder → PDF"), desc: t("Fotos & Scans umwandeln"), run: fromImages },
    { icon: FilePlus2, label: t("Leeres PDF"), desc: t("Neues Dokument anlegen"), run: blank },
  ];

  return (
    <div className={cx("library", dragOver && "is-dragover")}>
      <aside className="lib-side">
        <div className="brand">
          <img src="./lightpdf_iconsvg.svg" alt="" />
          <div>
            <b>LightPDF</b>
            <span>Studio</span>
          </div>
        </div>
        <Btn kind="primary" className="lib-new" onClick={openFiles}>
          <Plus size={17} /> {t("Datei öffnen")}
        </Btn>
        <nav className="lib-nav">
          {[
            { id: "all", icon: FileText, label: t("Alle Dokumente"), n: docs.length },
            { id: "recent", icon: Clock, label: t("Zuletzt geöffnet") },
            { id: "starred", icon: Star, label: t("Markiert"), n: docs.filter((d) => d.starred).length },
          ].map((n) => (
            <button key={n.id} className={cx(folder === n.id && "is-active")} onClick={() => setFolder(n.id)}>
              <n.icon size={17} /> <span>{n.label}</span>
              {!!n.n && <i>{n.n}</i>}
            </button>
          ))}
          <div className="lib-nav-head">
            {t("Ordner")}
            <button className="iconbtn iconbtn-sm" onClick={() => setNewFolder(true)} aria-label={t("Neuer Ordner")} data-tip-desc={t("Ordner zum Sortieren deiner Dokumente anlegen")}>
              <FolderPlus size={15} />
            </button>
          </div>
          {folders.map((f) => (
            <button
              key={f.id}
              className={cx(folder === f.id && "is-active")}
              onClick={() => setFolder(f.id)}
              onContextMenu={(e) => {
                e.preventDefault();
                setFolderCtx(f);
                folderMenu.open(e);
              }}
            >
              <FolderIcon size={17} style={{ color: f.color }} /> <span>{f.name}</span>
              <i>{docs.filter((d) => d.folderId === f.id).length || ""}</i>
            </button>
          ))}
          {!folders.length && <p className="lib-nav-empty">{t("Noch keine Ordner")}</p>}
        </nav>
        <div className="lib-side-foot">
          <button className="lib-settings" onClick={() => setShowSettings(true)}>
            <SettingsIcon size={17} /> {t("Einstellungen")}
          </button>
          <ThemeToggle settings={settings} setSettings={setSettings} />
        </div>
      </aside>

      <main className="lib-main">
        <header className="lib-head">
          <div className="lib-head-mobile">
            <img src="./lightpdf_iconsvg.svg" alt="" />
            <div>
              <span>LightPDF Studio</span>
              <b>{folderName}</b>
            </div>
            <span className="grow" />
            <ThemeToggle settings={settings} setSettings={setSettings} />
            <IconBtn icon={<SettingsIcon size={19} />} label={t("Einstellungen")} onClick={() => setShowSettings(true)} />
          </div>
          <h1 className="lib-title">{folderName}</h1>
          <div className="lib-search">
            <Search size={17} />
            <input value={q} onChange={(e) => setQ(e.target.value)} placeholder={t("Dokumente suchen …")} />
            {q && (
              <button className="iconbtn iconbtn-sm" onClick={() => setQ("")} aria-label={t("Leeren")}>
                <X size={15} />
              </button>
            )}
          </div>
          <div className="lib-chips">
            {[
              { id: "all", label: t("Alle") },
              { id: "starred", label: "★ " + t("Markiert") },
              ...folders.map((f) => ({ id: f.id, label: f.name, color: f.color })),
            ].map((c: any) => (
              <button key={c.id} className={cx("chip", folder === c.id && "is-active")} onClick={() => setFolder(c.id)}>
                {c.color && <i className="chip-dot" style={{ background: c.color }} />}
                {c.label}
              </button>
            ))}
            <button className="chip chip-add" onClick={() => setNewFolder(true)}>
              <FolderPlus size={14} /> {t("Ordner")}
            </button>
          </div>
        </header>

        {!q && folder === "all" && (
          <section className="lib-tools">
            {tools.map((tl) => (
              <button key={tl.label} className="tool-card" onClick={tl.run}>
                <span className="tool-icon">
                  <tl.icon size={20} />
                </span>
                <b>{tl.label}</b>
                <span>{tl.desc}</span>
              </button>
            ))}
          </section>
        )}

        {!q && folder === "all" && recent.length > 2 && (
          <section className="lib-recent">
            <h2>{t("Zuletzt geöffnet")}</h2>
            <div className="recent-strip">
              {recent.map((d) => (
                <button key={d.id} className="recent-item" onClick={() => onOpen(d.id)}>
                  <Cover d={d} />
                  <span>{d.name.replace(/\.(pdf|docx|epub)$/i, "")}</span>
                  {d.pages > 0 && d.lastPage > 1 && <i className="recent-progress" style={{ width: `${(d.lastPage / d.pages) * 100}%` }} />}
                </button>
              ))}
            </div>
          </section>
        )}

        <section className="lib-docs">
          <div className="lib-docs-head">
            <h2>
              {list.length} {list.length === 1 ? t("Dokument") : t("Dokumente")}
            </h2>
            <span className="grow" />
            {selecting ? (
              <>
                <Btn kind="ghost" onClick={() => setSel(sel.size === list.length ? new Set() : new Set(list.map((d) => d.id)))}>
                  {sel.size === list.length ? t("Keine") : t("Alle")}
                </Btn>
                <Btn kind="ghost" onClick={() => (setSelecting(false), setSel(new Set()))}>
                  {t("Fertig")}
                </Btn>
              </>
            ) : (
              <>
                <button className="sort-btn" onClick={sortMenu.open} data-tip={t("Sortieren")} data-tip-desc={t("Reihenfolge der Dokumente ändern")}>
                  {sortLabels[settings.sort]} <ChevronDown size={14} />
                </button>
                {list.length > 0 && <IconBtn icon={<CheckSquare size={18} />} label={t("Auswählen")} tip={t("Mehrere Dokumente auswählen, um sie zusammenzuführen, zu verschieben oder zu löschen")} onClick={() => setSelecting(true)} />}
                <Segmented
                  small
                  value={settings.view}
                  onChange={(v) => setSettings({ view: v })}
                  options={[
                    { value: "grid", label: <LayoutGrid size={15} />, title: t("Rasteransicht") },
                    { value: "list", label: <List size={15} />, title: t("Listenansicht") },
                  ]}
                />
              </>
            )}
          </div>

          {list.length === 0 ? (
            <Empty
              icon={<Sparkles size={26} />}
              title={q ? t("Nichts gefunden") : t("Noch keine Dokumente")}
              text={q ? t("Kein Dokument passt zu „{0}“.", q) : t("Öffne ein PDF, ein Word-Dokument oder ein EPUB – oder ziehe Dateien einfach hierher.")}
              action={
                !q && (
                  <Btn kind="primary" onClick={openFiles}>
                    <Upload size={16} /> {t("Datei öffnen")}
                  </Btn>
                )
              }
            />
          ) : (
            <div className={settings.view === "grid" ? "doc-grid" : "doc-list"}>
              {list.map((d) => (
                <div
                  key={d.id}
                  className={cx("doc", sel.has(d.id) && "is-selected")}
                  onClick={() => (selecting ? toggleSel(d.id) : onOpen(d.id))}
                  onContextMenu={(e) => {
                    e.preventDefault();
                    setMenuFor({ doc: d, anchor: e.currentTarget as HTMLElement });
                  }}
                >
                  <Cover d={d} />
                  <div className="doc-info">
                    <b title={d.name}>{d.name.replace(/\.(pdf|docx|epub)$/i, "")}</b>
                    <span>
                      {d.format !== "pdf" && <em className="badge">{d.format.toUpperCase()}</em>}
                      {d.encrypted && <Lock size={11} />}
                      {d.pages ? t("{0} S.", d.pages) + " · " : ""}
                      {fmtSize(d.size)}
                      {settings.view === "list" && " · " + new Date(d.openedAt).toLocaleDateString()}
                    </span>
                  </div>
                  {selecting ? (
                    <span className={cx("doc-check", sel.has(d.id) && "on")}>{sel.has(d.id) ? "✓" : ""}</span>
                  ) : (
                    <>
                      {d.starred && <Star size={14} className="doc-star" fill="currentColor" />}
                      <button
                        className="iconbtn iconbtn-sm doc-more"
                        aria-label={t("Mehr")}
                        data-tip-desc={t("Umbenennen, verschieben, speichern, teilen, löschen")}
                        onClick={(e) => {
                          e.stopPropagation();
                          setMenuFor({ doc: d, anchor: e.currentTarget as HTMLElement });
                        }}
                      >
                        <MoreVertical size={16} />
                      </button>
                    </>
                  )}
                </div>
              ))}
            </div>
          )}
        </section>
      </main>

      {selecting && sel.size > 0 && (
        <div className="selbar">
          <span>{t("{0} ausgewählt", sel.size)}</span>
          <Btn kind="ghost" disabled={sel.size < 2 || [...sel].some((id) => docs.find((d) => d.id === id)?.format !== "pdf")} onClick={() => setMergeList(list.filter((d) => sel.has(d.id)))}>
            <Combine size={16} /> {t("Zusammenführen")}
          </Btn>
          <Btn kind="ghost" onClick={() => setMoveDocs([...sel])}>
            <FolderInput size={16} /> {t("Verschieben")}
          </Btn>
          <Btn kind="danger" onClick={() => remove([...sel])}>
            <Trash2 size={16} /> {t("Löschen")}
          </Btn>
        </div>
      )}

      {!selecting && (
        <button className="fab" onClick={addMenu.open} aria-label={t("Hinzufügen")}>
          <Plus size={24} />
        </button>
      )}
      {addMenu.anchor && (
        <Menu
          anchor={addMenu.anchor}
          onClose={addMenu.close}
          items={[
            { label: t("Datei öffnen"), icon: <Upload size={16} />, onClick: openFiles },
            { label: isAndroid ? t("Fotos / Kamera → PDF") : t("Bilder → PDF"), icon: isAndroid ? <Camera size={16} /> : <ImagePlus size={16} />, onClick: fromImages },
            { label: t("PDFs zusammenführen"), icon: <Combine size={16} />, onClick: mergeFromDisk },
            { label: t("Leeres PDF"), icon: <FilePlus2 size={16} />, onClick: blank },
            { label: t("Neuer Ordner"), icon: <FolderPlus size={16} />, onClick: () => setNewFolder(true) },
          ]}
        />
      )}
      {sortMenu.anchor && (
        <Menu anchor={sortMenu.anchor} onClose={sortMenu.close} items={(Object.keys(sortLabels) as Settings["sort"][]).map((k) => ({ label: sortLabels[k], icon: settings.sort === k ? "✓" : "", onClick: () => setSettings({ sort: k }) }))} />
      )}
      {menuFor && <Menu anchor={menuFor.anchor} items={docMenu(menuFor.doc)} onClose={() => setMenuFor(null)} />}
      {folderMenu.anchor && folderCtx && (
        <Menu
          anchor={folderMenu.anchor}
          onClose={folderMenu.close}
          items={[
            {
              label: t("Umbenennen"),
              icon: <Pencil size={16} />,
              onClick: async () => {
                const n = prompt(t("Ordnername"), folderCtx.name);
                if (n?.trim()) {
                  await db.putFolder({ ...folderCtx, name: n.trim() });
                  await reload();
                }
              },
            },
            {
              label: t("Ordner löschen"),
              icon: <Trash2 size={16} />,
              danger: true,
              onClick: async () => {
                if (!(await confirm(t("Ordner löschen?"), t("Die Dokumente darin bleiben erhalten."), t("Löschen"), true))) return;
                for (const d of docs.filter((x) => x.folderId === folderCtx.id)) await db.putDoc({ ...d, folderId: null });
                await db.deleteFolder(folderCtx.id);
                setFolder("all");
                await reload();
              },
            },
          ]}
        />
      )}

      {rename && <RenameDialog doc={rename} onClose={() => setRename(null)} onSave={async (name) => (await update(rename, { name }), setRename(null))} />}
      {newFolder && (
        <NewFolderDialog
          onClose={() => setNewFolder(false)}
          onSave={async (name, color) => {
            const f = { id: uid(), name, color, createdAt: Date.now() };
            await db.putFolder(f);
            setNewFolder(false);
            await reload();
            setFolder(f.id);
          }}
        />
      )}
      {moveDocs && (
        <Modal open title={t("In Ordner verschieben")} icon={<FolderInput size={18} />} onClose={() => setMoveDocs(null)}>
          <div className="folder-pick">
            <button
              onClick={async () => {
                for (const id of moveDocs) {
                  const d = docs.find((x) => x.id === id);
                  if (d) await db.putDoc({ ...d, folderId: null });
                }
                setMoveDocs(null);
                setSelecting(false);
                setSel(new Set());
                await reload();
              }}
            >
              <FileText size={17} /> {t("Kein Ordner")}
            </button>
            {folders.map((f) => (
              <button
                key={f.id}
                onClick={async () => {
                  for (const id of moveDocs) {
                    const d = docs.find((x) => x.id === id);
                    if (d) await db.putDoc({ ...d, folderId: f.id });
                  }
                  setMoveDocs(null);
                  setSelecting(false);
                  setSel(new Set());
                  await reload();
                  toast(t("Verschoben nach „{0}“", f.name));
                }}
              >
                <FolderIcon size={17} style={{ color: f.color }} /> {f.name}
              </button>
            ))}
            <button onClick={() => (setMoveDocs(null), setNewFolder(true))}>
              <FolderPlus size={17} /> {t("Neuer Ordner …")}
            </button>
          </div>
        </Modal>
      )}
      {mergeList && <MergeDialog items={mergeList} onClose={() => setMergeList(null)} onMerge={merge} />}
      {showSettings && <SettingsDialog settings={settings} setSettings={setSettings} onClose={() => setShowSettings(false)} docs={docs} />}
      {dragOver && (
        <div className="dropzone">
          <Upload size={34} />
          <b>{t("Dateien hier ablegen")}</b>
          <span>{t("PDF, Word, EPUB oder Bilder")}</span>
        </div>
      )}
      {confirmNode}
    </div>
  );
}

function Cover({ d }: { d: DocMeta }) {
  return (
    <div className={cx("cover", "cover-" + d.format)}>
      {d.thumb ? (
        <img src={d.thumb} alt="" loading="lazy" draggable={false} />
      ) : (
        <div className="cover-blank">
          {d.encrypted ? <Lock size={22} /> : <FileText size={22} />}
          <span>{d.format.toUpperCase()}</span>
        </div>
      )}
    </div>
  );
}

function RenameDialog({ doc, onClose, onSave }: { doc: DocMeta; onClose: () => void; onSave: (name: string) => void }) {
  const t = useT();
  const ext = doc.name.match(/\.(pdf|docx|epub)$/i)?.[0] ?? "";
  const [v, setV] = useState(doc.name.slice(0, doc.name.length - ext.length));
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => ref.current?.select(), []);
  return (
    <Modal open title={t("Umbenennen")} icon={<Pencil size={18} />} onClose={onClose} footer={<><Btn kind="ghost" onClick={onClose}>{t("Abbrechen")}</Btn><Btn kind="primary" disabled={!v.trim()} onClick={() => onSave(v.trim() + ext)}>{t("Speichern")}</Btn></>}>
      <input ref={ref} className="input" value={v} onChange={(e) => setV(e.target.value)} onKeyDown={(e) => e.key === "Enter" && v.trim() && onSave(v.trim() + ext)} autoFocus />
    </Modal>
  );
}

function NewFolderDialog({ onClose, onSave }: { onClose: () => void; onSave: (name: string, color: string) => void }) {
  const t = useT();
  const [v, setV] = useState("");
  const [c, setC] = useState(FOLDER_COLORS[0]);
  return (
    <Modal open title={t("Neuer Ordner")} icon={<FolderPlus size={18} />} onClose={onClose} footer={<><Btn kind="ghost" onClick={onClose}>{t("Abbrechen")}</Btn><Btn kind="primary" disabled={!v.trim()} onClick={() => onSave(v.trim(), c)}>{t("Anlegen")}</Btn></>}>
      <input className="input" value={v} placeholder={t("Name")} onChange={(e) => setV(e.target.value)} onKeyDown={(e) => e.key === "Enter" && v.trim() && onSave(v.trim(), c)} autoFocus />
      <div className="swatches" style={{ marginTop: 14 }}>
        {FOLDER_COLORS.map((x) => (
          <button key={x} className={cx("swatch", x === c && "is-active")} style={{ background: x }} onClick={() => setC(x)} aria-label={x} />
        ))}
      </div>
    </Modal>
  );
}

function MergeDialog({ items, onClose, onMerge }: { items: DocMeta[]; onClose: () => void; onMerge: (items: DocMeta[], name: string) => void }) {
  const t = useT();
  const [list, setList] = useState(items);
  const [name, setName] = useState(t("Zusammengeführt") + " " + new Date().toLocaleDateString());
  const mv = (i: number, d: number) => {
    const l = [...list];
    [l[i], l[i + d]] = [l[i + d], l[i]];
    setList(l);
  };
  return (
    <Modal open wide title={t("PDFs zusammenführen")} icon={<Combine size={18} />} onClose={onClose} footer={<><Btn kind="ghost" onClick={onClose}>{t("Abbrechen")}</Btn><Btn kind="primary" disabled={list.length < 2} onClick={() => onMerge(list, name)}>{t("Zusammenführen")}</Btn></>}>
      <p className="muted">{t("Reihenfolge festlegen – Kommentare werden mit übernommen.")}</p>
      <div className="merge-list">
        {list.map((d, i) => (
          <div key={d.id} className="merge-item">
            <span className="merge-no">{i + 1}</span>
            <Cover d={d} />
            <div className="doc-info">
              <b>{d.name}</b>
              <span>{t("{0} Seiten", d.pages)}</span>
            </div>
            <IconBtn icon={<ArrowUp size={16} />} label={t("Nach oben")} disabled={i === 0} onClick={() => mv(i, -1)} />
            <IconBtn icon={<ArrowDown size={16} />} label={t("Nach unten")} disabled={i === list.length - 1} onClick={() => mv(i, 1)} />
            <IconBtn icon={<X size={16} />} label={t("Entfernen")} onClick={() => setList(list.filter((x) => x.id !== d.id))} />
          </div>
        ))}
      </div>
      <Field label={t("Name der neuen Datei")}>
        <input className="input" value={name} onChange={(e) => setName(e.target.value)} />
      </Field>
    </Modal>
  );
}

function SettingsDialog({ settings, setSettings, onClose, docs }: { settings: Settings; setSettings: (s: Partial<Settings>) => void; onClose: () => void; docs: DocMeta[] }) {
  const t = useT();
  const [usage, setUsage] = useState<string>("");
  useEffect(() => {
    navigator.storage?.estimate?.().then((e) => setUsage(fmtSize(e.usage ?? 0)));
  }, []);
  return (
    <Modal open title={t("Einstellungen")} icon={<SettingsIcon size={18} />} onClose={onClose} wide>
      <div className="settings">
        <Field label={t("Sprache")}>
          <Segmented value={settings.lang} onChange={(v) => setSettings({ lang: v })} options={[{ value: "de", label: <><Globe size={14} /> Deutsch</> }, { value: "en", label: <><Globe size={14} /> English</> }]} />
        </Field>
        <Field label={t("Design")}>
          <Segmented
            value={settings.theme}
            onChange={(v) => setSettings({ theme: v })}
            options={[
              { value: "light", label: <><Palette size={14} /> {t("Hell")}</> },
              { value: "dark", label: <><Palette size={14} /> {t("Dunkel")}</> },
              { value: "system", label: t("System") },
            ]}
          />
        </Field>
        <Field label={t("Dein Name für Kommentare")} hint={t("Wird bei Kommentaren als Autor gespeichert (sichtbar in Acrobat & Co.).")}>
          <div className="input-wrap">
            <User size={15} className="input-icon" />
            <input className="input" value={settings.author} placeholder={t("z. B. Max Mustermann")} onChange={(e) => setSettings({ author: e.target.value })} />
          </div>
        </Field>
        <div className="settings-card">
          <HardDrive size={18} />
          <div>
            <b>{t("Speicher")}</b>
            <span>{t("{0} Dokumente · {1} belegt · alles bleibt lokal auf diesem Gerät", docs.length, usage || "…")}</span>
          </div>
        </div>
        <div className="settings-card">
          <PenLine size={18} />
          <div>
            <b>{t("Als Standard-PDF-App")}</b>
            <span>
              {isAndroid
                ? t("Öffne ein PDF in einer anderen App, wähle „Öffnen mit“ → LightPDF Studio → „Immer“.")
                : isDesktop
                  ? t("Rechtsklick auf eine PDF-Datei → „Öffnen mit“ → LightPDF Studio → „Immer verwenden“.")
                  : t("Installiere die Desktop- oder Android-App, um PDFs direkt damit zu öffnen.")}
            </span>
          </div>
        </div>
        <div className="settings-about">
          <img src="./lightpdf_iconsvg.svg" alt="" />
          <div>
            <b>LightPDF Studio 2.1.1</b>
            <span>{t("Freier, quelloffener PDF-Reader und -Editor. Basiert auf LightPDF. Lizenz: MIT.")}</span>
            <span className="muted small">{t("Funktionen: Lesen, Suchen, Kommentieren, Ausfüllen & Signieren, Seiten organisieren, Zusammenführen, Teilen, Komprimieren, Schützen, Schwärzen, Wasserzeichen, Seitenzahlen, Export.")}</span>
          </div>
        </div>
      </div>
    </Modal>
  );
}
