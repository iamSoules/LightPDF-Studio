// LightPDF Studio – Desktop (Windows / macOS / Linux)
const { app, BrowserWindow, ipcMain, dialog, shell, protocol, net, Menu, nativeTheme, screen } = require("electron");
const path = require("node:path");
const fs = require("node:fs/promises");
const fss = require("node:fs");
const { pathToFileURL } = require("node:url");

const DIST = path.join(__dirname, "..", "dist");
const SUPPORTED = /\.(pdf|docx|epub|png|jpe?g|webp|gif|bmp)$/i;
let win = null;
let rendererReady = false;
const pending = []; // Dateien, die vor dem Start der Oberfläche geöffnet wurden

// Eigenes Protokoll: Web-App wird wie von einem sicheren Ursprung geladen (IndexedDB, Worker, Fetch)
protocol.registerSchemesAsPrivileged([
  { scheme: "app", privileges: { standard: true, secure: true, supportFetchAPI: true, corsEnabled: true, stream: true } },
]);

// Nur eine Instanz – weitere Starts geben ihre Dateien an das offene Fenster weiter
if (!app.requestSingleInstanceLock()) {
  app.quit();
} else {
  app.on("second-instance", (_e, argv) => {
    queueFiles(filesFromArgv(argv));
    if (win) {
      if (win.isMinimized()) win.restore();
      win.focus();
    }
  });
}

function filesFromArgv(argv) {
  return argv.slice(1).filter((a) => !a.startsWith("-") && SUPPORTED.test(a) && fss.existsSync(a));
}

async function readFiles(paths) {
  const out = [];
  for (const p of paths) {
    try {
      const data = await fs.readFile(p);
      out.push({ name: path.basename(p), path: p, data: new Uint8Array(data.buffer, data.byteOffset, data.byteLength) });
    } catch (e) {
      console.error("read failed", p, e);
    }
  }
  return out;
}

async function queueFiles(paths) {
  if (!paths.length) return;
  if (rendererReady && win) win.webContents.send("open-files", await readFiles(paths));
  else pending.push(...paths);
}

// macOS: Dateien per Finder / Dock öffnen
app.on("open-file", (e, p) => {
  e.preventDefault();
  queueFiles([p]);
});

function createWindow() {
  const { width, height } = screen.getPrimaryDisplay().workAreaSize;
  win = new BrowserWindow({
    width: Math.min(1440, Math.round(width * 0.9)),
    height: Math.min(960, Math.round(height * 0.92)),
    minWidth: 380,
    minHeight: 520,
    title: "LightPDF Studio",
    backgroundColor: nativeTheme.shouldUseDarkColors ? "#171a16" : "#fbf9f4",
    show: false,
    titleBarStyle: process.platform === "darwin" ? "hiddenInset" : "default",
    trafficLightPosition: { x: 14, y: 18 },
    autoHideMenuBar: true,
    icon: path.join(__dirname, "icon.png"),
    webPreferences: {
      preload: path.join(__dirname, "preload.cjs"),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
      spellcheck: false,
    },
  });
  win.once("ready-to-show", () => win.show());
  win.loadURL("app://lightpdf/index.html");

  // Externe Links im Browser öffnen, keine fremden Seiten im App-Fenster
  win.webContents.setWindowOpenHandler(({ url }) => {
    if (/^https?:|^mailto:/.test(url)) shell.openExternal(url);
    return { action: "deny" };
  });
  win.webContents.on("will-navigate", (e, url) => {
    if (!url.startsWith("app://")) {
      e.preventDefault();
      if (/^https?:|^mailto:/.test(url)) shell.openExternal(url);
    }
  });
  win.on("closed", () => {
    win = null;
    rendererReady = false;
  });
}

// ------------------------------------------------------------------ Schnittstelle zur Oberfläche
ipcMain.handle("ready", async () => {
  rendererReady = true;
  if (pending.length) {
    const files = await readFiles(pending.splice(0));
    win?.webContents.send("open-files", files);
  }
});

ipcMain.handle("open-files", async (_e, { filters, multi }) => {
  const r = await dialog.showOpenDialog(win, {
    properties: ["openFile", ...(multi ? ["multiSelections"] : [])],
    filters: [...filters, { name: "Alle Dateien", extensions: ["*"] }],
  });
  return r.canceled ? [] : readFiles(r.filePaths);
});

ipcMain.handle("save-file", async (_e, { defaultName, data, filters }) => {
  const r = await dialog.showSaveDialog(win, {
    defaultPath: path.join(app.getPath("documents"), defaultName),
    filters,
  });
  if (r.canceled || !r.filePath) return null;
  await fs.writeFile(r.filePath, Buffer.from(data));
  return r.filePath;
});

ipcMain.handle("write-file", async (_e, { path: p, data }) => {
  // Sicher schreiben: erst in eine temporäre Datei, dann ersetzen
  const tmp = p + ".lightpdf-tmp";
  await fs.writeFile(tmp, Buffer.from(data));
  await fs.rename(tmp, p);
});

ipcMain.on("show-in-folder", (_e, p) => shell.showItemInFolder(p));

// ------------------------------------------------------------------ Start
app.whenReady().then(() => {
  protocol.handle("app", (req) => {
    const { pathname } = new URL(req.url);
    const file = path.normalize(path.join(DIST, decodeURIComponent(pathname)));
    if (!file.startsWith(DIST)) return new Response("Forbidden", { status: 403 });
    return net.fetch(pathToFileURL(file).toString());
  });

  if (process.platform === "darwin") {
    Menu.setApplicationMenu(
      Menu.buildFromTemplate([
        { role: "appMenu" },
        { role: "editMenu" },
        { label: "Ansicht", submenu: [{ role: "togglefullscreen" }, { role: "reload" }, { role: "toggleDevTools" }] },
        { role: "windowMenu" },
      ]),
    );
  } else Menu.setApplicationMenu(null);

  createWindow();
  queueFiles(filesFromArgv(process.argv));
  app.on("activate", () => BrowserWindow.getAllWindows().length === 0 && createWindow());
});

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") app.quit();
});
