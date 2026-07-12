# doc-processor Docker

A small pandoc document processor (Markdown → PDF) with:

- Web UI on port 8085
- API on port 8086
- Health endpoint at /health
- Debian 12 slim base image
- PDF rendering via [tectonic](https://tectonic-typesetting.github.io/) (a small,
  self-contained xetex-compatible engine — no texlive), pre-warmed at build time

> The image source folder is still `docker/media-processor/`; the command, image and
> container are named `doc-processor` / `vncli-doc-processor`.

## Start from vn (Windows)

Use the TUI action:

- vn app open doc-processor

This script builds the image, starts the container, waits for health, and opens the browser.

## Manual run

From repository root:

```bash
docker build -t vncli-doc-processor:latest -f docker/media-processor/Dockerfile .
docker rm -f vncli-doc-processor 2>/dev/null || true
docker run -d --rm --name vncli-doc-processor \
	-p 8085:8085 \
	-p 8086:8086 \
	-e HOST_DESKTOP_DIR=/host/Desktop \
	-v "$HOME/Desktop:/host/Desktop" \
	vncli-doc-processor:latest
```

On Windows PowerShell, use:

```powershell
docker build -t vncli-doc-processor:latest -f docker/media-processor/Dockerfile .
docker rm -f vncli-doc-processor 2>$null
docker run -d --rm --name vncli-doc-processor `
	-p 8085:8085 `
	-p 8086:8086 `
	-e HOST_DESKTOP_DIR=/host/Desktop `
	-v "${env:USERPROFILE}\Desktop:/host/Desktop" `
	vncli-doc-processor:latest
```

## URLs

- UI: http://localhost:8085
- API: http://localhost:8086
- Health: http://localhost:8086/health

## Pandoc workflow

- Drop Markdown files into the UI's path drop zone, then use **Pandoc Processor → Markdown to PDF**.
- Choose the LaTeX-style or Viewer-style profile plus font size, paper size, margin, TOC, etc.
- Generated PDFs are saved on the host Desktop (`HOST_DESKTOP_DIR`) in a
  `pandoc-YYYY-MM-DD-HH-MM-SS` folder.

## Operations

```bash
docker logs -f vncli-doc-processor
docker stop vncli-doc-processor
docker ps --filter "name=vncli-doc-processor"
```

## Troubleshooting

- If Docker is not running, start Docker Desktop and retry.
- If ports are busy, free 8085/8086 (the launcher is native: `vn app open doc-processor`).
