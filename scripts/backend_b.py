#!/usr/bin/env python3
"""Mock Backend Server B - Port 8002"""
from http.server import HTTPServer, BaseHTTPRequestHandler
import json

class BackendHandler(BaseHTTPRequestHandler):
    def do_GET(self):
        response = {
            "server": "Backend-B",
            "port": 8002,
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
            "server": "Backend-B",
            "port": 8002,
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
        print(f"  [Backend-B:8002] {args[0]}")

if __name__ == "__main__":
    server = HTTPServer(("127.0.0.1", 8002), BackendHandler)
    print("🟢 Backend B running on http://127.0.0.1:8002")
    server.serve_forever()
