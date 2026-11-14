import * as ops from "../src/lib/pdfops";
import { readFileSync, writeFileSync } from "fs";
(async () => {
  const a = new Uint8Array(readFileSync("/tmp/claude-0/lptest/pdfa1b.pdf"));
  const ann: any[] = [{ id: "h", page: 1, type: "highlight", rects: [[50, 770, 250, 800]], color: "#f6c945", opacity: 1, createdAt: Date.now() }];
  writeFileSync("/tmp/claude-0/lptest/pdfa-annot.pdf", await ops.exportPdf(a, ann));
  writeFileSync("/tmp/claude-0/lptest/pdfa-prot.pdf", await ops.protect(a, { userPassword: "pw12", allowPrint: true, allowCopy: true, allowModify: true }));
  console.log("PDF/A erkannt:", await ops.isPdfA(a), "| nach Schutz:", await ops.isPdfA(readFileSync("/tmp/claude-0/lptest/pdfa-prot.pdf"), "pw12"));
})();
