import { PDFDocument } from "@cantoo/pdf-lib";
import * as ops from "../src/lib/pdfops";
(async () => {
  const d = await PDFDocument.create(); d.addPage();
  const prot = await ops.protect(await d.save(), { userPassword: "geheim", allowPrint: true, allowCopy: false, allowModify: false });
  const un = await ops.removePassword(prot, "geheim");
  const t = Buffer.from(un).toString("latin1");
  console.log("trailer part:", t.slice(-400).replace(/[^\x20-\x7e\n]/g, "."));
  try { const x = await PDFDocument.load(un); console.log("plain load ok", x.getPageCount()); } catch (e) { console.log("plain load fail", (e as Error).message.slice(0, 60)); }
})();
