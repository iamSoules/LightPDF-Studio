import { PDFDocument, PDFName, PDFArray, PDFDict } from "@cantoo/pdf-lib";
import { writeAnnotations, extractOwnAnnotations } from "../src/lib/annotio";
import * as ops from "../src/lib/pdfops";
import type { Annotation } from "../src/types";
import { writeFileSync } from "fs";
const png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
(async () => {
  const d = await PDFDocument.create();
  for (let i = 0; i < 4; i++) { const p = d.addPage([595, 842]); p.drawText("Seite " + (i + 1), { x: 50, y: 780, size: 30 }); }
  const base = await d.save();
  const b = { color: "#e0b030", opacity: 1, createdAt: Date.now(), author: "Test" };
  const ann: Annotation[] = [
    { ...b, id: "h1", page: 1, type: "highlight", rects: [[50, 770, 200, 810]], text: "Seite 1", note: "Wichtig" },
    { ...b, id: "u1", page: 1, type: "underline", rects: [[50, 700, 200, 720]] },
    { ...b, id: "s1", page: 1, type: "strike", rects: [[50, 650, 200, 670]] },
    { ...b, id: "i1", page: 2, type: "ink", paths: [[[100, 100], [150, 160], [200, 120]]], width: 3 },
    { ...b, id: "r1", page: 2, type: "rect", rect: [100, 300, 250, 400], width: 2, fill: "#ffeecc" },
    { ...b, id: "e1", page: 2, type: "ellipse", rect: [300, 300, 450, 400], width: 2 },
    { ...b, id: "a1", page: 3, type: "arrow", from: [100, 100], to: [300, 300], width: 2 },
    { ...b, id: "l1", page: 3, type: "line", from: [100, 500], to: [300, 500], width: 1 },
    { ...b, id: "t1", page: 3, type: "text", rect: [100, 600, 400, 700], text: "Hallo Welt – Größe äöü ß € 日本", fontSize: 14 },
    { ...b, id: "n1", page: 4, type: "note", at: [500, 800], text: "Notiz!" },
    { ...b, id: "g1", page: 4, type: "image", rect: [100, 100, 300, 160], src: png, kind: "signature" },
    { ...b, id: "x1", page: 4, type: "redact", rect: [100, 400, 300, 450] },
  ];
  const out = await ops.exportPdf(base, ann);
  writeFileSync("/tmp/claude-0/lptest/annots.pdf", out);
  const back = await ops.importPdf(out);
  console.log("roundtrip:", back.annots.length === ann.length, back.annots.map(a => a.type).join(","));
  console.log("note kept:", (back.annots[0] as any).note, "| text:", (back.annots.find(a=>a.id==="t1") as any).text);
  const chk = await PDFDocument.load(back.base);
  console.log("annots left in base:", chk.getPages().map(p => p.node.lookupMaybe(PDFName.of("Annots"), PDFArray)?.size() ?? 0).join(","));
  const subtypes = (await PDFDocument.load(out)).getPages().flatMap(p => { const a = p.node.lookupMaybe(PDFName.of("Annots"), PDFArray); return a ? Array.from({length:a.size()},(_,i)=>a.lookup(i, PDFDict).get(PDFName.of("Subtype"))!.toString()) : []; });
  console.log("subtypes:", subtypes.join(" "));
  const flat = await ops.exportPdf(base, ann, { flatten: true });
  console.log("flatten annots:", (await PDFDocument.load(flat)).getPages().map(p => p.node.lookupMaybe(PDFName.of("Annots"), PDFArray)?.size() ?? 0).join(","), flat.length);
  writeFileSync("/tmp/claude-0/lptest/flat.pdf", flat);
  // Seiten
  let r = await ops.reorderPages(base, [3, 2, 1, 0, 0]); console.log("reorder pages:", (await PDFDocument.load(r)).getPageCount());
  r = await ops.deletePages(base, [1, 2]); console.log("delete ->", (await PDFDocument.load(r)).getPageCount());
  r = await ops.rotatePages(base, [0], 90); console.log("rotate ->", (await PDFDocument.load(r)).getPage(0).getRotation().angle);
  r = await ops.insertBlank(base, 2); console.log("blank ->", (await PDFDocument.load(r)).getPageCount());
  r = await ops.insertPdf(base, base, 4); console.log("insertPdf ->", (await PDFDocument.load(r)).getPageCount());
  r = await ops.merge([base, base, base]); console.log("merge ->", (await PDFDocument.load(r)).getPageCount());
  console.log("ranges:", JSON.stringify(ops.parseRanges("1-2, 4, 3-", 4)));
  r = await ops.watermark(base, { text: "VERTRAULICH", size: 60, color: "#cc0000", opacity: 0.3, angle: 45 }); writeFileSync("/tmp/claude-0/lptest/wm.pdf", r);
  r = await ops.stampText(r, { template: "Seite {n} von {total}", position: "bc", size: 10, color: "#333333", start: 1, margin: 24, skipFirst: false, name: "" }); writeFileSync("/tmp/claude-0/lptest/num.pdf", r);
  const prot = await ops.protect(base, { userPassword: "geheim", allowPrint: true, allowCopy: false, allowModify: false });
  writeFileSync("/tmp/claude-0/lptest/prot.pdf", prot);
  const pd = await PDFDocument.load(prot, { ignoreEncryption: true }); console.log("encrypted:", pd.isEncrypted);
  const un = await ops.removePassword(prot, "geheim"); const ud = await PDFDocument.load(un); console.log("decrypted pages:", ud.getPageCount(), "enc:", ud.isEncrypted);
  const ex = await ops.exportPdf(prot, ann.slice(0,2), { password: "geheim" }); console.log("export protected enc:", (await PDFDocument.load(ex, {ignoreEncryption:true})).isEncrypted);
  writeFileSync("/tmp/claude-0/lptest/prot-annots.pdf", ex);
  const m = await ops.setMeta(base, { title: "Titel", author: "Ich", keywords: "a, b" }); console.log("meta:", JSON.stringify(await ops.getMeta(m)));
})().catch(e => { console.error("FAIL", e); process.exit(1); });
