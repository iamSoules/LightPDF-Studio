// Nach dem Packen: Windows-EXE mit Programmsymbol und Versionsinfo versehen (ohne Wine, rein in JavaScript)
const fs = require("node:fs");
const path = require("node:path");

function buildIco(pngs) {
  const header = Buffer.alloc(6 + 16 * pngs.length);
  header.writeUInt16LE(0, 0);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(pngs.length, 4);
  let offset = header.length;
  pngs.forEach(({ size, data }, i) => {
    const e = 6 + 16 * i;
    header.writeUInt8(size >= 256 ? 0 : size, e);
    header.writeUInt8(size >= 256 ? 0 : size, e + 1);
    header.writeUInt16LE(1, e + 4);
    header.writeUInt16LE(32, e + 6);
    header.writeUInt32LE(data.length, e + 8);
    header.writeUInt32LE(offset, e + 12);
    offset += data.length;
  });
  return Buffer.concat([header, ...pngs.map((p) => p.data)]);
}

exports.default = async function afterPack(ctx) {
  if (ctx.electronPlatformName !== "win32") return;
  const ResEdit = await import("resedit");
  const exe = path.join(ctx.appOutDir, `${ctx.packager.appInfo.productFilename}.exe`);
  const pngs = [16, 32, 64, 128, 256].map((size) => ({ size, data: fs.readFileSync(path.join(__dirname, `ico-${size}.png`)) }));
  const ico = buildIco(pngs);
  fs.writeFileSync(path.join(__dirname, "icon.ico"), ico);

  const data = fs.readFileSync(exe);
  const exeFile = ResEdit.NtExecutable.from(data, { ignoreCert: true });
  const res = ResEdit.NtExecutableResource.from(exeFile);
  const iconFile = ResEdit.Data.IconFile.from(ico);
  ResEdit.Resource.IconGroupEntry.replaceIconsForResource(
    res.entries,
    1,
    1033,
    iconFile.icons.map((i) => i.data),
  );
  const [vi] = ResEdit.Resource.VersionInfo.fromEntries(res.entries);
  if (vi) {
    const version = ctx.packager.appInfo.version.split(".").map(Number);
    vi.setFileVersion(...version, 0, 1033);
    vi.setProductVersion(...version, 0, 1033);
    vi.setStringValues(
      { lang: 1033, codepage: 1200 },
      {
        FileDescription: "LightPDF Studio",
        ProductName: "LightPDF Studio",
        CompanyName: "LightPDF Studio",
        LegalCopyright: "MIT License",
        OriginalFilename: "LightPDF Studio.exe",
        InternalName: "LightPDF Studio",
      },
    );
    vi.outputToResourceEntries(res.entries);
  }
  res.outputResource(exeFile);
  fs.writeFileSync(exe, Buffer.from(exeFile.generate()));
  console.log("  • Symbol und Versionsinfo gesetzt:", path.basename(exe));
};
