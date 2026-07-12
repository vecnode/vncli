// --- health check (existing) ---
const statusNode = document.getElementById("status");
const button = document.getElementById("check");

async function checkHealth() {
  statusNode.textContent = "Checking API...";
  try {
    const response = await fetch("http://localhost:8086/health");
    if (!response.ok) throw new Error("HTTP " + response.status);
    const data = await response.json();
    statusNode.textContent = "API is healthy at " + data.time;
  } catch (error) {
    statusNode.textContent = "API check failed: " + error.message;
  }
}

button.addEventListener("click", checkHealth);
checkHealth();

// --- right panel drop zone (new) ---
const dropZone = document.getElementById("dropZone");
const pathListNode = document.getElementById("pathList");
const pickFilesBtn = document.getElementById("pickFilesBtn");
const pickFolderBtn = document.getElementById("pickFolderBtn");
const clearPathsBtn = document.getElementById("clearPathsBtn");
const pickFilesInput = document.getElementById("pickFilesInput");
const pickFolderInput = document.getElementById("pickFolderInput");

const collectedPathSet = new Set();
const collectedFileMap = new Map();

function isMarkdownPath(pathValue) {
  return /\.(md|markdown)$/i.test(String(pathValue || ""));
}

function normalizePath(pathValue) {
  return String(pathValue || "")
    .replace(/\\\\/g, "/")
    .replace(/^\/+/, "")
    .trim();
}

function addPath(pathValue) {
  const normalized = normalizePath(pathValue);
  if (!normalized) return;
  collectedPathSet.add(normalized);
}

function addCollectedFile(pathValue, file) {
  const normalized = normalizePath(pathValue);
  if (!normalized || !file) return;
  collectedFileMap.set(normalized, file);
  addPath(normalized);
}

function renderPathList(message) {
  if (message) {
    pathListNode.textContent = message;
    return;
  }

  const values = Array.from(collectedPathSet).sort((a, b) =>
    a.localeCompare(b, undefined, { sensitivity: "base" })
  );
  pathListNode.textContent = values.length
    ? values.join("\n")
    : "No files selected yet.";
}

function addFilesFromList(fileList) {
  for (const file of Array.from(fileList || [])) {
    const pathValue = file.webkitRelativePath || file.name;
    addCollectedFile(pathValue, file);
  }
  renderPathList();
}

function walkFileSystemEntry(entry, basePath = "") {
  return new Promise((resolve) => {
    if (!entry) {
      resolve();
      return;
    }

    const safeName = normalizePath(entry.name || "");
    const nextBase = basePath ? `${basePath}/${safeName}` : safeName;

    if (entry.isFile) {
      entry.file(
        (file) => {
          addCollectedFile(nextBase, file);
          resolve();
        },
        () => {
          resolve();
        }
      );
      return;
    }

    if (entry.isDirectory) {
      const reader = entry.createReader();
      const readBatch = () => {
        reader.readEntries(async (entries) => {
          if (!entries.length) {
            resolve();
            return;
          }
          for (const child of entries) {
            await walkFileSystemEntry(child, nextBase);
          }
          readBatch();
        }, resolve);
      };
      readBatch();
      return;
    }

    resolve();
  });
}

async function handleDrop(event) {
  event.preventDefault();
  dropZone.classList.remove("active");

  const dt = event.dataTransfer;
  if (!dt) {
    renderPathList("Drop failed: no data transfer.");
    return;
  }

  const items = Array.from(dt.items || []);
  const supportsEntries = items.some((item) => typeof item.webkitGetAsEntry === "function");

  if (supportsEntries) {
    for (const item of items) {
      const entry = typeof item.webkitGetAsEntry === "function" ? item.webkitGetAsEntry() : null;
      if (entry) {
        await walkFileSystemEntry(entry);
      }
    }
    renderPathList();
    return;
  }

  // Fallback when directory entries are not available in the browser.
  addFilesFromList(dt.files);
}

function setDropZoneActive(isActive) {
  dropZone.classList.toggle("active", isActive);
}

dropZone.addEventListener("dragenter", (event) => {
  event.preventDefault();
  setDropZoneActive(true);
});

dropZone.addEventListener("dragover", (event) => {
  event.preventDefault();
  setDropZoneActive(true);
});

dropZone.addEventListener("dragleave", (event) => {
  if (!dropZone.contains(event.relatedTarget)) {
    setDropZoneActive(false);
  }
});

dropZone.addEventListener("drop", handleDrop);

dropZone.addEventListener("click", () => {
  pickFilesInput.click();
});

dropZone.addEventListener("keydown", (event) => {
  if (event.key === "Enter" || event.key === " ") {
    event.preventDefault();
    pickFilesInput.click();
  }
});

pickFilesBtn.addEventListener("click", () => {
  pickFilesInput.click();
});

