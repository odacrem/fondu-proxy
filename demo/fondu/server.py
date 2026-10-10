"""Demo fondu backend: answers every GET with the same directives.

Like any origin, it receives the client's request (path, query, headers and
cookies) and controls caching with its response headers. A real fondu backend
would choose directives based on the request.
"""

import sys
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

DIRECTIVES = Path(__file__).with_name("directives.json")


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_GET(self):
        cookie = self.headers.get("Cookie", "-")
        print(f"fondu: GET {self.path} cookie={cookie}", flush=True)
        body = DIRECTIVES.read_bytes()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "public, max-age=60")
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 4000
    HTTPServer(("127.0.0.1", port), Handler).serve_forever()
