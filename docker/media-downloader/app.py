#!/usr/bin/env python3
"""media-downloader — a tiny web UI/API wrapping yt-dlp + ffmpeg.

Paste a video URL, pick MP3 / WAV / MP4, and the file is saved to the host
folder bind-mounted at OUTPUT_DIR (the host Desktop, in normal use).

Safety model (this app downloads from arbitrary, untrusted web links):
  * The container runs non-root, with all Linux capabilities dropped and
    no-new-privileges (set by the run scripts).
  * Only http/https URLs are accepted, and URLs whose host resolves to a
    loopback/private/link-local/reserved address are rejected up front (a
    fast-fail SSRF check on the initial URL).
  * yt-dlp itself is routed through a small in-container egress-guard proxy
    (see start_egress_proxy/EGRESS_PROXY_PORT below) that re-resolves and
    re-validates *every* connection yt-dlp makes -- not just the initial URL
    -- at the moment it actually connects. This closes the gap the fast-fail
    check alone can't: yt-dlp following a redirect to a different (private)
    host, or a hostname resolving differently between the initial check and
    yt-dlp's own lookup (DNS rebinding).
  * yt-dlp runs with --ignore-config (no attacker-supplied config is read),
    --restrict-filenames, --no-playlist and a --max-filesize cap. It never
    runs post-process commands.
  * The download lands in a temp dir *inside* OUTPUT_DIR, then the single
    produced file is moved up with a sanitized basename whose realpath is
    verified to stay within OUTPUT_DIR (no path traversal), and never
    overwrites an existing file (a numeric suffix is added on collision).
"""
import ipaddress
import json
import os
import re
import shutil
import socket
import subprocess
import tempfile
import threading
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

PORT = int(os.environ.get("PORT", "8095"))
DOWNLOAD_TIMEOUT_SECONDS = int(os.environ.get("DOWNLOAD_TIMEOUT_SECONDS", "1800"))
OUTPUT_DIR = os.environ.get("OUTPUT_DIR", "/output")
# Label shown to the user for the output folder (the run scripts set "Desktop").
OUTPUT_LABEL = os.environ.get("OUTPUT_LABEL", "the output folder")
MAX_FILESIZE = os.environ.get("MAX_FILESIZE", "4G")
# Set ALLOW_PRIVATE_HOSTS=1 only if you knowingly want to fetch from LAN hosts.
ALLOW_PRIVATE_HOSTS = os.environ.get("ALLOW_PRIVATE_HOSTS", "0") == "1"

KINDS = ("mp3", "wav", "mp4")

# UI assets live in static/ (index.html, style.css, main.js) rather than
# embedded in this file - see do_GET's fixed routes for the three of them.
STATIC_DIR = Path(__file__).resolve().parent / "static"


def tool_version(command: list) -> str:
    try:
        out = subprocess.check_output(command, stderr=subprocess.STDOUT, text=True, timeout=10)
        return out.splitlines()[0].strip() if out else "unknown"
    except Exception:
        return "unavailable"


def host_is_blocked(host: str) -> bool:
    """True if the host resolves to a non-public address (basic SSRF guard)."""
    if ALLOW_PRIVATE_HOSTS:
        return False
    host = host.strip("[]")
    try:
        infos = socket.getaddrinfo(host, None)
    except Exception:
        # Unresolvable here — let yt-dlp try and fail rather than hard-blocking.
        return False
    for info in infos:
        ip = info[4][0]
        try:
            addr = ipaddress.ip_address(ip)
        except ValueError:
            continue
        if (addr.is_loopback or addr.is_private or addr.is_link_local
                or addr.is_reserved or addr.is_multicast or addr.is_unspecified):
            return True
    return False


