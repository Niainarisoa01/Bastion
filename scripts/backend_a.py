#!/usr/bin/env python3
"""Mock Backend Server A - Port 8001"""
from http.server import HTTPServer, BaseHTTPRequestHandler
import json

class BackendHandler(BaseHTTPRequestHandler):
    def do_GET(self):
        response = {
            "server": "Backend-A",
            "port": 8001,
            "path": self.path,
            "method": "GET",
            "headers": {
                "X-Request-ID": self.headers.get("X-Request-ID", "none"),
                "X-Real-IP": self.headers.get("X-Real-IP", "none"),
                "X-Forwarded-For": self.headers.get("X-Forwarded-For", "none"),
            }
        }
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(json.dumps(response, indent=2).encode())

    def do_POST(self):
        content_length = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(content_length).decode() if content_length > 0 else ""
        response = {
            "server": "Backend-A",
            "port": 8001,
            "path": self.path,
            "method": "POST",
            "body_received": body,
            "headers": {
                "X-Request-ID": self.headers.get("X-Request-ID", "none"),
                "X-Real-IP": self.headers.get("X-Real-IP", "none"),
                "X-Forwarded-For": self.headers.get("X-Forwarded-For", "none"),
            }
        }
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(json.dumps(response, indent=2).encode())

    def log_message(self, format, *args):
        print(f"  [Backend-A:8001] {args[0]}")

if __name__ == "__main__":
    server = HTTPServer(("127.0.0.1", 8001), BackendHandler)
    print("🟢 Backend A running on http://127.0.0.1:8001")
    server.serve_forever()
