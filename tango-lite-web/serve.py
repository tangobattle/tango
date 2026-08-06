#!/usr/bin/env python3
"""Serve dist/ for local testing, with the COOP/COEP headers the shared
wasm memory needs (in production the _headers file tells Cloudflare
Pages to send the same two — a plain `python3 -m http.server` doesn't,
and the module then fails to instantiate).

Usage: python3 serve.py [port]
"""
import http.server
import pathlib
import sys


class Handler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=str(pathlib.Path(__file__).parent / "dist"), **kwargs)

    def end_headers(self):
        self.send_header("Cross-Origin-Opener-Policy", "same-origin")
        self.send_header("Cross-Origin-Embedder-Policy", "require-corp")
        self.send_header("Cache-Control", "no-store")
        super().end_headers()


port = int(sys.argv[1]) if len(sys.argv) > 1 else 8080
print(f"http://127.0.0.1:{port}/")
http.server.ThreadingHTTPServer(("127.0.0.1", port), Handler).serve_forever()
