import { PDFDocument, degrees, StandardFonts } from "@cantoo/pdf-lib";
import * as ops from "../src/lib/pdfops";
import { writeFileSync } from "fs";
const png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAAEUlEQVR42mP8z8DwHwQYGRgYAA0YAgJQjJZsAAAAAElFTkSuQmCC";
(async () => {
  const d = await PDFDocument.create(); const f = await d.embedFont(StandardFonts.Helvetica);
  for (const r of [0, 90, 180, 270]) { const p = d.addPage([400, 560]); p.drawText("Seite mit Rotate " + r, { x: 30, y: 520, size: 18, font: f }); p.setRotation(degrees(r)); }
  let b = await d.save();
  b = await ops.watermark(b, { text: "ENTWURF", size: 50, color: "#1f5fbf", opacity: 0.3, angle: 30 });
  b = await ops.stampText(b, { template: "Seite {n} von {total}", position: "br", size: 11, color: "#c62828", start: 1, margin: 20, skipFirst: false, name: "" });
  const base = { color: "#2e7d32", opacity: 1, createdAt: Date.now() };
  const ann: any[] = [];
  // Rechteck im Seitenraum, das in der Ansicht oben links liegt (Breite > Höhe sichtbar)
  const rects: Record<number, number[]> = { 1: [40, 400, 240, 440], 2: [300, 40, 340, 240], 3: [160, 120, 360, 160], 4: [60, 320, 100, 520] };
  for (let i = 1; i <= 4; i++) {
    ann.push({ ...base, id: "t" + i, page: i, type: "text", rect: rects[i], text: "Aufrechter Text", fontSize: 14 });
  }
  const out = await ops.exportPdf(b, ann);
  writeFileSync("/tmp/claude-0/lptest/rotated.pdf", out);
})();
