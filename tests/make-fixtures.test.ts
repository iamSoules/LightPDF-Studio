import { PDFDocument, StandardFonts, rgb } from "@cantoo/pdf-lib";
import { writeFileSync } from "fs";
(async () => {
  const d = await PDFDocument.create();
  const font = await d.embedFont(StandardFonts.Helvetica);
  const form = d.getForm();
  const p = d.addPage([595, 842]);
  p.drawText("Anmeldeformular", { x: 50, y: 780, size: 24, font });
  p.drawText("Name:", { x: 50, y: 720, size: 12, font });
  const tf = form.createTextField("name"); tf.addToPage(p, { x: 120, y: 712, width: 250, height: 22 });
  p.drawText("E-Mail:", { x: 50, y: 680, size: 12, font });
  form.createTextField("email").addToPage(p, { x: 120, y: 672, width: 250, height: 22 });
  p.drawText("Newsletter:", { x: 50, y: 640, size: 12, font });
  form.createCheckBox("news").addToPage(p, { x: 130, y: 636, width: 16, height: 16 });
  p.drawText("Unterschrift: ______________________", { x: 50, y: 560, size: 12, font });
  writeFileSync("/tmp/claude-0/-home-user/839bfce1-f0b2-534e-89ff-721cb617b28b/scratchpad/e2e" + "/formular.pdf", await d.save());
})();
