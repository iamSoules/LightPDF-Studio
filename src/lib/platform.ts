// Plattform-Schicht: Browser, Desktop (Electron) und Android (Capacitor)
import { Capacitor } from "@capacitor/core";

export interface IncomingFile {
  name: string;
  data: Uint8Array;
  path?: string;
}

interface NativeDesktop {
  platform: string;
  openFiles(filters: { name: string; extensions: string[] }[], multi: boolean): Promise<IncomingFile[]>;
  saveFile(defaultName: string, data: Uint8Array, filters: { name: string; extensions: string[] }[]): Promise<string | null>;
  writeFile(path: string, data: Uint8Array): Promise<void>;
  pathForFile(f: File): string | undefined;
  onOpenFiles(cb: (files: IncomingFile[]) => void): void;
  ready(): void;
  showInFolder(path: string): void;
}

const desktop: NativeDesktop | undefined = (window as any).lpDesktop;
export const isDesktop = !!desktop;
export const isAndroid = Capacitor.getPlatform() === "android";
export const isMac = desktop ? desktop.platform === "darwin" : /Mac/.test(navigator.platform);
export const canPrint = !isAndroid;
export const canShare = isAndroid || !!(navigator as any).canShare;

const MIME: Record<string, string> = {
  pdf: "application/pdf",
  zip: "application/zip",
  txt: "text/plain",
  png: "image/png",
  jpg: "image/jpeg",
};
const ext = (name: string) => name.split(".").pop()!.toLowerCase();

export function bytesToBase64(bytes: Uint8Array) {
  let s = "";
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) s += String.fromCharCode(...bytes.subarray(i, i + chunk));
  return btoa(s);
}
export function base64ToBytes(b64: string) {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

/** Dateien auswählen */
export async function pickFiles(accept: string, multiple = true): Promise<IncomingFile[]> {
  if (desktop) {
    const exts = accept
      .split(",")
      .map((a) => a.trim().replace(/^\./, ""))
      .filter((a) => a && !a.includes("/"));
    const images = accept.includes("image/");
    const list = images ? [...exts, "png", "jpg", "jpeg", "webp", "gif", "bmp"] : exts;
    return desktop.openFiles([{ name: "Dokumente", extensions: list.length ? list : ["*"] }], multiple);
  }
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = accept;
    input.multiple = multiple;
    input.style.display = "none";
    document.body.appendChild(input);
    input.onchange = async () => {
      const files = Array.from(input.files ?? []);
      input.remove();
      resolve(await Promise.all(files.map(async (f) => ({ name: f.name, data: new Uint8Array(await f.arrayBuffer()) }))));
    };
    addEventListener("focus", () => setTimeout(() => input.isConnected && (input.remove(), resolve([])), 800), { once: true });
    input.click();
  });
}

export async function fileFromDrop(f: File): Promise<IncomingFile> {
  return { name: f.name, data: new Uint8Array(await f.arrayBuffer()), path: desktop?.pathForFile(f) };
}

/** Speichern unter … – gibt den Pfad (Desktop) bzw. eine Beschreibung zurück, null = abgebrochen */
export async function saveAs(name: string, data: Uint8Array): Promise<{ path?: string; label: string } | null> {
  const e = ext(name);
  if (desktop) {
    const path = await desktop.saveFile(name, data, [{ name: e.toUpperCase(), extensions: [e] }]);
    return path ? { path, label: path } : null;
  }
  if (isAndroid) {
    const { Filesystem, Directory } = await import("@capacitor/filesystem");
    const path = `LightPDF/${name}`;
    try {
      await Filesystem.writeFile({ path, data: bytesToBase64(data), directory: Directory.Documents, recursive: true });
      return { label: "Dokumente/" + path };
    } catch {
      await share(name, data);
      return { label: name };
    }
  }
  const blob = new Blob([data], { type: MIME[e] ?? "application/octet-stream" });
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 4000);
  return { label: name };
}

/** Vorhandene Datei überschreiben (nur Desktop) */
export async function writeFile(path: string, data: Uint8Array) {
  if (!desktop) throw new Error("not supported");
  await desktop.writeFile(path, data);
}

export function showInFolder(path: string) {
  desktop?.showInFolder(path);
}

/** Teilen (Android-Teilen-Menü bzw. Web Share) */
export async function share(name: string, data: Uint8Array) {
  if (isAndroid) {
    const { Filesystem, Directory } = await import("@capacitor/filesystem");
    const { Share } = await import("@capacitor/share");
    const res = await Filesystem.writeFile({ path: name, data: bytesToBase64(data), directory: Directory.Cache });
    await Share.share({ title: name, files: [res.uri] } as any);
    return;
  }
  const file = new File([data], name, { type: MIME[ext(name)] ?? "application/octet-stream" });
  if ((navigator as any).canShare?.({ files: [file] })) await navigator.share({ files: [file], title: name });
  else await saveAs(name, data);
}

/** Dateien, die von außen geöffnet werden („Öffnen mit …“, Doppelklick, Drag & Drop aufs Symbol) */
export function onIncomingFiles(cb: (files: IncomingFile[]) => void) {
  if (desktop) {
    desktop.onOpenFiles(cb);
    desktop.ready();
  }
  if (isAndroid) {
    const handle = (d: any) => {
      if (d?.base64) cb([{ name: d.name || "Dokument.pdf", data: base64ToBytes(d.base64) }]);
    };
    addEventListener("lightpdf_native_open_file", (e: any) => handle(e.detail));
    const poll = () => {
      try {
        const raw = (window as any).LightPdfNative?.getPendingFile?.();
        if (raw) handle(JSON.parse(raw));
      } catch {
        /* ignorieren */
      }
    };
    (window as any).__lpReady = true;
    poll();
    setTimeout(poll, 800);
  }
}

/** Android: Höhe der System-Leisten übernehmen und Hell/Dunkel der Leistensymbole setzen */
export function nativeTheme(dark: boolean) {
  const n = (window as any).LightPdfNative;
  if (!n) return;
  try {
    n.setDarkTheme?.(dark);
    const ins = JSON.parse(n.getInsets?.() || "{}");
    if (ins.top) document.documentElement.style.setProperty("--sat", ins.top + "px");
    if (ins.bottom) document.documentElement.style.setProperty("--sab", ins.bottom + "px");
  } catch {
    /* ältere Version */
  }
}

/** Drucken: Seiten als Bilder in einen unsichtbaren Rahmen legen und den Druckdialog öffnen */
export async function printImages(urls: string[]) {
  const frame = document.createElement("iframe");
  frame.style.cssText = "position:fixed;right:0;bottom:0;width:0;height:0;border:0";
  document.body.appendChild(frame);
  const doc = frame.contentDocument!;
  doc.open();
  doc.write(
    `<!doctype html><html><head><style>@page{margin:0}body{margin:0}img{display:block;width:100%;height:auto;page-break-after:always;break-after:page}</style></head><body>${urls
      .map((u) => `<img src="${u}">`)
      .join("")}</body></html>`,
  );
  doc.close();
  await Promise.all(Array.from(doc.images).map((i) => (i.complete ? null : new Promise((r) => (i.onload = r)))));
  frame.contentWindow!.focus();
  frame.contentWindow!.print();
  setTimeout(() => frame.remove(), 60_000);
}