pickFolderBtn.addEventListener("click", () => {
  pickFolderInput.click();
});

clearPathsBtn.addEventListener("click", () => {
  collectedPathSet.clear();
  collectedFileMap.clear();
  renderPathList();
});

pickFilesInput.addEventListener("change", () => {
  addFilesFromList(pickFilesInput.files);
  pickFilesInput.value = "";
});

pickFolderInput.addEventListener("change", () => {
  addFilesFromList(pickFolderInput.files);
  pickFolderInput.value = "";
});

// --- pandoc processor ---
const pandocVersionBtn = document.getElementById("pandocVersionBtn");
const pandocConvertBtn = document.getElementById("pandocConvertBtn");
const pandocPdfModeSelect = document.getElementById("pandocPdfModeSelect");
const pandocFontSizeSelect = document.getElementById("pandocFontSizeSelect");
const pandocBlackLinksToggle = document.getElementById("pandocBlackLinksToggle");
const pandocPaperSizeSelect = document.getElementById("pandocPaperSizeSelect");
const pandocMarginSelect = document.getElementById("pandocMarginSelect");
const pandocTocToggle = document.getElementById("pandocTocToggle");
const pandocNumberSectionsToggle = document.getElementById("pandocNumberSectionsToggle");
const pandocConvertStatus = document.getElementById("pandocConvertStatus");
const pandocStatusOutput = pandocConvertStatus;

function formatSeconds(value) {
  return `${Math.max(0, value).toFixed(1)}s`;
}

if (pandocVersionBtn && pandocStatusOutput) {
  pandocVersionBtn.addEventListener("click", async () => {
    pandocStatusOutput.textContent = "Loading pandoc version...";
    try {
      const res = await fetch("http://localhost:8086/pandoc/version");
      if (!res.ok) throw new Error("HTTP " + res.status);
      const data = await res.json();
      pandocStatusOutput.textContent = data.version || "unavailable";
    } catch (error) {
      pandocStatusOutput.textContent = "Error: " + error.message;
    }
  });
}

if (pandocConvertBtn) {
  pandocConvertBtn.addEventListener("click", async () => {
    const selectedMode = String(pandocPdfModeSelect?.value || "latex");
    const endpoint = selectedMode === "viewer"
      ? "http://localhost:8086/pandoc/markdown-to-pdf-viewer"
      : "http://localhost:8086/pandoc/markdown-to-pdf";
    const modeLabel = selectedMode === "viewer" ? "Viewer-style" : "LaTeX-style";

    await runMarkdownToPdfConversion({
      endpoint,
      modeLabel,
      options: {
        linksBlack: Boolean(pandocBlackLinksToggle?.checked),
        fontSize: String(pandocFontSizeSelect?.value || "").trim(),
        paperSize: String(pandocPaperSizeSelect?.value || "").trim(),
        margin: String(pandocMarginSelect?.value || "").trim(),
        toc: Boolean(pandocTocToggle?.checked),
        numberSections: Boolean(pandocNumberSectionsToggle?.checked),
      },
    });
  });
}

