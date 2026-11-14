// Testdatei für Node bündeln (Vite-Spezialimporte werden ersetzt) und ausführen
import { build } from "esbuild";
import { pathToFileURL } from "node:url";
const file = process.argv[2];
await build({
  entryPoints: [file], bundle: true, platform: "node", format: "esm", outfile: "/tmp/claude-0/lptest/out.mjs",
  plugins: [{ name: "url", setup(b) {
    b.onResolve({ filter: /\?url$/ }, (a) => ({ path: a.path, namespace: "url" }));
    b.onLoad({ filter: /.*/, namespace: "url" }, () => ({ contents: "export default ''", loader: "js" }));
  } }],
  logLevel: "error",
});
await import(pathToFileURL("/tmp/claude-0/lptest/out.mjs").href);
