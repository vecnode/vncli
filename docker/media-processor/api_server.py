from __future__ import annotations

import os
import re
import subprocess
import tempfile
import time
from datetime import datetime, timezone
from pathlib import Path, PurePosixPath

from fastapi import FastAPI, File, Form, HTTPException, UploadFile
from fastapi.middleware.cors import CORSMiddleware

app = FastAPI(title="vncli doc-processor API", version="0.1.0")

app.add_middleware(
    CORSMiddleware,
    allow_origins=["http://localhost:8085", "http://127.0.0.1:8085"],
    allow_credentials=False,
    allow_methods=["*"],
    allow_headers=["*"],
)


@app.get("/health")
def health() -> dict[str, object]:
    return {
        "status": "ok",
        "service": "doc-processor-api",
        "time": datetime.now(timezone.utc).isoformat(),
    }


@app.get("/tools")
def tools() -> dict[str, str]:
    return {
        "python3": run_version(["python3", "--version"]),
        "pandoc": run_version(["pandoc", "--version"]),
    }


@app.get("/pandoc/version")
def pandoc_version() -> dict[str, str]:
    return {
        "version": run_full_output(["pandoc", "--version"]),
    }


@app.post("/pandoc/markdown-to-pdf")
async def pandoc_markdown_to_pdf(
    files: list[UploadFile] = File(...),
    paths: list[str] = Form(default=[]),
    links_black: bool = Form(default=False),
    font_size: str = Form(default=""),
    paper_size: str = Form(default=""),
    margin: str = Form(default=""),
    toc: bool = Form(default=False),
    number_sections: bool = Form(default=False),
) -> dict[str, object]:
    return await convert_markdown_to_pdf(
        files=files,
        paths=paths,
        mode="latex",
        links_black=links_black,
        font_size=font_size,
        paper_size=paper_size,
        margin=margin,
        toc=toc,
        number_sections=number_sections,
    )


@app.post("/pandoc/markdown-to-pdf-viewer")
async def pandoc_markdown_to_pdf_viewer(
    files: list[UploadFile] = File(...),
    paths: list[str] = Form(default=[]),
    links_black: bool = Form(default=False),
    font_size: str = Form(default=""),
    paper_size: str = Form(default=""),
    margin: str = Form(default=""),
    toc: bool = Form(default=False),
    number_sections: bool = Form(default=False),
) -> dict[str, object]:
    return await convert_markdown_to_pdf(
        files=files,
        paths=paths,
        mode="viewer",
        links_black=links_black,
        font_size=font_size,
        paper_size=paper_size,
        margin=margin,
        toc=toc,
        number_sections=number_sections,
    )


def natural_sort_key(value: str) -> list:
    """Sort key so 'ch2.pdf' comes before 'ch10.pdf'. Mixed chunks are wrapped
    in a (type, int, str) tuple so int and str never compare directly."""
    key: list = []
    for part in re.split(r"(\d+)", str(value).lower()):
        if part.isdigit():
            key.append((0, int(part), ""))
        elif part:
            key.append((1, 0, part))
    return key


def sanitize_pdf_name(name: str) -> str:
    name = os.path.basename(str(name or "").strip())
    name = re.sub(r'[<>:"/\\|?*\x00-\x1f]', "", name).strip()
    if not name:
        return ""
    if not name.lower().endswith(".pdf"):
        name += ".pdf"
    return name


@app.post("/pdf/join")
async def pdf_join(
    files: list[UploadFile] = File(...),
    paths: list[str] = Form(default=[]),
    output_name: str = Form(default=""),
) -> dict[str, object]:
    """Merge the uploaded PDFs into one, ordered by their file path/name."""
    try:
        from pypdf import PdfWriter
    except Exception as exc:  # pragma: no cover
        raise HTTPException(status_code=500, detail=f"pypdf is not available: {exc}") from exc

    started_at = time.perf_counter()

    items: list[tuple[str, bytes]] = []
    for index, upload in enumerate(files):
        submitted = paths[index] if index < len(paths) else (upload.filename or f"file-{index}.pdf")
        if not str(submitted).lower().endswith(".pdf"):
            continue
        items.append((submitted, await upload.read()))

    if len(items) < 2:
        raise HTTPException(
            status_code=400,
            detail="Select a folder with at least two PDF files to join.",
        )

    items.sort(key=lambda kv: natural_sort_key(kv[0]))

    output_base, output_note = get_output_base_dir()
    timestamp = datetime.now().strftime("%Y-%m-%d-%H-%M-%S")
    output_dir = output_base / f"pdf-join-{timestamp}"
    output_dir.mkdir(parents=True, exist_ok=False)
    out_name = sanitize_pdf_name(output_name) or "joined.pdf"
    output_pdf = output_dir / out_name

    order: list[str] = []
    writer = PdfWriter()
    try:
        with tempfile.TemporaryDirectory(prefix="pdf-join-") as temp_root:
            for i, (submitted, raw) in enumerate(items):
                src = Path(temp_root) / f"{i:04d}.pdf"
                src.write_bytes(raw)
                try:
                    writer.append(str(src))
                except Exception as exc:
                    raise HTTPException(
                        status_code=400,
                        detail=f"Could not read PDF {PurePosixPath(submitted).name}: {exc}",
                    ) from exc
                order.append(PurePosixPath(submitted).name)
            with open(output_pdf, "wb") as fh:
                writer.write(fh)
    finally:
        writer.close()

    return {
        "joined_count": len(items),
        "order": order,
        "output_folder": str(output_dir),
        "output_file": out_name,
        "duration_seconds": round(time.perf_counter() - started_at, 2),
        "note": output_note,
    }


