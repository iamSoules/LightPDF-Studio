// Brücke zwischen Oberfläche und System – nur diese Funktionen sind erreichbar.
const { contextBridge, ipcRenderer, webUtils } = require("electron");

contextBridge.exposeInMainWorld("lpDesktop", {
  platform: process.platform,
  openFiles: (filters, multi) => ipcRenderer.invoke("open-files", { filters, multi }),
  saveFile: (defaultName, data, filters) => ipcRenderer.invoke("save-file", { defaultName, data, filters }),
  writeFile: (path, data) => ipcRenderer.invoke("write-file", { path, data }),
  pathForFile: (file) => {
    try {
      return webUtils.getPathForFile(file) || undefined;
    } catch {
      return undefined;
    }
  },
  onOpenFiles: (cb) => ipcRenderer.on("open-files", (_e, files) => cb(files)),
  ready: () => ipcRenderer.invoke("ready"),
  showInFolder: (path) => ipcRenderer.send("show-in-folder", path),
});
