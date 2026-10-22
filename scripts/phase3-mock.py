"""Localhost mock for fixtures/phase3-chain (manual and browser checks).

Usage: python3 scripts/phase3-mock.py [port]   (default 18090)
"""
import json
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

TOKEN = "t-abc"


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def _send(self, status, body):
        data = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def _body(self):
        n = int(self.headers.get("content-length") or 0)
        return json.loads(self.rfile.read(n) or b"null")

    def _authed(self):
        return self.headers.get("authorization") == f"Bearer {TOKEN}" and self.headers.get("x-run") == "chain"

    def do_POST(self):
        body = self._body()
        if self.path == "/login":
            if body == {"user": "ana", "password": "secret"}:
                return self._send(200, {"token": TOKEN})
            return self._send(401, {"error": "bad credentials"})
        if self.path == "/users":
            if not self._authed() or self.headers.get("x-request-id") != f"req-{TOKEN}":
                return self._send(401, {"error": "unauthorized"})
            return self._send(201, {"id": 42, "name": body.get("name")})
        self._send(404, {"error": "not found"})

    def do_GET(self):
        if self.path == "/me":
            return self._send(200, {"name": "Ana"}) if self._authed() else self._send(401, {"error": "unauthorized"})
        self._send(404, {"error": "not found"})

    def log_message(self, *args):
        pass


port = int(sys.argv[1]) if len(sys.argv) > 1 else 18090
ThreadingHTTPServer(("127.0.0.1", port), Handler).serve_forever()
