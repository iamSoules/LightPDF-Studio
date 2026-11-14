// Lokale Datenbank (IndexedDB) – alles bleibt auf dem Gerät.
import type { Annotation, DocMeta, Folder, SavedSignature, Settings } from "../types";

const DB_NAME = "lightpdf-studio";
const VERSION = 1;
const STORES = ["docs", "files", "annots", "folders", "signatures", "kv"] as const;
type Store = (typeof STORES)[number];

let dbPromise: Promise<IDBDatabase> | null = null;

function open(): Promise<IDBDatabase> {
  if (!dbPromise) {
    dbPromise = new Promise((resolve, reject) => {
      const req = indexedDB.open(DB_NAME, VERSION);
      req.onupgradeneeded = () => {
        const db = req.result;
        for (const s of STORES) if (!db.objectStoreNames.contains(s)) db.createObjectStore(s);
      };
      req.onsuccess = () => resolve(req.result);
      req.onerror = () => reject(req.error);
    });
  }
  return dbPromise;
}

function tx<T>(store: Store, mode: IDBTransactionMode, fn: (s: IDBObjectStore) => IDBRequest<T> | void): Promise<T> {
  return open().then(
    (db) =>
      new Promise<T>((resolve, reject) => {
        const t = db.transaction(store, mode);
        const s = t.objectStore(store);
        const r = fn(s);
        let result: T;
        if (r) r.onsuccess = () => (result = r.result);
        t.oncomplete = () => resolve(result);
        t.onerror = () => reject(t.error);
        t.onabort = () => reject(t.error);
      }),
  );
}

const get = <T>(store: Store, key: string) => tx<T>(store, "readonly", (s) => s.get(key));
const put = (store: Store, key: string, value: unknown) => tx(store, "readwrite", (s) => s.put(value, key));
const del = (store: Store, key: string) => tx(store, "readwrite", (s) => s.delete(key));
const all = <T>(store: Store) => tx<T[]>(store, "readonly", (s) => s.getAll());

export const db = {
  // Dokumente
  listDocs: () => all<DocMeta>("docs"),
  getDoc: (id: string) => get<DocMeta | undefined>("docs", id),
  putDoc: (d: DocMeta) => put("docs", d.id, d),
  async deleteDoc(id: string) {
    await del("docs", id);
    await del("files", id);
    await del("annots", id);
  },
  // Dateiinhalte
  getFile: (id: string) => get<ArrayBuffer | undefined>("files", id),
  putFile: (id: string, data: ArrayBuffer | Uint8Array) =>
    put("files", id, data instanceof Uint8Array ? data.slice().buffer : data),
  // Kommentare
  getAnnots: async (id: string) => (await get<Annotation[] | undefined>("annots", id)) ?? [],
  putAnnots: (id: string, a: Annotation[]) => put("annots", id, a),
  // Ordner
  listFolders: () => all<Folder>("folders"),
  putFolder: (f: Folder) => put("folders", f.id, f),
  deleteFolder: (id: string) => del("folders", id),
  // Unterschriften
  listSignatures: () => all<SavedSignature>("signatures"),
  putSignature: (s: SavedSignature) => put("signatures", s.id, s),
  deleteSignature: (id: string) => del("signatures", id),
  // Einstellungen
  getSettings: () => get<Partial<Settings> | undefined>("kv", "settings"),
  putSettings: (s: Settings) => put("kv", "settings", s),
};

export const uid = () => Math.random().toString(36).slice(2, 10) + Date.now().toString(36).slice(-4);
