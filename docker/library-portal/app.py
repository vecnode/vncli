#!/usr/bin/env python3
"""library-portal — a light web viewer/manager for the vncli library/ folder.

Serves an Anthropic-style index of the PDFs under /library (bind-mounted at
runtime) and streams each file for in-browser viewing. It also lets you:

  * edit a document's display metadata (title / author / year)
  * rename the file on disk
  * add tags (e.g. "read")
  * switch between list and grid (thumbnail) views
  * sort by title (A-Z / Z-A) or year

App state (metadata overrides + tags) lives in a small sidecar file at
/library/.portal/portal.json, and thumbnails are cached under
/library/.portal/thumbs/. The PDFs themselves are only modified on an explicit
rename. Thumbnails are rendered with PyMuPDF if available; otherwise the grid
view shows a simple placeholder.
"""
import hashlib
import html
import json
import os
import re
import threading
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

try:
    import fitz  # PyMuPDF, for thumbnails
    fitz.TOOLS.mupdf_display_errors(False)
except Exception:  # pragma: no cover
    fitz = None

LIBRARY = os.path.realpath(os.environ.get("LIBRARY_DIR", "/library"))
PORT = int(os.environ.get("PORT", "8090"))
PORTAL_DIR = os.path.join(LIBRARY, ".portal")
SIDECAR = os.path.join(PORTAL_DIR, "portal.json")
THUMB_DIR = os.path.join(PORTAL_DIR, "thumbs")

NAME_RE = re.compile(r"^(\d{4})-([A-Za-z][A-Za-z0-9]*)-(.+)$")
_LOCK = threading.Lock()


# --------------------------------------------------------------------------- #
# helpers
# --------------------------------------------------------------------------- #
def human_size(n: int) -> str:
    value = float(n)
    for unit in ("B", "KB", "MB", "GB"):
        if value < 1024 or unit == "GB":
            return f"{value:.0f} {unit}" if unit == "B" else f"{value:.1f} {unit}"
        value /= 1024
    return f"{n} B"


def split_camel(text: str) -> str:
    text = text.replace("_", " ")
    text = re.sub(r"(?<=[a-z0-9])(?=[A-Z])", " ", text)
    text = re.sub(r"(?<=[A-Za-z])(?=[0-9])", " ", text)
    return text.strip()


def load_sidecar() -> dict:
    try:
        with open(SIDECAR, encoding="utf-8") as fh:
            data = json.load(fh)
            return data if isinstance(data, dict) else {}
    except Exception:
        return {}


def save_sidecar(data: dict) -> None:
    os.makedirs(PORTAL_DIR, exist_ok=True)
    tmp = SIDECAR + ".tmp"
    with open(tmp, "w", encoding="utf-8") as fh:
        json.dump(data, fh, ensure_ascii=False, indent=1)
    os.replace(tmp, SIDECAR)


def norm_tags(raw) -> list:
    if isinstance(raw, str):
        parts = re.split(r"[,\s]+", raw)
    elif isinstance(raw, list):
        parts = raw
    else:
        parts = []
    out, seen = [], set()
    for p in parts:
        t = str(p).strip().lstrip("#").lower()
        t = re.sub(r"[^a-z0-9_-]", "", t)
        if t and t not in seen:
            seen.add(t)
            out.append(t)
    return out


def list_pdfs() -> list:
    side = load_sidecar()
    items = []
    for root, dirs, files in os.walk(LIBRARY):
        dirs[:] = [d for d in dirs if not d.startswith(".")]
        for f in files:
            if not f.lower().endswith(".pdf"):
                continue
            full = os.path.join(root, f)
            rel = os.path.relpath(full, LIBRARY).replace(os.sep, "/")
            try:
                size = os.path.getsize(full)
            except OSError:
                size = 0
            stem = f[:-4]
            m = NAME_RE.match(stem)
            if m:
                year, author, title = m.group(1), m.group(2), split_camel(m.group(3))
            else:
                year, author, title = "", "", split_camel(stem)
            ov = side.get(rel, {})
            items.append({
                "rel": rel,
                "file": f,
                "size": size,
                "year": str(ov.get("year") or year),
                "author": ov.get("author") or author,
                "title": ov.get("title") or title,
                "tags": norm_tags(ov.get("tags")),
                "folder": os.path.dirname(rel),
            })
    items.sort(key=lambda d: (d["year"], d["title"].lower()), reverse=True)
    return items


def list_dirs() -> list:
    """All folders under the library (relative paths, excluding hidden ones)."""
    dirs = set()
    for root, ds, _ in os.walk(LIBRARY):
        ds[:] = [d for d in ds if not d.startswith(".")]
        for d in ds:
            rel = os.path.relpath(os.path.join(root, d), LIBRARY).replace(os.sep, "/")
            dirs.add(rel)
    return sorted(dirs)


def clean_folder_rel(raw: str) -> str:
    """Sanitize a relative folder path: drop traversal/hidden/empty segments."""
    parts = []
    for p in str(raw).replace("\\", "/").split("/"):
        p = re.sub(r'[<>:"|?*\x00-\x1f]', "", p).strip()
        if p and p not in (".", "..") and not p.startswith("."):
            parts.append(p)
    return "/".join(parts)


def safe_path(rel: str):
    rel = urllib.parse.unquote(rel)
    full = os.path.realpath(os.path.join(LIBRARY, rel))
    if full != LIBRARY and not full.startswith(LIBRARY + os.sep):
        return None
    if not os.path.isfile(full) or not full.lower().endswith(".pdf"):
        return None
    return full


def sanitize_name(name: str) -> str:
    name = os.path.basename(name.strip()).replace("\\", "").replace("/", "")
    name = re.sub(r'[<>:"|?*\x00-\x1f]', "", name)
    if not name:
        return ""
    if not name.lower().endswith(".pdf"):
        name += ".pdf"
    return name


def thumb_file(rel: str) -> str:
    h = hashlib.sha1(rel.encode("utf-8")).hexdigest()
    return os.path.join(THUMB_DIR, h + ".png")


def make_thumb(full: str, out: str) -> bool:
    if fitz is None:
        return False
    try:
        os.makedirs(THUMB_DIR, exist_ok=True)
        doc = fitz.open(full)
        page = doc.load_page(0)
        zoom = min(1.2, 300.0 / max(1.0, page.rect.width))
        pix = page.get_pixmap(matrix=fitz.Matrix(zoom, zoom), alpha=False)
        pix.save(out)
        doc.close()
        return True
    except Exception:
        return False


