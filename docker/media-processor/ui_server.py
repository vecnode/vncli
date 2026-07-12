#!/usr/bin/env python3
from __future__ import annotations

import http.server
import socketserver
import sys
from pathlib import Path


def main() -> None:
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8085
    base_dir = Path(__file__).resolve().parent / "ui"

    class Handler(http.server.SimpleHTTPRequestHandler):
        def __init__(self, *args, **kwargs):
            super().__init__(*args, directory=str(base_dir), **kwargs)

    class ReusableTCPServer(socketserver.TCPServer):
        allow_reuse_address = True

    with ReusableTCPServer(("0.0.0.0", port), Handler) as httpd:
        print(f"[INFO] UI server listening on 0.0.0.0:{port}")
        httpd.serve_forever()


if __name__ == "__main__":
    main()
