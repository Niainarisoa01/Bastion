#!/bin/bash
set -e

echo "==========================================="
echo "   BASTION PHASE 2 : LIVE TEST SCRIPT"
echo "==========================================="

echo "1. Generating JWT token with HS256..."
SECRET="bastion-test-secret"
TOKEN=$(python3 -c "
import hmac, hashlib, base64, json
def encode(d): return base64.urlsafe_b64encode(json.dumps(d).encode('utf-8')).decode('utf-8').rstrip('=')
h = encode({'alg': 'HS256', 'typ': 'JWT'})
p = encode({'sub': 'admin-user', 'role': 'superadmin', 'exp': 1900000000})
sig = base64.urlsafe_b64encode(hmac.new(b'$SECRET', f'{h}.{p}'.encode('utf-8'), hashlib.sha256).digest()).decode('utf-8').rstrip('=')
print(f'{h}.{p}.{sig}')
")

echo "Generated Token: Bearer $TOKEN"
echo ""

echo "2. Starting Mock Python Backends on ports 8001 and 8002..."
python3 -c "
import http.server, socketserver
class Handler(http.server.SimpleHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.end_headers()
        self.wfile.write(b'Response from Backend 8001\n')
socketserver.TCPServer(('', 8001), Handler).serve_forever()
" &
PID1=$!

python3 -c "
import http.server, socketserver
class Handler(http.server.SimpleHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.end_headers()
        self.wfile.write(b'Response from Backend 8002\n')
socketserver.TCPServer(('', 8002), Handler).serve_forever()
" &
PID2=$!

echo "3. Building and starting Bastion Proxy in background..."
cargo build
target/debug/bastion --config config/bastion.toml &
BASTION_PID=$!

sleep 3 # Wait for startup

echo ""
echo "=========================================================="
echo " TEST 1: CORS PREFLIGHT (OPTIONS)"
echo " Should return 204 No Content with Access-Control headers"
echo "=========================================================="
curl -s -i -X OPTIONS http://127.0.0.1:8080/api/test \
    -H "Origin: http://example.com" \
    -H "Access-Control-Request-Method: GET" | grep -E "HTTP/|Access-Control"

echo ""
echo "=========================================================="
echo " TEST 2: JWT AUTH - MISSING TOKEN"
echo " Should return 401 Unauthorized"
echo "=========================================================="
curl -s -i http://127.0.0.1:8080/api/test | grep "HTTP/"

echo ""
echo "=========================================================="
echo " TEST 3: JWT AUTH - SKIP PATH"
echo " Route /public should bypass JWT and succeed (200 OK)"
echo "=========================================================="
curl -s -i http://127.0.0.1:8080/public/test | grep "HTTP/"

echo ""
echo "=========================================================="
echo " TEST 4: JWT AUTH - VALID TOKEN & CLAIMS INJECTION"
echo " Should pass and backend should receive x-user-id header"
echo "=========================================================="
curl -s http://127.0.0.1:8080/api/data -H "Authorization: Bearer $TOKEN" | grep -E "Response|x-user-id"

echo ""
echo "=========================================================="
echo " TEST 5: REQUEST VALIDATION - PAYLOAD TOO LARGE"
echo " Sending > 1MB body. Should return 413 Payload Too Large"
echo "=========================================================="
dd if=/dev/zero of=/tmp/large_file.txt bs=1M count=2 2>/dev/null
curl -s -i -X POST http://127.0.0.1:8080/api/upload \
    -H "Authorization: Bearer $TOKEN" \
    -d @/tmp/large_file.txt | grep "HTTP/"
rm /tmp/large_file.txt

echo ""
echo "=========================================================="
echo " TEST 6: RATE LIMITING - TRIGGER 429"
echo " Limit is 5 req/10s. We will make 6 rapid requests."
echo "=========================================================="
for i in {1..6}; do
    STATUS=$(curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:8080/public/spam)
    echo "Request $i: HTTP $STATUS"
done

echo ""
echo "Cleaning up..."
kill $PID1 $PID2 $BASTION_PID
wait $BASTION_PID 2>/dev/null || true
echo "Tests finished!"