# --------------------------------------------------------------------------- #
# HTML
# --------------------------------------------------------------------------- #
PAGE = """<!doctype html>
<html lang="en"><head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Library</title>
<style>
:root{{
  --bg:#f7f6f2;--surface:#ffffff;--surface-soft:#fbfaf7;--ink:#1f1f1a;
  --muted:#66655f;--line:#dfddd5;--line-strong:#cdcabf;
  --accent:#1f6feb;--accent-tint:#eaf1fd;
}}
*{{box-sizing:border-box;}}
body{{margin:0;background:var(--bg);color:var(--ink);font-family:"Segoe UI","Inter","Noto Sans","Helvetica Neue",Arial,sans-serif;line-height:1.5;-webkit-font-smoothing:antialiased;}}
.wrap{{max-width:1040px;margin:0 auto;padding:48px 24px 96px;}}
header h1{{font-weight:600;font-size:32px;margin:0 0 4px;letter-spacing:-.01em;}}
header p{{margin:0;color:var(--muted);font-size:15px;}}
.controls{{display:flex;flex-wrap:wrap;gap:10px;align-items:center;margin:26px 0 6px;}}
.search{{flex:1 1 240px;padding:12px 15px;font-size:15px;background:var(--surface-soft);border:1px solid var(--line-strong);border-radius:10px;color:var(--ink);outline:none;}}
.search:focus{{border-color:var(--accent);}}
select,.toggle button{{padding:11px 13px;font-size:14px;background:var(--surface-soft);border:1px solid var(--line-strong);border-radius:10px;color:var(--ink);cursor:pointer;}}
.toggle{{display:inline-flex;border:1px solid var(--line-strong);border-radius:10px;overflow:hidden;}}
.toggle button{{border:none;border-radius:0;background:var(--surface-soft);}}
.toggle button.active{{background:var(--accent);color:#fff;}}
.pathbar{{display:flex;gap:8px;align-items:center;flex-wrap:wrap;margin:14px 0 4px;}}
.pathbar input{{flex:0 1 260px;padding:8px 11px;font-size:13.5px;background:var(--surface-soft);border:1px solid var(--line-strong);border-radius:9px;color:var(--ink);outline:none;}}
.pathbar input:focus{{border-color:var(--accent);}}
.pathbar .btn{{padding:8px 11px;font-size:13.5px;}}
.crumbs{{flex:1 1 auto;font-size:14px;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}}
.crumbs a{{color:var(--accent);cursor:pointer;text-decoration:none;}}
.crumbs a:hover{{text-decoration:underline;}}
.crumbs .sep{{color:var(--muted);margin:0 5px;}}
.navfolder{{cursor:pointer;}}
.view-list .navfolder{{display:flex;align-items:center;gap:10px;padding:14px 16px;margin-bottom:10px;font-weight:600;}}
.view-grid .navfolder{{display:flex;flex-direction:column;align-items:center;justify-content:center;gap:10px;aspect-ratio:3/4;
  font-weight:600;text-align:center;padding:14px;word-break:break-word;
  background:var(--surface);border:1px solid var(--line);border-radius:12px;transition:border-color .12s,transform .12s;}}
.view-grid .navfolder:hover{{border-color:var(--accent);transform:translateY(-1px);}}
.view-grid .navfolder span:first-child{{font-size:38px;line-height:1;}}
.navfolder.drop-hover{{outline:2px dashed var(--accent);outline-offset:-2px;background:var(--accent-tint);}}
.rowbtns .mv:hover{{border-color:var(--accent);color:var(--accent);}}
.count{{color:var(--muted);font-size:13px;margin:2px 2px 18px;}}
#list.view-grid{{display:grid;grid-template-columns:repeat(auto-fill,minmax(190px,1fr));gap:14px;}}
.item{{position:relative;background:var(--surface);border:1px solid var(--line);border-radius:12px;transition:border-color .12s,transform .12s;}}
.item:hover{{border-color:var(--accent);transform:translateY(-1px);}}
/* list view */
.view-list .item{{display:flex;align-items:baseline;gap:14px;padding:15px 16px;margin-bottom:10px;}}
.view-list .thumb{{display:none;}}
.view-list .year{{flex:0 0 auto;min-width:44px;color:var(--accent);font-weight:600;font-size:14px;font-variant-numeric:tabular-nums;}}
.view-list .body{{flex:1 1 auto;min-width:0;}}
.view-list .size{{flex:0 0 auto;color:var(--muted);font-size:12px;font-variant-numeric:tabular-nums;}}
/* grid view */
.view-grid .open{{display:block;padding:0;}}
.view-grid .thumb{{display:block;width:100%;aspect-ratio:3/4;object-fit:cover;background:#eceae1;border-radius:12px 12px 0 0;border-bottom:1px solid var(--line);}}
.view-grid .body{{padding:10px 12px 12px;}}
.view-grid .year{{color:var(--accent);font-weight:600;font-size:12.5px;padding:6px 12px 0;}}
.view-grid .size,.view-grid .meta{{display:none;}}
.open{{text-decoration:none;color:inherit;display:contents;}}
.title{{font-size:15.5px;font-weight:500;word-break:break-word;}}
.meta{{color:var(--muted);font-size:13px;margin-top:2px;}}
.tags{{margin-top:6px;display:flex;flex-wrap:wrap;gap:5px;}}
.tag{{font-size:11.5px;color:var(--accent);background:var(--accent-tint);border-radius:20px;padding:1px 9px;}}
.rowbtns{{position:absolute;top:8px;right:8px;display:flex;gap:6px;opacity:0;transition:opacity .12s;z-index:2;}}
.item:hover .rowbtns{{opacity:1;}}
.rowbtns button{{border:1px solid var(--line);background:var(--surface);color:var(--muted);
  border-radius:8px;font-size:12px;padding:3px 8px;cursor:pointer;}}
.rowbtns .edit:hover{{border-color:var(--accent);color:var(--accent);}}
.rowbtns .del:hover{{border-color:#C0392B;color:#C0392B;}}
.btn.danger{{background:#C0392B;border-color:#C0392B;color:#fff;}}
.empty{{color:var(--muted);padding:40px 0;text-align:center;}}
footer{{color:var(--muted);font-size:12px;margin-top:30px;text-align:center;}}
/* modal */
.backdrop{{position:fixed;inset:0;background:rgba(24,22,20,.34);display:none;align-items:center;justify-content:center;padding:20px;z-index:9;}}
.backdrop.show{{display:flex;}}
.modal{{background:var(--surface);border:1px solid var(--line);border-radius:12px;width:100%;max-width:460px;padding:22px 22px 18px;box-shadow:0 1px 2px rgba(0,0,0,.03);}}
.modal h3{{margin:0 0 14px;font-weight:600;font-size:18px;letter-spacing:-.01em;}}
.modal label{{display:block;font-size:12.5px;color:var(--muted);margin:11px 0 4px;}}
.modal input{{width:100%;padding:9px 11px;font-size:14px;border:1px solid var(--line-strong);border-radius:9px;background:var(--surface-soft);color:var(--ink);outline:none;}}
.modal input:focus{{border-color:var(--accent);}}
.row2{{display:flex;gap:10px;}}.row2>div{{flex:1;}}
.modal select{{width:100%;padding:9px 11px;font-size:14px;border:1px solid var(--line-strong);border-radius:9px;background:var(--surface-soft);color:var(--ink);}}
.actions{{display:flex;justify-content:flex-end;gap:10px;margin-top:18px;}}
/* tree view */
.tree{{background:var(--surface);border:1px solid var(--line);border-radius:12px;padding:8px;}}
.trow{{display:flex;align-items:center;gap:8px;padding:6px 8px;border-radius:8px;font-size:14.5px;}}
.trow:hover{{background:#F3F1E9;}}
.tcaret{{width:14px;flex:0 0 auto;color:var(--muted);user-select:none;}}
.tfolder{{cursor:pointer;font-weight:600;}}
.tname{{flex:1 1 auto;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}}
.tname a{{color:inherit;text-decoration:none;}}
.tsize{{flex:0 0 auto;color:var(--muted);font-size:12px;font-variant-numeric:tabular-nums;}}
.tx{{flex:0 0 auto;cursor:pointer;color:var(--muted);border:1px solid var(--line);background:var(--surface);border-radius:6px;font-size:12px;line-height:1;padding:3px 7px;}}
.tx:hover{{border-color:#C0392B;color:#C0392B;}}
.tfile{{cursor:grab;}}
.tchildren.collapsed{{display:none;}}
.drop-hover{{outline:2px dashed var(--accent);outline-offset:-2px;background:var(--accent-tint);}}
.btn{{padding:9px 15px;font-size:14px;border-radius:9px;border:1px solid var(--line);background:var(--surface);color:var(--ink);cursor:pointer;}}
.btn.primary{{background:var(--accent);border-color:var(--accent);color:#fff;}}
/* multi-select */
.selwrap{{position:absolute;top:8px;left:8px;z-index:3;opacity:0;transition:opacity .12s;}}
.item:hover .selwrap,.item.selected .selwrap{{opacity:1;}}
.selbox{{width:17px;height:17px;cursor:pointer;accent-color:var(--accent);}}
.item.selected{{border-color:var(--accent);box-shadow:inset 0 0 0 1px var(--accent);}}
.trow.selected{{background:var(--accent-tint);}}
.trow .selbox{{flex:0 0 auto;}}
.selbar{{position:fixed;left:50%;bottom:18px;transform:translateX(-50%);display:none;align-items:center;gap:12px;
  background:var(--ink);color:#fff;padding:10px 14px;border-radius:12px;box-shadow:0 6px 26px rgba(0,0,0,.28);z-index:20;}}
.selbar.show{{display:flex;}}
.selbar select{{padding:7px 9px;border-radius:8px;border:1px solid #4a4a4a;background:#2a2826;color:#fff;font-size:13px;max-width:260px;}}
.selbar .sb{{padding:7px 12px;border-radius:8px;border:none;cursor:pointer;background:var(--accent);color:#fff;font-size:13px;}}
.selbar .sb.ghost{{background:transparent;border:1px solid #4a4a4a;color:#ddd;}}
.selcount{{font-size:13.5px;font-weight:600;}}
</style></head>
<body>
<div class="wrap">
  <header><h1>Library</h1><p>{count} documents · {total}</p></header>
  <div class="controls">
    <input id="q" class="search" type="search" placeholder="Search title, author, year, #tag…" autofocus>
    <select id="sort">
      <option value="year-desc">Year (new → old)</option>
      <option value="year-asc">Year (old → new)</option>
      <option value="title-asc">Title (A → Z)</option>
      <option value="title-desc">Title (Z → A)</option>
    </select>
    <div class="toggle">
      <button id="btnList" class="active" type="button">List</button>
      <button id="btnGrid" type="button">Grid</button>
      <button id="btnTree" type="button">Tree</button>
    </div>
    <button id="btnNewFolder" class="btn" type="button" style="display:none">+ New folder</button>
  </div>
  <div id="pathbar" class="pathbar">
    <button id="upBtn" class="btn" type="button" title="Up one folder">↑</button>
    <span id="crumbs" class="crumbs"></span>
    <input id="pathInput" type="text" placeholder="folder path, e.g. pdfs/Theses">
    <button id="goBtn" class="btn" type="button">Go</button>
  </div>
  <div id="count" class="count"></div>
  <div id="list" class="view-list">{rows}</div>
  <div id="tree" class="tree" style="display:none"></div>
  <div id="empty" class="empty" style="display:none">No documents match.</div>
  <footer>library-portal · served live from <code>library/</code></footer>
</div>

<div id="selbar" class="selbar">
  <span class="selcount" id="selcount">0 selected</span>
  <select id="selfolder"></select>
  <button class="sb" type="button" onclick="bulkMove()">Move</button>
  <button class="sb ghost" type="button" onclick="selectAllVisible()">Select all</button>
  <button class="sb ghost" type="button" onclick="clearSel()">Clear</button>
</div>

<div id="backdrop" class="backdrop">
  <div class="modal">
    <h3>Edit document</h3>
    <input type="hidden" id="m_rel">
    <label>Filename</label><input id="m_file" type="text">
    <div class="row2">
      <div><label>Year</label><input id="m_year" type="text"></div>
      <div><label>Author letters</label><input id="m_author" type="text"></div>
    </div>
    <label>Title</label><input id="m_title" type="text">
    <label>Tags (space or comma separated, e.g. read to-read favourite)</label>
    <input id="m_tags" type="text">
    <div class="actions">
      <button class="btn" type="button" onclick="closeModal()">Cancel</button>
      <button class="btn primary" type="button" onclick="saveModal()">Save</button>
    </div>
  </div>
</div>

<div id="delback" class="backdrop">
  <div class="modal">
    <h3>Delete document</h3>
    <p id="delmsg" style="color:var(--muted);font-size:14px;margin:0;word-break:break-word;"></p>
    <input type="hidden" id="d_rel">
    <div class="actions">
      <button class="btn" type="button" onclick="closeDelete()">Cancel</button>
      <button class="btn danger" type="button" onclick="confirmDelete()">Yes, delete</button>
    </div>
  </div>
</div>

<div id="nfback" class="backdrop">
  <div class="modal">
    <h3>New folder</h3>
    <label>Inside</label><select id="nf_parent"></select>
    <label>Folder name</label><input id="nf_name" type="text" placeholder="e.g. Theses">
    <div class="actions">
      <button class="btn" type="button" onclick="closeNF()">Cancel</button>
      <button class="btn primary" type="button" onclick="createFolder()">Create</button>
    </div>
  </div>
</div>

<div id="mvback" class="backdrop">
  <div class="modal">
    <h3>Move to folder</h3>
    <p id="mvmsg" style="color:var(--muted);font-size:13px;margin:0 0 4px;word-break:break-word;"></p>
    <input type="hidden" id="mv_rel">
    <label>Destination folder</label><select id="mv_folder"></select>
    <div class="actions">
      <button class="btn" type="button" onclick="closeMv()">Cancel</button>
      <button class="btn primary" type="button" onclick="confirmMove()">Move</button>
    </div>
  </div>
</div>

<div id="pvback" class="backdrop">
  <div class="modal" style="max-width:400px;text-align:center;">
    <h3 id="pv_name" style="font-size:15px;word-break:break-word;"></h3>
    <img id="pv_img" alt="" style="max-width:100%;max-height:60vh;border:1px solid var(--line);border-radius:8px;background:#eceae1;">
    <div class="actions" style="justify-content:center;">
      <a id="pv_open" class="btn primary" target="_blank" rel="noopener">Open PDF</a>
      <button class="btn" type="button" onclick="closePv()">Close</button>
    </div>
  </div>
</div>

<script>
const FOLDERS={folders_json};
const listEl=document.getElementById('list');
const q=document.getElementById('q'), sortEl=document.getElementById('sort');
const countEl=document.getElementById('count'), emptyEl=document.getElementById('empty');
let items=Array.from(document.querySelectorAll('.item'));
const FOLDERSET=new Set(FOLDERS);
let cwd=localStorage.getItem('lp_cwd')||'';
if(cwd && !FOLDERSET.has(cwd)) cwd='';
function parentOf(f){{ const i=f.lastIndexOf('/'); return i<0?'':f.slice(0,i); }}
function childFoldersOf(dir){{ return FOLDERS.filter(f=>parentOf(f)===dir).sort(); }}
function inTree(){{ return document.getElementById('btnTree').classList.contains('active'); }}
function bindDropFolder(el,folderRel){{
  el.addEventListener('dragover',e=>{{e.preventDefault();el.classList.add('drop-hover');}});
  el.addEventListener('dragleave',()=>el.classList.remove('drop-hover'));
  el.addEventListener('drop',async e=>{{e.preventDefault();el.classList.remove('drop-hover');
    const rel=e.dataTransfer.getData('text/plain'); if(!rel)return;
    const r=await moveFile(rel,folderRel);
    if(r.ok)location.reload(); else alert('Move failed: '+(await r.text()));
  }});
}}
function navigate(dir){{
  dir=(dir||'').replace(/^\/+|\/+$/g,'');
  if(dir!=='' && !FOLDERSET.has(dir)){{ alert('No such folder: '+dir); return; }}
  document.getElementById('q').value='';
  cwd=dir; localStorage.setItem('lp_cwd',cwd); apply();
}}
function renderNav(){{
  const crumbs=document.getElementById('crumbs');
  let acc='', h='<a data-go="">library</a>';
  for(const p of (cwd?cwd.split('/'):[])){{ acc=acc?acc+'/'+p:p; h+='<span class="sep">/</span><a data-go="'+escapeHtml(acc)+'">'+escapeHtml(p)+'</a>'; }}
  crumbs.innerHTML=h;
  crumbs.querySelectorAll('a').forEach(a=>a.onclick=()=>navigate(a.dataset.go));
  document.getElementById('pathInput').value=cwd;
  document.getElementById('upBtn').disabled=(cwd==='');
  listEl.querySelectorAll('.navfolder').forEach(n=>n.remove());
  const subs=childFoldersOf(cwd);
  const frag=document.createDocumentFragment();
  for(const f of subs){{
    const name=f.indexOf('/')<0?f:f.slice(f.lastIndexOf('/')+1);
    const el=document.createElement('div'); el.className='navfolder'; el.dataset.folder=f;
    el.innerHTML='<span>📁</span><span class="tname">'+escapeHtml(name)+'</span>';
    el.onclick=()=>navigate(f);
    bindDropFolder(el,f);
    frag.appendChild(el);
  }}
  listEl.insertBefore(frag, listEl.firstChild);
  let shown=0;
  for(const el of items){{
    const inDir=el.dataset.folder===cwd;
    el.style.display=inDir?'':'none';
    if(inDir){{ shown++; el.draggable=true;
      if(!el._dragbound){{ el._dragbound=1; el.addEventListener('dragstart',e=>{{e.dataTransfer.setData('text/plain',el.dataset.rel);e.dataTransfer.effectAllowed='move';}}); }}
    }}
  }}
  countEl.textContent=subs.length+' folder'+(subs.length===1?'':'s')+' · '+shown+' file'+(shown===1?'':'s');
  emptyEl.style.display=(subs.length+shown)?'none':'';
}}
function apply(){{
  if(inTree()) return;
  const term=q.value.trim().toLowerCase();
  if(!term){{ renderNav(); return; }}
  listEl.querySelectorAll('.navfolder').forEach(n=>n.remove());
  document.getElementById('crumbs').innerHTML='<span style="color:var(--muted)">search across all folders</span>';
  document.getElementById('pathInput').value='';
  let shown=0;
  for(const el of items){{ const hit=el.dataset.search.includes(term); el.style.display=hit?'':'none'; if(hit)shown++; }}
  countEl.textContent=shown+(shown===1?' result':' results');
  emptyEl.style.display=shown?'none':'';
}}
function sortItems(){{
  const v=sortEl.value;
  items.sort((a,b)=>{{
    if(v.startsWith('year')){{
      const r=(a.dataset.year||'').localeCompare(b.dataset.year||'');
      return v==='year-asc'?r:-r;
    }} else {{
      const r=(a.dataset.title||'').localeCompare(b.dataset.title||'');
      return v==='title-asc'?r:-r;
    }}
  }});
  for(const el of items) listEl.appendChild(el);
}}
function escapeHtml(s){{return (s||'').replace(/[&<>"]/g,c=>({{'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}}[c]));}}
function setView(mode){{
  const list=document.getElementById('list'),tree=document.getElementById('tree'),nf=document.getElementById('btnNewFolder'),pb=document.getElementById('pathbar');
  document.getElementById('btnList').classList.toggle('active',mode==='list');
  document.getElementById('btnGrid').classList.toggle('active',mode==='grid');
  document.getElementById('btnTree').classList.toggle('active',mode==='tree');
  if(mode==='tree'){{ list.style.display='none'; nf.style.display=''; pb.style.display='none'; tree.style.display=''; buildTree(); }}
  else {{ tree.style.display='none'; nf.style.display='none'; pb.style.display='flex'; list.style.display=''; list.className=(mode==='grid')?'view-grid':'view-list'; apply(); }}
  localStorage.setItem('lp_view',mode);
}}
function moveFile(rel,folder){{
  return fetch('/api/move',{{method:'POST',headers:{{'Content-Type':'application/json'}},body:JSON.stringify({{rel,folder}})}});
}}
function mkFolderRow(label,folderRel,depth){{
  const row=document.createElement('div'); row.className='trow tfolder';
  row.style.paddingLeft=(depth*18+8)+'px'; row.dataset.folder=folderRel;
  row.innerHTML='<span class="tcaret">▾</span><span>📁</span><span class="tname">'+escapeHtml(label)+'</span>';
  row.addEventListener('dragover',e=>{{e.preventDefault();row.classList.add('drop-hover');}});
  row.addEventListener('dragleave',()=>row.classList.remove('drop-hover'));
  row.addEventListener('drop',async e=>{{e.preventDefault();row.classList.remove('drop-hover');
    const rel=e.dataTransfer.getData('text/plain'); if(!rel)return;
    const r=await moveFile(rel,folderRel);
    if(r.ok)location.reload(); else alert('Move failed: '+(await r.text()));
  }});
  return row;
}}
function wireToggle(row,kids){{
  row.addEventListener('click',e=>{{
    if(e.target.closest('a')||e.target.closest('.tx'))return;
    const col=kids.classList.toggle('collapsed');
    row.querySelector('.tcaret').textContent=col?'▸':'▾';
  }});
}}
function mkFileRow(fl,depth){{
  const row=document.createElement('div'); row.className='trow tfile'; row.dataset.rel=fl.rel;
  row.style.paddingLeft=(depth*18+24)+'px'; row.draggable=true;
  row.addEventListener('dragstart',e=>{{e.dataTransfer.setData('text/plain',fl.rel);e.dataTransfer.effectAllowed='move';}});
  row.innerHTML='<input type="checkbox" class="selbox"><span>📄</span><span class="tname">'+escapeHtml(fl.name)+'</span><span class="tsize">'+escapeHtml(fl.size)+'</span>'
    +'<button class="tx bmv" type="button">Move</button><button class="tx bdel" type="button">Delete</button>';
  const cb=row.querySelector('.selbox');
  if(selected.has(fl.rel)){{ row.classList.add('selected'); cb.checked=true; }}
  cb.addEventListener('click',e=>e.stopPropagation());
  cb.addEventListener('change',()=>toggleSel(fl.rel,cb.checked,row));
  row.querySelector('.bmv').onclick=e=>{{e.preventDefault();e.stopPropagation();openMv(fl.rel,fl.name);}};
  row.querySelector('.bdel').onclick=e=>{{e.preventDefault();e.stopPropagation();openDeleteRel(fl.rel,fl.name);}};
  row.addEventListener('click',e=>{{ if(e.target.closest('button')||e.target.classList.contains('selbox'))return; openPreview(fl.rel,fl.name); }});
  return row;
}}
function renderNode(node,base,depth,into){{
  for(const fn of Object.keys(node.folders).sort()){{
    const rel=base?base+'/'+fn:fn;
    const fr=mkFolderRow(fn,rel,depth); into.appendChild(fr);
    const kids=document.createElement('div'); kids.className='tchildren';
    renderNode(node.folders[fn],rel,depth+1,kids); into.appendChild(kids);
    wireToggle(fr,kids);
  }}
  for(const fl of node.files.slice().sort((a,b)=>a.name.localeCompare(b.name))) into.appendChild(mkFileRow(fl,depth));
}}
function buildTree(){{
  const tree=document.getElementById('tree');
  const files=Array.from(document.querySelectorAll('.item')).map(el=>({{rel:el.dataset.rel,name:el.dataset.file,size:el.dataset.size||''}}));
  const root={{folders:{{}},files:[]}};
  function ensure(parts){{let n=root;for(const p of parts){{if(!p)continue;n.folders[p]=n.folders[p]||{{folders:{{}},files:[]}};n=n.folders[p];}}return n;}}
  for(const f of FOLDERS) ensure(f.split('/'));
  for(const fl of files){{const parts=fl.rel.split('/');parts.pop();ensure(parts).files.push(fl);}}
  tree.innerHTML='';
  const top=mkFolderRow('library/','',0); tree.appendChild(top);
  const kids=document.createElement('div'); kids.className='tchildren';
  renderNode(root,'',1,kids); tree.appendChild(kids); wireToggle(top,kids);
}}
const nfBtn=document.getElementById('btnNewFolder');
nfBtn.onclick=()=>{{
  const sel=document.getElementById('nf_parent');
  sel.innerHTML='<option value="">library/ (root)</option>'+FOLDERS.map(f=>'<option value="'+escapeHtml(f)+'">'+escapeHtml(f)+'</option>').join('');
  document.getElementById('nf_name').value='';
  document.getElementById('nfback').classList.add('show');
}};
function closeNF(){{document.getElementById('nfback').classList.remove('show');}}
async function createFolder(){{
  const parent=document.getElementById('nf_parent').value;
  const name=document.getElementById('nf_name').value.trim();
  if(!name){{alert('Enter a folder name');return;}}
  const r=await fetch('/api/mkdir',{{method:'POST',headers:{{'Content-Type':'application/json'}},body:JSON.stringify({{parent,name}})}});
  if(r.ok)location.reload(); else alert('Create failed: '+(await r.text()));
}}
document.getElementById('nfback').addEventListener('click',e=>{{if(e.target.id==='nfback')closeNF();}});
q.addEventListener('input',apply);
sortEl.addEventListener('change',()=>{{sortItems();apply();}});
document.getElementById('btnList').onclick=()=>setView('list');
document.getElementById('btnGrid').onclick=()=>setView('grid');
document.getElementById('btnTree').onclick=()=>setView('tree');

// path bar
document.getElementById('goBtn').onclick=()=>navigate(document.getElementById('pathInput').value);
document.getElementById('pathInput').addEventListener('keydown',e=>{{if(e.key==='Enter')navigate(e.target.value);}});
document.getElementById('upBtn').onclick=()=>navigate(parentOf(cwd));

// move modal
function openMv(rel,file){{
  document.getElementById('mv_rel').value=rel;
  document.getElementById('mvmsg').textContent='Move "'+file+'" to:';
  document.getElementById('mv_folder').innerHTML='<option value="">library/ (root)</option>'+FOLDERS.map(f=>'<option value="'+escapeHtml(f)+'">'+escapeHtml(f)+'</option>').join('');
  document.getElementById('mvback').classList.add('show');
}}
function closeMv(){{document.getElementById('mvback').classList.remove('show');}}
async function confirmMove(){{
  const r=await moveFile(document.getElementById('mv_rel').value, document.getElementById('mv_folder').value);
  if(r.ok)location.reload(); else alert('Move failed: '+(await r.text()));
}}
document.querySelectorAll('.mv').forEach(b=>b.onclick=e=>{{e.preventDefault();e.stopPropagation();const it=b.closest('.item');openMv(it.dataset.rel,it.dataset.file);}});
document.getElementById('mvback').addEventListener('click',e=>{{if(e.target.id==='mvback')closeMv();}});

// preview modal (tree thumbnail on click)
function openPreview(rel,file){{
  const enc='/'+rel.split('/').map(encodeURIComponent).join('/');
  document.getElementById('pv_name').textContent=file;
  document.getElementById('pv_img').src='/thumb'+enc;
  document.getElementById('pv_open').href='/view'+enc;
  document.getElementById('pvback').classList.add('show');
}}
function closePv(){{document.getElementById('pvback').classList.remove('show');}}
document.getElementById('pvback').addEventListener('click',e=>{{if(e.target.id==='pvback')closePv();}});

function openModal(el){{
  document.getElementById('m_rel').value=el.dataset.rel;
  document.getElementById('m_file').value=el.dataset.file;
  document.getElementById('m_year').value=el.dataset.year;
  document.getElementById('m_author').value=el.dataset.author;
  document.getElementById('m_title').value=el.dataset.titleRaw;
  document.getElementById('m_tags').value=el.dataset.tags;
  document.getElementById('backdrop').classList.add('show');
}}
function closeModal(){{document.getElementById('backdrop').classList.remove('show');}}
async function saveModal(){{
  const payload={{
    rel:document.getElementById('m_rel').value,
    newname:document.getElementById('m_file').value,
    year:document.getElementById('m_year').value,
    author:document.getElementById('m_author').value,
    title:document.getElementById('m_title').value,
    tags:document.getElementById('m_tags').value,
  }};
  const r=await fetch('/api/save',{{method:'POST',headers:{{'Content-Type':'application/json'}},body:JSON.stringify(payload)}});
  if(r.ok){{ location.reload(); }}
  else {{ alert('Save failed: '+(await r.text())); }}
}}
document.querySelectorAll('.edit').forEach(b=>b.onclick=e=>{{e.preventDefault();e.stopPropagation();openModal(b.closest('.item'));}});
document.getElementById('backdrop').addEventListener('click',e=>{{if(e.target.id==='backdrop')closeModal();}});

function openDeleteRel(rel,file){{
  document.getElementById('d_rel').value=rel;
  document.getElementById('delmsg').textContent='Permanently delete "'+file+'" from the library? This removes the file from disk and cannot be undone.';
  document.getElementById('delback').classList.add('show');
}}
function openDelete(el){{ openDeleteRel(el.dataset.rel, el.dataset.file); }}
function closeDelete(){{document.getElementById('delback').classList.remove('show');}}
async function confirmDelete(){{
  const rel=document.getElementById('d_rel').value;
  const r=await fetch('/api/delete',{{method:'POST',headers:{{'Content-Type':'application/json'}},body:JSON.stringify({{rel}})}});
  if(r.ok){{ location.reload(); }} else {{ alert('Delete failed: '+(await r.text())); }}
}}
document.querySelectorAll('.del').forEach(b=>b.onclick=e=>{{e.preventDefault();e.stopPropagation();openDelete(b.closest('.item'));}});
document.getElementById('delback').addEventListener('click',e=>{{if(e.target.id==='delback')closeDelete();}});

// ---- multi-select (list / grid / tree) ----
const selected=new Set();
function refreshSelBar(){{
  document.getElementById('selcount').textContent=selected.size+' selected';
  document.getElementById('selbar').classList.toggle('show',selected.size>0);
}}
function toggleSel(rel,on,el){{
  if(on)selected.add(rel); else selected.delete(rel);
  if(el)el.classList.toggle('selected',on);
  refreshSelBar();
}}
function clearSel(){{
  selected.clear();
  document.querySelectorAll('.selected').forEach(e=>e.classList.remove('selected'));
  document.querySelectorAll('.selbox').forEach(c=>c.checked=false);
  refreshSelBar();
}}
function selectAllVisible(){{
  if(inTree()){{
    document.querySelectorAll('#tree .tfile').forEach(r=>{{const rel=r.dataset.rel; if(!rel)return; selected.add(rel); r.classList.add('selected'); const cb=r.querySelector('.selbox'); if(cb)cb.checked=true;}});
  }} else {{
    document.querySelectorAll('#list .item').forEach(el=>{{ if(el.style.display==='none')return; selected.add(el.dataset.rel); el.classList.add('selected'); const cb=el.querySelector('.selbox'); if(cb)cb.checked=true; }});
  }}
  refreshSelBar();
}}
async function bulkMove(){{
  if(selected.size===0)return;
  const folder=document.getElementById('selfolder').value;
  const r=await fetch('/api/move-batch',{{method:'POST',headers:{{'Content-Type':'application/json'}},body:JSON.stringify({{rels:[...selected],folder}})}});
  if(r.ok)location.reload(); else alert('Move failed: '+(await r.text()));
}}
document.getElementById('selfolder').innerHTML='<option value="">Move to: library/ (root)</option>'+FOLDERS.map(f=>'<option value="'+escapeHtml(f)+'">Move to: '+escapeHtml(f)+'</option>').join('');
document.querySelectorAll('#list .selbox').forEach(cb=>{{
  cb.addEventListener('click',e=>e.stopPropagation());
  cb.addEventListener('change',()=>{{const it=cb.closest('.item'); toggleSel(it.dataset.rel,cb.checked,it);}});
}});

// init
sortItems();
setView(localStorage.getItem('lp_view')||'list');
apply();
</script>
</body></html>"""


