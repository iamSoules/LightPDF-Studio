import { useCallback, useEffect, useState } from "react";
import Library from "./components/Library";
import Reader from "./components/Reader";
import HtmlReader from "./components/HtmlReader";
import { ToastProvider, TooltipLayer, useToast } from "./components/ui";
import { db } from "./lib/db";
import { importFiles } from "./lib/library";
import { nativeTheme, onIncomingFiles } from "./lib/platform";
import type { DocMeta, Folder, Settings } from "./types";
import { LangProvider, setLang, tr } from "./i18n";

const DEFAULTS: Settings = {
  lang: navigator.language?.startsWith("de") ? "de" : "en",
  theme: "system",
  author: "",
  reading: "normal",
  scrollMode: "vertical",
  spread: "none",
  sort: "recent",
  view: "grid",
};

function Shell() {
  const { toast, busy } = useToast();
  const [settings, setSettingsState] = useState<Settings | null>(null);
  const [docs, setDocs] = useState<DocMeta[]>([]);
  const [folders, setFolders] = useState<Folder[]>([]);
  const [open, setOpen] = useState<DocMeta | null>(null);

  const reload = useCallback(async () => {
    const [d, f] = await Promise.all([db.listDocs(), db.listFolders()]);
    setDocs(d);
    setFolders(f.sort((a, b) => a.createdAt - b.createdAt));
  }, []);

  useEffect(() => {
    (async () => {
      const s = { ...DEFAULTS, ...((await db.getSettings()) ?? {}) };
      setLang(s.lang);
      setSettingsState(s);
      await reload();
      // Von außen geöffnete Dateien (Doppelklick, „Öffnen mit“, Teilen)
      onIncomingFiles(async (files) => {
        try {
          busy(tr("Öffne …"));
          const metas = await importFiles(files);
          busy(null);
          await reload();
          if (metas[0]) setOpen(metas[0]);
        } catch (e) {
          busy(null);
          toast(tr("Konnte die Datei nicht öffnen: {0}", (e as Error).message), "err");
        }
      });
    })();
  }, []);

  const setSettings = useCallback((patch: Partial<Settings>) => {
    setSettingsState((s) => {
      const n = { ...s!, ...patch };
      db.putSettings(n);
      if (patch.lang) setLang(patch.lang);
      return n;
    });
  }, []);

  // Design
  useEffect(() => {
    if (!settings) return;
    const apply = () => {
      const dark = settings.theme === "dark" || (settings.theme === "system" && matchMedia("(prefers-color-scheme: dark)").matches);
      document.documentElement.dataset.theme = dark ? "dark" : "light";
      document.querySelector('meta[name="theme-color"]')?.setAttribute("content", dark ? "#171a16" : "#fbf9f4");
      nativeTheme(dark);
    };
    apply();
    const mq = matchMedia("(prefers-color-scheme: dark)");
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, [settings?.theme]);

  const openId = async (id: string) => {
    const m = (await db.getDoc(id)) ?? null;
    if (m) setOpen({ ...m });
  };

  if (!settings) return <div className="splash" />;
  return (
    <LangProvider lang={settings.lang}>
      <div className="app">
        {open ? (
          open.format === "pdf" ? (
            <Reader
              key={open.id + open.openedAt}
              meta={open}
              settings={settings}
              setSettings={setSettings}
              onOpenDoc={openId}
              onClose={() => {
                setOpen(null);
                reload();
              }}
            />
          ) : (
            <HtmlReader
              key={open.id}
              meta={open}
              settings={settings}
              setSettings={setSettings}
              onClose={() => {
                setOpen(null);
                reload();
              }}
            />
          )
        ) : (
          <Library docs={docs} folders={folders} settings={settings} setSettings={setSettings} reload={reload} onOpen={openId} />
        )}
      </div>
    </LangProvider>
  );
}

export default function App() {
  return (
    <ToastProvider>
      <Shell />
      <TooltipLayer />
    </ToastProvider>
  );
}
