#!/usr/bin/env python3
"""A scripted, stdlib-only stand-in for a served OCR model.

Used by ``scripts/verify-ocr-worker.sh`` so the end-to-end worker path can be
proven with no GPU and no real vision model: it answers every
OpenAI-compatible chat completion with fixed text naming the fixture asset
the shell script creates, so the run produces a real claim in
``graph:extraction`` rather than an empty one.
"""

from __future__ import annotations

import json
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):  # noqa: N802 - BaseHTTPRequestHandler's own naming
        if self.path == "/health":
            self.send_response(200)
            self.end_headers()
            return
        self.send_response(404)
        self.end_headers()

    def do_POST(self):  # noqa: N802 - BaseHTTPRequestHandler's own naming
        length = int(self.headers.get("content-length", 0))
        self.rfile.read(length)  # request content is irrelevant to this script
        body = json.dumps(
            {"choices": [{"message": {"content": self.server.ocr_text, "role": "assistant"}}]}
        ).encode("utf-8")
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass  # silence the default stderr access log


if __name__ == "__main__":
    port = int(sys.argv[1])
    fqn = sys.argv[2] if len(sys.argv) > 2 else "prod.ocr-check"
    server = HTTPServer(("127.0.0.1", port), Handler)
    server.ocr_text = (
        f"{fqn} is a fixture asset created by verify-ocr-worker.sh "
        "for end-to-end OCR verification."
    )
    server.serve_forever()
