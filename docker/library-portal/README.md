# library

A lightweight web viewer/manager for the repo's `library/` folder.

- Image: built locally as `vncli-library` from this folder
  (`python:3.12-slim` + a single stdlib `app.py`, plus PyMuPDF for thumbnails).
- The build context is **only this folder**, so **no PDFs are baked into the image**.
- At runtime the launcher bind-mounts the repo `library/` to `/library`. The server walks
  it on each request and renders an index in the same light, simple style as doc-processor's
  UI (shared CSS tokens, no serif headings), streaming PDFs inline for the browser's PDF
  viewer.

Features:
- **Edit** a document's display title / author / year and **rename** the file on disk.
- **Tags** per document (e.g. `read`), shown as `#tag` chips and searchable.
- **List, Grid and Tree views** — grid shows a first-page **thumbnail** per document; tree
  is a file-explorer layout where you can **create folders** and **drag PDFs into folders**.
- **Sort** by year (new/old) or title (A–Z / Z–A), plus live search.

State: metadata overrides + tags live in `library/.portal/portal.json`, and thumbnails are
cached under `library/.portal/thumbs/` (both gitignored, hidden from the listing). The PDFs
themselves are only modified on an explicit rename.

Run it from the vncli TUI **Open** menu (`vn app open library`) — it builds the image,
starts the container with `library/` mounted on **port 8090**, and opens Chrome at
`http://localhost:8090`. Stop with `vn app stop library`.
