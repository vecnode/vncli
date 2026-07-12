# media-downloader

A tiny web UI/API wrapping `yt-dlp` + `ffmpeg` to save video/audio from a pasted link.

- Image: built locally as `vncli-media-downloader` from this folder (`debian:12-slim` +
  a single stdlib `app.py`; `yt-dlp` is installed via `pip`, not the stale Debian apt
  package, so extractors stay current).
- UI assets live in [static/](static/) (`index.html`, `style.css`, `main.js`) — same light
  style as doc-processor's UI, served from disk rather than embedded in `app.py`.
- Paste a URL, pick **MP3 / WAV / MP4**; the file is saved to the host folder bind-mounted
  at `/output` (the launcher uses the host **Desktop**).

Because it fetches arbitrary, untrusted URLs, it's hardened beyond the usual cap-drop/
non-root posture (see [SECURITY.md](../../SECURITY.md)):
- Only `http`/`https` URLs are accepted; an initial check rejects hosts resolving to
  loopback/private/link-local ranges.
- `yt-dlp` itself is routed through a small in-container **egress-guard proxy**
  (`start_egress_proxy` in `app.py`) that re-resolves and re-validates *every* connection
  it makes at actual connect time — this catches a redirect to a different (private) host
  or DNS rebinding between the initial check and yt-dlp's own lookup, not just the first URL.
- `yt-dlp` runs with `--ignore-config --restrict-filenames --no-exec --max-filesize`, and
  the output filename is sanitized, traversal-checked, and collision-safe.

Run it from the vncli TUI **Open** menu (`open-media-downloader`) — it builds the image,
starts the container on **port 8095**, and opens Chrome at `http://localhost:8095`. Stop
with `stop-media-downloader`.

## Manual run

From repository root:

```bash
docker build -t vncli-media-downloader:latest docker/media-downloader
docker rm -f media-downloader 2>/dev/null || true
docker run -d --name media-downloader \
	-p 127.0.0.1:8095:8095 \
	--cap-drop ALL --security-opt no-new-privileges \
	-v "$HOME/Desktop:/output" \
	vncli-media-downloader:latest
```

## URLs

- UI: http://localhost:8095
- API: `POST /api/download` (`{"url": "...", "kind": "mp3"|"wav"|"mp4"}`)
- Health: http://localhost:8095/health
