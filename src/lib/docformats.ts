import mammoth from "mammoth";
import JSZip from "jszip";
import { DocumentFormat, ParsedDocument, ParsedDocumentPage } from "../types";

/**
 * Detects document format from file name or MIME type
 */
export function detectDocumentFormat(fileName: string, mimeType?: string): DocumentFormat {
  const lower = fileName.toLowerCase();
  if (lower.endsWith(".docx") || mimeType?.includes("wordprocessingml")) {
    return "docx";
  }
  if (lower.endsWith(".epub") || mimeType?.includes("epub")) {
    return "epub";
  }
  return "pdf";
}

/**
 * Parses Word (.docx) document into paginated pages
 */
export async function parseDocxFile(file: File): Promise<ParsedDocument> {
  const arrayBuffer = await file.arrayBuffer();
  
  // Convert docx to HTML using mammoth
  const result = await mammoth.convertToHtml(
    { arrayBuffer },
    {
      styleMap: [
        "p[style-name='Heading 1'] => h1:fresh",
        "p[style-name='Heading 2'] => h2:fresh",
        "p[style-name='Heading 3'] => h3:fresh",
        "p[style-name='Title'] => h1.title:fresh",
        "p[style-name='Subtitle'] => h2.subtitle:fresh",
      ],
    }
  );

  const rawHtml = result.value || "<p>Empty Word document</p>";

  // Paginate the HTML content into reading pages
  const pages = paginateHtmlContent(rawHtml, file.name);

  return {
    id: `docx-${Date.now()}`,
    title: file.name.replace(/\.docx$/i, ""),
    format: "docx",
    totalPages: pages.length,
    pages,
    rawText: result.messages.map((m) => m.message).join("\n"),
  };
}

/**
 * Parses EPUB ebook archive into readable chapters and pages
 */
export async function parseEpubFile(file: File): Promise<ParsedDocument> {
  const arrayBuffer = await file.arrayBuffer();
  const zip = await JSZip.loadAsync(arrayBuffer);

  // 1. Locate container.xml to discover OPF file location
  const containerXml = await zip.file("META-INF/container.xml")?.async("text");
  let opfPath = "OEBPS/content.opf";
  
  if (containerXml) {
    const parser = new DOMParser();
    const doc = parser.parseFromString(containerXml, "application/xml");
    const rootfile = doc.querySelector("rootfile");
    const fullPath = rootfile?.getAttribute("full-path");
    if (fullPath) {
      opfPath = fullPath;
    }
  }

  // Fallback scan if opfPath not found
  let opfFile = zip.file(opfPath);
  if (!opfFile) {
    const opfEntry = Object.keys(zip.files).find((p) => p.endsWith(".opf"));
    if (opfEntry) {
      opfPath = opfEntry;
      opfFile = zip.file(opfEntry);
    }
  }

  if (!opfFile) {
    throw new Error("Invalid EPUB format: metadata package (.opf) not found");
  }

  const opfText = await opfFile.async("text");
  const parser = new DOMParser();
  const opfDoc = parser.parseFromString(opfText, "application/xml");

  // Determine base directory of OPF
  const opfBaseDir = opfPath.includes("/") ? opfPath.substring(0, opfPath.lastIndexOf("/") + 1) : "";

  // 2. Extract title
  const titleEl = opfDoc.querySelector("title");
  const docTitle = titleEl?.textContent?.trim() || file.name.replace(/\.epub$/i, "");

  // 3. Extract manifest items
  const manifestItems: Record<string, { href: string; mediaType: string }> = {};
  const itemEls = opfDoc.querySelectorAll("manifest > item");
  itemEls.forEach((item) => {
    const id = item.getAttribute("id");
    const href = item.getAttribute("href");
    const mediaType = item.getAttribute("media-type") || "";
    if (id && href) {
      // Resolve path relative to OPF base dir
      const resolvedHref = resolveZipPath(opfBaseDir, href);
      manifestItems[id] = { href: resolvedHref, mediaType };
    }
  });

  // 4. Extract spine itemrefs (chapter sequence)
  const spineItemRefs = opfDoc.querySelectorAll("spine > itemref");
  const spineHrefs: string[] = [];

  spineItemRefs.forEach((ref) => {
    const idref = ref.getAttribute("idref");
    if (idref && manifestItems[idref]) {
      spineHrefs.push(manifestItems[idref].href);
    }
  });

  // If no spine items found, fallback to all html/xhtml in manifest
  if (spineHrefs.length === 0) {
    Object.values(manifestItems).forEach((item) => {
      if (item.mediaType.includes("html") || item.href.endsWith(".html") || item.href.endsWith(".xhtml")) {
        spineHrefs.push(item.href);
      }
    });
  }

  // 5. Pre-extract image files to object URLs for inline rendering
  const imageUrlMap: Record<string, string> = {};
  for (const [_, item] of Object.entries(manifestItems)) {
    if (item.mediaType.startsWith("image/")) {
      const imgEntry = zip.file(item.href);
      if (imgEntry) {
        try {
          const blob = await imgEntry.async("blob");
          imageUrlMap[item.href] = URL.createObjectURL(blob);
          // Also store bare filename
          const bareName = item.href.split("/").pop();
          if (bareName) {
            imageUrlMap[bareName] = imageUrlMap[item.href];
          }
        } catch (e) {
          console.warn("Could not load epub image:", item.href, e);
        }
      }
    }
  }

  // 6. Load spine chapters into pages
  const pages: ParsedDocumentPage[] = [];

  for (let i = 0; i < spineHrefs.length; i++) {
    const chapterHref = spineHrefs[i];
    const chapterEntry = zip.file(chapterHref);
    if (!chapterEntry) continue;

    try {
      const chapterHtml = await chapterEntry.async("text");
      const chapterDoc = parser.parseFromString(chapterHtml, "text/html");
      
      // Fix images src to use object URLs
      const imgs = chapterDoc.querySelectorAll("img, image");
      imgs.forEach((img) => {
        const src = img.getAttribute("src") || img.getAttribute("xlink:href");
        if (src) {
          const resolvedSrc = resolveZipPath(
            chapterHref.includes("/") ? chapterHref.substring(0, chapterHref.lastIndexOf("/") + 1) : "",
            src
          );
          const bare = src.split("/").pop();
          const mapped = imageUrlMap[resolvedSrc] || (bare ? imageUrlMap[bare] : null);
          if (mapped) {
            img.setAttribute("src", mapped);
          }
        }
      });

      // Extract body content
      const body = chapterDoc.body;
      const chapterTitle = chapterDoc.querySelector("h1, h2, title")?.textContent?.trim() || `Chapter ${i + 1}`;

      if (body) {
        // Break chapter into pages if long, or keep as a clean chapter page
        const chapterPages = paginateHtmlContent(body.innerHTML, chapterTitle);
        pages.push(...chapterPages);
      }
    } catch (err) {
      console.warn("Failed to parse chapter:", chapterHref, err);
    }
  }

  // If pages are still empty, create at least 1 page
  if (pages.length === 0) {
    pages.push({
      pageNumber: 1,
      title: docTitle,
      htmlContent: `<div class="p-6"><h1>${docTitle}</h1><p>EPUB content loaded.</p></div>`,
    });
  }

  // Renumber pages sequentially
  pages.forEach((p, idx) => {
    p.pageNumber = idx + 1;
  });

  return {
    id: `epub-${Date.now()}`,
    title: docTitle,
    format: "epub",
    totalPages: pages.length,
    pages,
  };
}