async def convert_markdown_to_pdf(
    files: list[UploadFile],
    paths: list[str],
    mode: str,
    links_black: bool = False,
    font_size: str = "",
    paper_size: str = "",
    margin: str = "",
    toc: bool = False,
    number_sections: bool = False,
) -> dict[str, object]:
    if not files:
        raise HTTPException(status_code=400, detail="No files received.")

    started_at = time.perf_counter()

    output_base, output_note = get_output_base_dir()
    timestamp = datetime.now().strftime("%Y-%m-%d-%H-%M-%S")
    output_dir = output_base / f"pandoc-{timestamp}"
    output_dir.mkdir(parents=True, exist_ok=False)

    converted_files: list[str] = []
    uploaded_count = 0

    with tempfile.TemporaryDirectory(prefix="pandoc-md-") as temp_root:
        temp_root_path = Path(temp_root)
        markdown_inputs: list[Path] = []

        for index, upload in enumerate(files):
            raw = await upload.read()
            uploaded_count += 1

            submitted_path = paths[index] if index < len(paths) else upload.filename
            safe_relative = safe_relative_path(submitted_path or upload.filename or f"file-{index}.md")

            input_path = temp_root_path / safe_relative
            input_path.parent.mkdir(parents=True, exist_ok=True)

            if safe_relative.suffix.lower() in {".md", ".markdown"}:
                input_path.write_bytes(normalize_markdown_frontmatter_bytes(raw))
                markdown_inputs.append(safe_relative)
            else:
                input_path.write_bytes(raw)

        for safe_relative in markdown_inputs:
            input_path = temp_root_path / safe_relative

            output_pdf_rel = safe_relative.with_suffix(".pdf")
            output_pdf = output_dir / output_pdf_rel
            output_pdf.parent.mkdir(parents=True, exist_ok=True)

            try:
                run_markdown_pdf_command(
                    input_path=input_path,
                    output_pdf=output_pdf,
                    mode=mode,
                    links_black=links_black,
                    font_size=font_size,
                    paper_size=paper_size,
                    margin=margin,
                    toc=toc,
                    number_sections=number_sections,
                )
            except subprocess.CalledProcessError as exc:
                output_text = (exc.output or "").strip() or str(exc)
                raise HTTPException(
                    status_code=500,
                    detail=f"Pandoc failed for {safe_relative.as_posix()}: {output_text}",
                ) from exc

            converted_files.append(output_pdf_rel.as_posix())

    if not converted_files:
        try:
            output_dir.rmdir()
        except OSError:
            pass
        raise HTTPException(status_code=400, detail="No Markdown files found (.md or .markdown).")

    return {
        "uploaded_count": uploaded_count,
        "converted_count": len(converted_files),
        "output_folder": str(output_dir),
        "converted_files": converted_files,
        "engine": "tectonic",
        "duration_seconds": round(time.perf_counter() - started_at, 2),
        "note": output_note,
    }


def run_version(command: list[str]) -> str:
    try:
        output = subprocess.check_output(command, stderr=subprocess.STDOUT, text=True)
        first_line = output.splitlines()[0] if output else "unknown"
        return first_line.strip()
    except Exception:
        return "unavailable"


def run_full_output(command: list[str]) -> str:
    try:
        output = subprocess.check_output(command, stderr=subprocess.STDOUT, text=True)
        return output.strip() if output else "unknown"
    except Exception:
        return "unavailable"


def safe_relative_path(path_value: str) -> Path:
    candidate = str(path_value or "").replace("\\", "/").strip()
    pure = PurePosixPath(candidate)
    safe_parts = [part for part in pure.parts if part not in {"", ".", ".."}]
    if not safe_parts:
        return Path("input.md")
    return Path(*safe_parts)


def normalize_markdown_frontmatter_bytes(raw: bytes) -> bytes:
    try:
        text = raw.decode("utf-8-sig")
    except UnicodeDecodeError:
        return raw

    normalized = normalize_yaml_frontmatter(text)
    return normalized.encode("utf-8")