# ---------------------------------------------------------------------------
# Egress guard: a tiny in-container forward proxy yt-dlp is pointed at (see
# --proxy in handle_download), so every outbound connection it makes -- not
# just the initial URL -- is resolved and validated at actual TCP-connect
# time. `host_is_blocked` above only checks the *original* URL's host before
# yt-dlp ever runs; it can't see the further connections yt-dlp itself makes
# (redirects to a different host, CDN nodes, etc.), and a hostname could
# resolve differently between that check and yt-dlp's own lookup (DNS
# rebinding). Routing yt-dlp through this proxy closes both gaps: every CONNECT
# (https) and every absolute-URI request (http) is resolved exactly once and
# validated before the outbound socket is opened, using the same resolved
# address for the connection (no second lookup, no rebinding window).
# ---------------------------------------------------------------------------

EGRESS_PROXY_PORT = int(os.environ.get("EGRESS_PROXY_PORT", "8899"))


class _EgressBlocked(Exception):
    pass


def _resolve_validated(host: str, port: int):
    """Resolve host once and return a single validated address to connect to,
    or raise `_EgressBlocked`. Validates every returned address (not just the
    first) so a multi-A-record host can't slip a private address through."""
    try:
        infos = socket.getaddrinfo(host, port, proto=socket.IPPROTO_TCP)
    except OSError as exc:
        raise _EgressBlocked(f"DNS resolution failed for {host}: {exc}") from exc
    if not infos:
        raise _EgressBlocked(f"no address found for {host}")
    if ALLOW_PRIVATE_HOSTS:
        return infos[0]
    for info in infos:
        ip = info[4][0]
        try:
            addr = ipaddress.ip_address(ip)
        except ValueError:
            continue
        if (addr.is_loopback or addr.is_private or addr.is_link_local
                or addr.is_reserved or addr.is_multicast or addr.is_unspecified):
            raise _EgressBlocked(f"refusing to connect to blocked address {ip} for host {host}")
    return infos[0]


def _pipe(src: socket.socket, dst: socket.socket) -> None:
    try:
        while True:
            data = src.recv(65536)
            if not data:
                break
            dst.sendall(data)
    except OSError:
        pass
    finally:
        try:
            dst.shutdown(socket.SHUT_WR)
        except OSError:
            pass


def _egress_relay(client_sock, host, port, is_connect, prelude=b""):
    try:
        family, socktype, proto, _, sockaddr = _resolve_validated(host, port)
    except _EgressBlocked as exc:
        body = str(exc).encode()
        client_sock.sendall(
            b"HTTP/1.1 403 Forbidden\r\nConnection: close\r\nContent-Length: "
            + str(len(body)).encode() + b"\r\n\r\n" + body
        )
        client_sock.close()
        return

    try:
        upstream = socket.socket(family, socktype, proto)
        upstream.settimeout(15)
        upstream.connect(sockaddr)
        upstream.settimeout(None)
    except OSError:
        client_sock.sendall(b"HTTP/1.1 502 Bad Gateway\r\nConnection: close\r\n\r\n")
        client_sock.close()
        return

    if is_connect:
        client_sock.sendall(b"HTTP/1.1 200 Connection Established\r\n\r\n")
    elif prelude:
        upstream.sendall(prelude)

    t1 = threading.Thread(target=_pipe, args=(client_sock, upstream), daemon=True)
    t2 = threading.Thread(target=_pipe, args=(upstream, client_sock), daemon=True)
    t1.start()
    t2.start()
    t1.join()
    t2.join()
    for s in (upstream, client_sock):
        try:
            s.close()
        except OSError:
            pass