def render_index() -> str:
    items = list_pdfs()
    total = human_size(sum(i["size"] for i in items))
    rows = []
    for it in items:
        tags = it["tags"]
        search = " ".join([it["title"], it["author"], it["year"], it["file"]]
                          + ["#" + t for t in tags]).lower()
        meta_bits = []
        if it["author"]:
            meta_bits.append(it["author"])
        if it["folder"] and it["folder"] != "pdfs":
            meta_bits.append(it["folder"])
        meta = html.escape(" · ".join(meta_bits))
        href = "/view/" + urllib.parse.quote(it["rel"])
        thumb = "/thumb/" + urllib.parse.quote(it["rel"])
        tag_html = "".join(f'<span class="tag">#{html.escape(t)}</span>' for t in tags)
        a = lambda s: html.escape(s, quote=True)
        rows.append(
            f'<div class="item" data-rel="{a(it["rel"])}" data-file="{a(it["file"])}" '
            f'data-folder="{a(it["folder"])}" data-size="{a(human_size(it["size"]))}" '
            f'data-title="{a(it["title"].lower())}" data-title-raw="{a(it["title"])}" '
            f'data-year="{a(it["year"])}" data-author="{a(it["author"])}" '
            f'data-tags="{a(" ".join(tags))}" data-search="{a(search)}">'
            f'<div class="selwrap"><input type="checkbox" class="selbox" title="Select"></div>'
            f'<div class="rowbtns"><button class="edit" type="button">Edit</button>'
            f'<button class="mv" type="button">Move</button>'
            f'<button class="del" type="button">Delete</button></div>'
            f'<a class="open" href="{href}" target="_blank" rel="noopener">'
            f'<img class="thumb" loading="lazy" src="{thumb}" alt="">'
            f'<div class="year">{html.escape(it["year"]) or "&nbsp;"}</div>'
            f'<div class="body"><div class="title">{html.escape(it["title"])}</div>'
            f'<div class="meta">{meta}</div>'
            f'<div class="tags">{tag_html}</div></div>'
            f'<div class="size">{human_size(it["size"])}</div></a></div>'
        )
    body = "\n".join(rows) if rows else '<div class="empty">No PDFs found in library/.</div>'
    return PAGE.format(count=len(items), total=total, rows=body,
                       folders_json=json.dumps(list_dirs()))