def normalize_yaml_frontmatter(text: str) -> str:
    lines = text.splitlines(keepends=True)
    if not lines:
        return text

    if lines[0].strip() != "---":
        return text

    end_index = -1
    for i in range(1, len(lines)):
        marker = lines[i].strip()
        if marker in {"---", "..."}:
            end_index = i
            break

    if end_index == -1:
        return text

    # Quote scalar values like: title: Week 1: Intro (contains colon) so YAML parses safely.
    frontmatter = lines[1:end_index]
    for i, line in enumerate(frontmatter):
        newline = "\n" if line.endswith("\n") else ""
        base = line[:-1] if newline else line

        match = re.match(r"^(\s*[A-Za-z0-9_-]+\s*:\s*)(.+?)\s*$", base)
        if not match:
            continue

        prefix, value = match.group(1), match.group(2).strip()
        if not value:
            continue

        if value[0] in {'"', "'", "[", "{", "|", ">", "!", "&", "*"}:
            continue

        if ":" not in value:
            continue

        escaped = value.replace("\\", "\\\\").replace('"', '\\"')
        frontmatter[i] = f'{prefix}"{escaped}"{newline}'

    rebuilt = [lines[0], *frontmatter, *lines[end_index:]]
    return "".join(rebuilt)


def get_output_base_dir() -> tuple[Path, str]:
    configured_host_desktop = os.environ.get("HOST_DESKTOP_DIR", "").strip()
    if configured_host_desktop:
        raw = configured_host_desktop.replace("\\", "/")
        candidates: list[str] = [raw]

        # Git Bash may rewrite '/host/Desktop' into 'C:/msys64/host/Desktop'.
        lower_raw = raw.lower()
        anchor = "/host/desktop"
        if anchor in lower_raw:
            start = lower_raw.find(anchor)
            candidates.append(raw[start:])

        for candidate in candidates:
            path_candidate = Path(candidate)
            try:
                path_candidate.mkdir(parents=True, exist_ok=True)
                return path_candidate, "Saved to HOST_DESKTOP_DIR."
            except OSError:
                continue

    local_desktop = Path.home() / "Desktop"
    if local_desktop.exists():
        return local_desktop, "Saved to local Desktop."

    fallback = Path("/outputs")
    fallback.mkdir(parents=True, exist_ok=True)
    return (
        fallback,
        "HOST_DESKTOP_DIR is not set; saved to /outputs inside container/runtime.",
    )


def run_markdown_pdf_command(
    input_path: Path,
    output_pdf: Path,
    mode: str,
    links_black: bool = False,
    font_size: str = "",
    paper_size: str = "",
    margin: str = "",
    toc: bool = False,
    number_sections: bool = False,
) -> None:
    resource_dir = str(input_path.parent)
    normalized_font_size = str(font_size or "").strip().lower()
    allowed_sizes = {"10pt", "11pt", "12pt"}
    selected_font_size = normalized_font_size if normalized_font_size in allowed_sizes else ""

    normalized_paper_size = str(paper_size or "").strip().lower()
    allowed_paper_sizes = {"a4", "letter"}
    selected_paper_size = normalized_paper_size if normalized_paper_size in allowed_paper_sizes else ""

    normalized_margin = str(margin or "").strip().lower()
    allowed_margins = {"0.75in", "1in", "1.25in"}
    selected_margin = normalized_margin if normalized_margin in allowed_margins else "1in"

    link_color = "black" if links_black else "blue"

    if mode == "viewer":
        viewer_args = [
            "pandoc",
            str(input_path),
            "--from=gfm",
            "--resource-path",
            resource_dir,
            "--pdf-engine=tectonic",
            "-V",
            "mainfont=Latin Modern Sans",
            "-V",
            "sansfont=Latin Modern Sans",
        ]
        if toc:
            viewer_args.append("--toc")
        if number_sections:
            viewer_args.append("--number-sections")
        if selected_font_size:
            viewer_args.extend(["-V", f"fontsize={selected_font_size}"])
        else:
            viewer_args.extend(["-V", "fontsize=11pt"])
        if selected_paper_size:
            viewer_args.extend(["-V", f"papersize={selected_paper_size}"])
        viewer_args.extend(
            [
                "-V",
                f"geometry:margin={selected_margin}",
                "-V",
                "colorlinks=true",
                "-V",
                f"urlcolor={link_color}",
                "--highlight-style=tango",
                "-o",
                str(output_pdf),
            ]
        )
        subprocess.check_output(
            viewer_args,
            stderr=subprocess.STDOUT,
            text=True,
            cwd=resource_dir,
        )
        return

    latex_args = [
        "pandoc",
        str(input_path),
        "--resource-path",
        resource_dir,
        "--pdf-engine=tectonic",
    ]
    if toc:
        latex_args.append("--toc")
    if number_sections:
        latex_args.append("--number-sections")
    if selected_font_size:
        latex_args.extend(["-V", f"fontsize={selected_font_size}"])
    if selected_paper_size:
        latex_args.extend(["-V", f"papersize={selected_paper_size}"])
    latex_args.extend(["-V", f"geometry:margin={selected_margin}"])
    if links_black:
        latex_args.extend(["-V", "colorlinks=true", "-V", "urlcolor=black"])
    latex_args.extend(["-o", str(output_pdf)])

    subprocess.check_output(
        latex_args,
        stderr=subprocess.STDOUT,
        text=True,
        cwd=resource_dir,
    )