async function runMarkdownToPdfConversion({ endpoint, modeLabel, options = {} }) {
  if (!pandocStatusOutput) {
    return;
  }

  const markdownEntries = Array.from(collectedFileMap.entries()).filter(([pathValue]) =>
    isMarkdownPath(pathValue)
  );

  if (!markdownEntries.length) {
    pandocStatusOutput.textContent = "No Markdown files found in the current paths.";
    return;
  }

  pandocStatusOutput.textContent =
    `Preparing ${markdownEntries.length} Markdown file(s)...\n` +
    `Mode: ${modeLabel}`;

  const form = new FormData();
  for (const [pathValue, file] of markdownEntries) {
    form.append("files", file, pathValue);
    form.append("paths", pathValue);
  }
  form.append("links_black", options.linksBlack ? "true" : "false");
  form.append("toc", options.toc ? "true" : "false");
  form.append("number_sections", options.numberSections ? "true" : "false");
  if (options.fontSize) {
    form.append("font_size", options.fontSize);
  }
  if (options.paperSize) {
    form.append("paper_size", options.paperSize);
  }
  if (options.margin) {
    form.append("margin", options.margin);
  }

  const selectedFontSize = options.fontSize || "default";
  const linksLabel = options.linksBlack ? "black" : "default";
  const selectedPaperSize = options.paperSize || "default";
  const selectedMargin = options.margin || "default";
  const tocLabel = options.toc ? "on" : "off";
  const numberSectionsLabel = options.numberSections ? "on" : "off";

  pandocStatusOutput.textContent =
    `Uploading ${markdownEntries.length} Markdown file(s) to API...\n` +
    `Mode: ${modeLabel}\n` +
    `Font size: ${selectedFontSize}\n` +
    `Paper size: ${selectedPaperSize}\n` +
    `Margin: ${selectedMargin}\n` +
    `TOC: ${tocLabel}\n` +
    `Number sections: ${numberSectionsLabel}\n` +
    `Link color: ${linksLabel}`;

  const requestStartedAt = performance.now();
  const progressTimer = window.setInterval(() => {
    const elapsed = (performance.now() - requestStartedAt) / 1000;
    pandocStatusOutput.textContent =
      `Converting ${markdownEntries.length} Markdown file(s)...\n` +
      `Mode: ${modeLabel}\n` +
      `Font size: ${selectedFontSize}\n` +
      `Paper size: ${selectedPaperSize}\n` +
      `Margin: ${selectedMargin}\n` +
      `TOC: ${tocLabel}\n` +
      `Number sections: ${numberSectionsLabel}\n` +
      `Link color: ${linksLabel}\n` +
      `Elapsed: ${formatSeconds(elapsed)}`;
  }, 700);

  try {
    const res = await fetch(endpoint, {
      method: "POST",
      body: form,
    });
    if (!res.ok) {
      const err = await res.json().catch(() => ({ detail: res.statusText }));
      throw new Error(err.detail || res.statusText);
    }

    const data = await res.json();
    const lines = [
      `Mode: ${modeLabel}`,
      `Font size: ${selectedFontSize}`,
      `Paper size: ${selectedPaperSize}`,
      `Margin: ${selectedMargin}`,
      `TOC: ${tocLabel}`,
      `Number sections: ${numberSectionsLabel}`,
      `Link color: ${linksLabel}`,
      `Output folder: ${data.output_folder}`,
      `Converted: ${data.converted_count}`,
      `Engine: ${data.engine || "pandoc"}`,
      `Time: ${formatSeconds(Number(data.duration_seconds || 0))}`,
    ];

    if (Array.isArray(data.converted_files) && data.converted_files.length) {
      lines.push("", "PDF files:", ...data.converted_files);
    }

    if (data.note) {
      lines.push("", `Note: ${data.note}`);
    }

    pandocStatusOutput.textContent = lines.join("\n");
  } catch (error) {
    pandocStatusOutput.textContent = "Error: " + error.message;
  } finally {
    window.clearInterval(progressTimer);
  }
}

// --- join PDFs (by name) ---
const joinPdfBtn = document.getElementById("joinPdfBtn");
const joinStatus = document.getElementById("joinStatus");
const joinOutputName = document.getElementById("joinOutputName");

function isPdfPath(pathValue) {
  return /\.pdf$/i.test(String(pathValue || ""));
}

if (joinPdfBtn && joinStatus) {
  joinPdfBtn.addEventListener("click", async () => {
    const pdfEntries = Array.from(collectedFileMap.entries())
      .filter(([pathValue]) => isPdfPath(pathValue))
      .sort(([a], [b]) => a.localeCompare(b, undefined, { numeric: true, sensitivity: "base" }));

    if (pdfEntries.length < 2) {
      joinStatus.textContent =
        "Select a folder with at least two PDF files first (use 'Choose Folder' above).";
      return;
    }

    joinStatus.textContent =
      `Joining ${pdfEntries.length} PDF(s) in this order:\n` +
      pdfEntries.map(([pathValue], i) => `${i + 1}. ${pathValue}`).join("\n");

    const form = new FormData();
    for (const [pathValue, file] of pdfEntries) {
      form.append("files", file, pathValue);
      form.append("paths", pathValue);
    }
    const outName = String(joinOutputName?.value || "").trim();
    if (outName) form.append("output_name", outName);

    const startedAt = performance.now();
    const timer = window.setInterval(() => {
      const elapsed = (performance.now() - startedAt) / 1000;
      joinStatus.textContent =
        `Joining ${pdfEntries.length} PDF(s)...\nElapsed: ${formatSeconds(elapsed)}`;
    }, 700);

    try {
      const res = await fetch("http://localhost:8086/pdf/join", { method: "POST", body: form });
      if (!res.ok) {
        const err = await res.json().catch(() => ({ detail: res.statusText }));
        throw new Error(err.detail || res.statusText);
      }
      const data = await res.json();
      const lines = [
        `Joined ${data.joined_count} PDF(s)`,
        `Output folder: ${data.output_folder}`,
        `Output file: ${data.output_file}`,
        `Time: ${formatSeconds(Number(data.duration_seconds || 0))}`,
      ];
      if (Array.isArray(data.order) && data.order.length) {
        lines.push("", "Merge order:", ...data.order.map((n, i) => `${i + 1}. ${n}`));
      }
      if (data.note) {
        lines.push("", `Note: ${data.note}`);
      }
      joinStatus.textContent = lines.join("\n");
    } catch (error) {
      joinStatus.textContent = "Error: " + error.message;
    } finally {
      window.clearInterval(timer);
    }
  });
}