# --------------------------------------------------------------------------- #
# server
# --------------------------------------------------------------------------- #
class Handler(BaseHTTPRequestHandler):
    server_version = "library-portal"

    def _send(self, code, body=b"", ctype="text/html; charset=utf-8", extra=None):
        if isinstance(body, str):
            body = body.encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        for k, v in (extra or {}).items():
            self.send_header(k, v)
        self.end_headers()
        if body:
            self.wfile.write(body)

    def _json(self, code, obj):
        self._send(code, json.dumps(obj), "application/json")

    def do_GET(self):
        path = self.path.split("?", 1)[0]
        if path in ("/", "/index.html"):
            self._send(200, render_index())
        elif path == "/health":
            self._send(200, b"ok", "text/plain; charset=utf-8")
        elif path.startswith("/view/"):
            full = safe_path(path[len("/view/"):])
            if not full:
                return self._send(404, b"Not found", "text/plain")
            with open(full, "rb") as fh:
                data = fh.read()
            disp = 'inline; filename="%s"' % os.path.basename(full).replace('"', "")
            self._send(200, data, "application/pdf", {"Content-Disposition": disp})
        elif path.startswith("/thumb/"):
            rel = path[len("/thumb/"):]
            full = safe_path(rel)
            if not full:
                return self._send(404, b"", "image/png")
            out = thumb_file(urllib.parse.unquote(rel))
            try:
                fresh = os.path.exists(out) and os.path.getmtime(out) >= os.path.getmtime(full)
            except OSError:
                fresh = False
            if not fresh and not make_thumb(full, out):
                return self._send(404, b"", "image/png")
            with open(out, "rb") as fh:
                self._send(200, fh.read(), "image/png", {"Cache-Control": "max-age=86400"})
        else:
            self._send(404, b"Not found", "text/plain")

    def _read_json(self):
        length = int(self.headers.get("Content-Length", "0"))
        return json.loads(self.rfile.read(length) or b"{}")

    def do_POST(self):
        if self.path == "/api/save":
            return self.handle_save()
        if self.path == "/api/delete":
            return self.handle_delete()
        if self.path == "/api/mkdir":
            return self.handle_mkdir()
        if self.path == "/api/move":
            return self.handle_move()
        if self.path == "/api/move-batch":
            return self.handle_move_batch()
        return self._send(404, b"Not found", "text/plain")

    def handle_move_batch(self):
        try:
            payload = self._read_json()
        except Exception:
            return self._json(400, {"error": "bad request"})
        rels = payload.get("rels", [])
        if not isinstance(rels, list):
            return self._json(400, {"error": "rels must be a list"})
        folder = clean_folder_rel(payload.get("folder", ""))
        dest_dir = os.path.realpath(os.path.join(LIBRARY, folder)) if folder else LIBRARY
        if dest_dir != LIBRARY and not dest_dir.startswith(LIBRARY + os.sep):
            return self._json(400, {"error": "invalid folder"})
        if not os.path.isdir(dest_dir):
            return self._json(400, {"error": "destination folder does not exist"})

        moved, errors = 0, []
        with _LOCK:
            side = load_sidecar()
            changed = False
            for rel in rels:
                full = safe_path(rel)
                if not full:
                    errors.append(rel)
                    continue
                target = os.path.join(dest_dir, os.path.basename(full))
                if os.path.realpath(target) == os.path.realpath(full):
                    continue  # already in the destination
                if os.path.exists(target):
                    errors.append(rel)
                    continue
                try:
                    os.rename(full, target)
                except OSError:
                    errors.append(rel)
                    continue
                newrel = os.path.relpath(target, LIBRARY).replace(os.sep, "/")
                if rel in side:
                    side[newrel] = side.pop(rel)
                    changed = True
                old_thumb = thumb_file(rel)
                if os.path.exists(old_thumb):
                    try:
                        os.remove(old_thumb)
                    except OSError:
                        pass
                moved += 1
            if changed:
                save_sidecar(side)
        return self._json(200, {"ok": True, "moved": moved, "errors": errors})

    def handle_mkdir(self):
        try:
            payload = self._read_json()
        except Exception:
            return self._json(400, {"error": "bad request"})
        parent = clean_folder_rel(payload.get("parent", ""))
        name = clean_folder_rel(payload.get("name", ""))
        rel = "/".join(p for p in (parent, name) if p)
        if not name:
            return self._json(400, {"error": "invalid folder name"})
        full = os.path.realpath(os.path.join(LIBRARY, rel))
        if full != LIBRARY and not full.startswith(LIBRARY + os.sep):
            return self._json(400, {"error": "invalid path"})
        if os.path.exists(full):
            return self._json(409, {"error": "folder already exists"})
        try:
            os.makedirs(full)
        except OSError as exc:
            return self._json(500, {"error": f"mkdir failed: {exc}"})
        return self._json(200, {"ok": True, "path": rel})

    def handle_move(self):
        try:
            payload = self._read_json()
        except Exception:
            return self._json(400, {"error": "bad request"})
        rel = payload.get("rel", "")
        full = safe_path(rel)
        if not full:
            return self._json(404, {"error": "not found"})
        folder = clean_folder_rel(payload.get("folder", ""))
        dest_dir = os.path.realpath(os.path.join(LIBRARY, folder)) if folder else LIBRARY
        if dest_dir != LIBRARY and not dest_dir.startswith(LIBRARY + os.sep):
            return self._json(400, {"error": "invalid folder"})
        if not os.path.isdir(dest_dir):
            return self._json(400, {"error": "destination folder does not exist"})
        target = os.path.join(dest_dir, os.path.basename(full))
        if os.path.realpath(target) == os.path.realpath(full):
            return self._json(200, {"ok": True, "rel": rel})  # already there
        if os.path.exists(target):
            return self._json(409, {"error": "a file with that name exists in the target folder"})
        with _LOCK:
            try:
                os.rename(full, target)
            except OSError as exc:
                return self._json(500, {"error": f"move failed: {exc}"})
            newrel = os.path.relpath(target, LIBRARY).replace(os.sep, "/")
            side = load_sidecar()
            if rel in side:
                side[newrel] = side.pop(rel)
                save_sidecar(side)
            old_thumb = thumb_file(rel)
            if os.path.exists(old_thumb):
                try:
                    os.remove(old_thumb)
                except OSError:
                    pass
        return self._json(200, {"ok": True, "rel": newrel})

    def handle_delete(self):
        try:
            payload = self._read_json()
        except Exception:
            return self._json(400, {"error": "bad request"})
        rel = payload.get("rel", "")
        full = safe_path(rel)
        if not full:
            return self._json(404, {"error": "not found"})
        with _LOCK:
            try:
                os.remove(full)
            except OSError as exc:
                return self._json(500, {"error": f"delete failed: {exc}"})
            side = load_sidecar()
            if side.pop(rel, None) is not None:
                save_sidecar(side)
            thumb = thumb_file(rel)
            if os.path.exists(thumb):
                try:
                    os.remove(thumb)
                except OSError:
                    pass
        self._json(200, {"ok": True})

    def handle_save(self):
        try:
            payload = self._read_json()
        except Exception:
            return self._json(400, {"error": "bad request"})

        rel = payload.get("rel", "")
        full = safe_path(rel)
        if not full:
            return self._json(404, {"error": "not found"})

        with _LOCK:
            side = load_sidecar()
            newrel = rel
            newname = sanitize_name(payload.get("newname", "") or os.path.basename(rel))
            if newname and newname != os.path.basename(full):
                target = os.path.join(os.path.dirname(full), newname)
                if os.path.exists(target):
                    return self._json(409, {"error": "a file with that name already exists"})
                try:
                    os.rename(full, target)
                except OSError as exc:
                    return self._json(500, {"error": f"rename failed: {exc}"})
                newrel = os.path.relpath(target, LIBRARY).replace(os.sep, "/")
                if rel in side:
                    side[newrel] = side.pop(rel)
                old_thumb = thumb_file(rel)
                if os.path.exists(old_thumb):
                    try:
                        os.remove(old_thumb)
                    except OSError:
                        pass

            entry = side.get(newrel, {})
            for key in ("title", "author", "year"):
                val = str(payload.get(key, "")).strip()
                if val:
                    entry[key] = val
                else:
                    entry.pop(key, None)
            tags = norm_tags(payload.get("tags"))
            if tags:
                entry["tags"] = tags
            else:
                entry.pop("tags", None)
            if entry:
                side[newrel] = entry
            else:
                side.pop(newrel, None)
            save_sidecar(side)

        self._json(200, {"ok": True, "rel": newrel})

    def log_message(self, *args):
        pass


def main():
    engine = "with thumbnails" if fitz else "no thumbnail engine"
    print(f"[library-portal] serving {LIBRARY} on http://0.0.0.0:{PORT} ({engine})", flush=True)
    ThreadingHTTPServer(("0.0.0.0", PORT), Handler).serve_forever()


if __name__ == "__main__":
    main()
