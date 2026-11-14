import { PDFDocument, StandardFonts } from "@cantoo/pdf-lib";
import * as ops from "../src/lib/pdfops";
import { writeFileSync } from "fs";
(async () => {
  const d = await PDFDocument.create();
  const f = await d.embedFont(StandardFonts.Helvetica);
  for (let i = 0; i < 3; i++) d.addPage([595, 842]).drawText("PDF/A Seite " + (i + 1), { x: 50, y: 780, size: 20, font: f });
  await (d as any).convertToPDFA({ conformance: "1B" });
  const a = await d.save({ useObjectStreams: false });
  writeFileSync("/tmp/claude-0/lptest/pdfa1b.pdf", a);
  const ann: any[] = [{ id: "h", page: 1, type: "highlight", rects: [[50, 770, 250, 800]], color: "#f6c945", opacity: 1, createdAt: Date.now() }];
  const steps: [string, () => Promise<Uint8Array | unknown>][] = [
    ["exportPdf", () => ops.exportPdf(a, ann)],
    ["exportPdf flatten", () => ops.exportPdf(a, ann, { flatten: true })],
    ["importPdf", async () => (await ops.importPdf(await ops.exportPdf(a, ann))).annots.length],
    ["rotate", () => ops.rotatePages(a, [0], 90)],
    ["delete", () => ops.deletePages(a, [1])],
    ["reorder", () => ops.reorderPages(a, [2, 1, 0])],
    ["blank", () => ops.insertBlank(a, 1)],
    ["insertPdf", () => ops.insertPdf(a, a, 3)],
    ["extract", () => ops.extractPages(a, [0])],
    ["watermark", () => ops.watermark(a, { text: "X", size: 40, color: "#ff0000", opacity: 0.3, angle: 45 })],
    ["numbers", () => ops.stampText(a, { template: "{n}", position: "bc", size: 10, color: "#000000", start: 1, margin: 20, skipFirst: false, name: "" })],
    ["setMeta", () => ops.setMeta(a, { title: "T" })],
    ["protect", () => ops.protect(a, { userPassword: "pw12", allowPrint: true, allowCopy: true, allowModify: true })],
    ["compress lossless", () => ops.compress(a, "lossless")],
    ["merge", () => ops.merge([a, a])],
  ];
  for (const [n, fn] of steps) {
    try {
      const r = await fn();
      if (r instanceof Uint8Array) await PDFDocument.load(r, { ignoreEncryption: true });
      console.log("OK  ", n);
    } catch (e) {
      console.log("FAIL", n, "-", (e as Error).message.slice(0, 70));
    }
  }
})();