/**
 * Resolves relative file path in a zip archive
 */
function resolveZipPath(baseDir: string, relativePath: string): string {
  // If absolute path or url, clean it
  if (relativePath.startsWith("/")) {
    relativePath = relativePath.substring(1);
  }
  const parts = (baseDir + relativePath).split("/");
  const resolved: string[] = [];

  for (const part of parts) {
    if (part === "" || part === ".") continue;
    if (part === "..") {
      resolved.pop();
    } else {
      resolved.push(part);
    }
  }

  return resolved.join("/");
}

/**
 * Paginates an HTML string into structured book pages
 */
function paginateHtmlContent(htmlString: string, docTitle: string): ParsedDocumentPage[] {
  const parser = new DOMParser();
  const doc = parser.parseFromString(`<div>${htmlString}</div>`, "text/html");
  const container = doc.body.firstElementChild || doc.body;

  const children = Array.from(container.children);
  if (children.length === 0) {
    return [
      {
        pageNumber: 1,
        title: docTitle,
        htmlContent: htmlString,
      },
    ];
  }

  const pages: ParsedDocumentPage[] = [];
  let currentPageBlocks: string[] = [];
  let currentWordCount = 0;
  const TARGET_WORDS_PER_PAGE = 350;

  children.forEach((el, index) => {
    const text = el.textContent || "";
    const words = text.trim().split(/\s+/).filter(Boolean).length;
    const isHeading = ["H1", "H2"].includes(el.tagName);

    // If adding this block exceeds word limit or if it's a major heading (and current page isn't empty), start new page
    if ((currentWordCount + words > TARGET_WORDS_PER_PAGE && currentWordCount > 150) || (isHeading && currentWordCount > 200)) {
      pages.push({
        pageNumber: pages.length + 1,
        title: docTitle,
        htmlContent: currentPageBlocks.join("\n"),
      });
      currentPageBlocks = [];
      currentWordCount = 0;
    }

    currentPageBlocks.push(el.outerHTML);
    currentWordCount += words;
  });

  if (currentPageBlocks.length > 0) {
    pages.push({
      pageNumber: pages.length + 1,
      title: docTitle,
      htmlContent: currentPageBlocks.join("\n"),
    });
  }

  return pages.length > 0
    ? pages
    : [
        {
          pageNumber: 1,
          title: docTitle,
          htmlContent: htmlString,
        },
      ];
}