def _egress_client_thread(client_sock) -> None:
    try:
        client_sock.settimeout(10)
        buf = b""
        while b"\r\n\r\n" not in buf:
            chunk = client_sock.recv(4096)
            if not chunk:
                client_sock.close()
                return
            buf += chunk
            if len(buf) > 65536:
                client_sock.close()
                return
        client_sock.settimeout(None)

        header_block, sep, rest = buf.partition(b"\r\n\r\n")
        request_line = header_block.split(b"\r\n", 1)[0].decode("latin-1", "replace")
        parts = request_line.split()
        if len(parts) < 2:
            client_sock.close()
            return
        method, target = parts[0].upper(), parts[1]

        if method == "CONNECT":
            host, _, port_s = target.partition(":")
            port = int(port_s) if port_s else 443
            _egress_relay(client_sock, host, port, is_connect=True)
            return

        # Plain-HTTP absolute-URI proxying (used for http:// targets): forward
        # the original request bytes verbatim to the validated upstream so
        # redirects/CDN hosts get the same check as the initial URL.
        parsed = urllib.parse.urlparse(target)
        host = parsed.hostname
        port = parsed.port or 80
        if not host:
            client_sock.sendall(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n")
            client_sock.close()
            return
        _egress_relay(client_sock, host, port, is_connect=False, prelude=header_block + sep + rest)
    except Exception:
        try:
            client_sock.close()
        except OSError:
            pass


def start_egress_proxy() -> None:
    """Start the loopback-only egress guard proxy in a background thread."""
    server = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    server.bind(("127.0.0.1", EGRESS_PROXY_PORT))
    server.listen(64)

    def serve():
        while True:
            try:
                client_sock, _ = server.accept()
            except OSError:
                return
            threading.Thread(target=_egress_client_thread, args=(client_sock,), daemon=True).start()

    threading.Thread(target=serve, daemon=True).start()


def sanitize_basename(name: str) -> str:
    """Reduce to a single safe path component (defense-in-depth on top of
    yt-dlp's --restrict-filenames)."""
    name = os.path.basename(name)
    name = name.replace("\\", "_")
    name = re.sub(r'[<>:"/|?*\x00-\x1f]', "_", name)
    name = name.strip(" .") or "download"
    return name[:200]


def collision_safe_path(directory: str, filename: str) -> str:
    stem, ext = os.path.splitext(filename)
    candidate = os.path.join(directory, filename)
    i = 1
    while os.path.exists(candidate):
        candidate = os.path.join(directory, f"{stem}-{i}{ext}")
        i += 1
    return candidate


class Handler(BaseHTTPRequestHandler):
    server_version = "media-downloader"

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
            html = (STATIC_DIR / "index.html").read_text(encoding="utf-8")
            return self._send(200, html.replace("__OUTPUT_LABEL__", OUTPUT_LABEL))
        if path == "/static/style.css":
            return self._send(
                200, (STATIC_DIR / "style.css").read_bytes(), "text/css; charset=utf-8"
            )
        if path == "/static/main.js":
            return self._send(
                200,
                (STATIC_DIR / "main.js").read_bytes(),
                "text/javascript; charset=utf-8",
            )
        if path == "/health":
            return self._json(200, {
                "status": "ok",
                "service": "media-downloader-api",
                "output_dir": OUTPUT_DIR,
                "output_writable": os.path.isdir(OUTPUT_DIR) and os.access(OUTPUT_DIR, os.W_OK),
                "yt_dlp": tool_version(["yt-dlp", "--version"]),
                "ffmpeg": tool_version(["ffmpeg", "-version"]),
            })
        return self._send(404, b"Not found", "text/plain")

    def do_POST(self):
        if self.path.split("?", 1)[0] != "/api/download":
            return self._send(404, b"Not found", "text/plain")
        return self.handle_download()

    def handle_download(self):
        try:
            length = int(self.headers.get("Content-Length", "0") or "0")
            raw = self.rfile.read(length) if length else b"{}"
            payload = json.loads(raw or b"{}")
        except Exception:
            return self._json(400, {"detail": "Invalid JSON body."})

        url = str(payload.get("url", "")).strip()
        kind = str(payload.get("kind", "")).strip().lower()
        if kind not in KINDS:
            return self._json(400, {"detail": "kind must be mp3, wav, or mp4."})

        parsed = urllib.parse.urlparse(url)
        if parsed.scheme not in ("http", "https") or not parsed.hostname:
            return self._json(400, {"detail": "Please provide a valid http(s) URL."})
        if host_is_blocked(parsed.hostname):
            return self._json(403, {"detail": "Refusing to fetch from a local/private address."})

        if not (os.path.isdir(OUTPUT_DIR) and os.access(OUTPUT_DIR, os.W_OK)):
            return self._json(500, {
                "detail": f"Output folder {OUTPUT_DIR} is not mounted/writable. "
                          "Start the container via 'vn run ...-open-media-downloader'."
            })

        # Download into a temp dir INSIDE the output dir so the final move is a
        # same-filesystem rename and large files never touch container RAM/layers.
        try:
            job_dir = tempfile.mkdtemp(prefix=".vn-dl-", dir=OUTPUT_DIR)
        except OSError as exc:
            return self._json(500, {"detail": f"Cannot create temp dir in output: {exc}"})

        out_template = os.path.join(job_dir, "%(title).180B.%(ext)s")
        base_cmd = [
            "yt-dlp",
            "--ignore-config",
            "--no-playlist",
            "--restrict-filenames",
            "--no-exec",
            "--proxy", f"http://127.0.0.1:{EGRESS_PROXY_PORT}",
            "--max-filesize", MAX_FILESIZE,
            "--retries", "3",
            "--socket-timeout", "30",
            "-o", out_template,
        ]
        if kind in ("mp3", "wav"):
            cmd = base_cmd + ["-x", "--audio-format", kind, url]
        else:
            cmd = base_cmd + ["-f", "bv*+ba/b", "--merge-output-format", "mp4", url]

        env = dict(os.environ)
        env["HOME"] = "/tmp"
        env["XDG_CACHE_HOME"] = "/tmp"
        # Force everything through the validated egress proxy above; don't let
        # an inherited proxy env var override the --proxy flag.
        for key in ("HTTP_PROXY", "HTTPS_PROXY", "http_proxy", "https_proxy", "ALL_PROXY", "all_proxy"):
            env.pop(key, None)

        try:
            proc = subprocess.run(
                cmd, capture_output=True, text=True,
                timeout=DOWNLOAD_TIMEOUT_SECONDS, env=env,
            )
        except subprocess.TimeoutExpired:
            shutil.rmtree(job_dir, ignore_errors=True)
            return self._json(504, {"detail": "Download timed out."})

        if proc.returncode != 0:
            shutil.rmtree(job_dir, ignore_errors=True)
            tail = (proc.stderr or proc.stdout or "").strip()[-1500:]
            return self._json(400, {"detail": "yt-dlp failed: " + tail})

        # Pick the produced file matching the requested extension (largest, if several).
        produced = [
            os.path.join(job_dir, f) for f in os.listdir(job_dir)
            if f.lower().endswith("." + kind)
        ]
        if not produced:
            shutil.rmtree(job_dir, ignore_errors=True)
            return self._json(500, {"detail": "Download produced no output file."})
        src = max(produced, key=lambda p: os.path.getsize(p))

        safe_name = sanitize_basename(os.path.basename(src))
        final_path = collision_safe_path(OUTPUT_DIR, safe_name)

        # Confirm the destination really stays inside OUTPUT_DIR (no traversal).
        out_real = os.path.realpath(OUTPUT_DIR)
        final_real = os.path.realpath(final_path)
        if final_real != out_real and not final_real.startswith(out_real + os.sep):
            shutil.rmtree(job_dir, ignore_errors=True)
            return self._json(400, {"detail": "Rejected unsafe output path."})

        try:
            os.replace(src, final_path)
        except OSError as exc:
            shutil.rmtree(job_dir, ignore_errors=True)
            return self._json(500, {"detail": f"Could not save file: {exc}"})
        shutil.rmtree(job_dir, ignore_errors=True)

        return self._json(200, {
            "ok": True,
            "filename": os.path.basename(final_path),
            "saved_to": OUTPUT_LABEL,
        })

    def log_message(self, fmt, *args):
        print("[media-downloader] " + (fmt % args))


def main():
    start_egress_proxy()
    ThreadingHTTPServer(("0.0.0.0", PORT), Handler).serve_forever()


if __name__ == "__main__":
    main()
