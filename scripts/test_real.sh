#!/bin/bash
set -e

echo "╔══════════════════════════════════════════════════╗"
echo "║   🏰 Bastion API Gateway — Real E2E Test        ║"
echo "╚══════════════════════════════════════════════════╝"
echo ""

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

# Kill any leftover processes from previous runs
cleanup() {
    echo ""
    echo "🧹 Cleaning up..."
    kill $BACKEND_A_PID 2>/dev/null || true
    kill $BACKEND_B_PID 2>/dev/null || true
    kill $BASTION_PID 2>/dev/null || true
    echo "✅ All processes stopped."
}
trap cleanup EXIT

# 1. Start Backend Servers
echo "━━━ Step 1: Starting Mock Backend Servers ━━━"
python3 "$SCRIPT_DIR/backend_a.py" &
BACKEND_A_PID=$!

python3 "$SCRIPT_DIR/backend_b.py" &
BACKEND_B_PID=$!

sleep 1

# 2. Build and Start Bastion
echo ""
echo "━━━ Step 2: Building & Starting Bastion ━━━"
cargo build --manifest-path "$PROJECT_DIR/Cargo.toml" 2>&1 | tail -3

"$PROJECT_DIR/target/debug/bastion" --config "$PROJECT_DIR/config/bastion.toml" &
BASTION_PID=$!

sleep 2
echo ""

# ──────────────────────────────────────────
# TEST SUITE
# ──────────────────────────────────────────

PASS=0
FAIL=0

run_test() {
    local test_name="$1"
    local expected="$2"
    local actual="$3"

    if echo "$actual" | grep -q "$expected"; then
        echo "  ✅ PASS: $test_name"
        PASS=$((PASS + 1))
    else
        echo "  ❌ FAIL: $test_name"
        echo "     Expected to contain: $expected"
        echo "     Got: $actual"
        FAIL=$((FAIL + 1))
    fi
}

echo "━━━ Step 3: Running Real Tests ━━━"
echo ""

# Test 1: Basic GET through proxy → should hit one of the backends
echo "📌 Test 1: GET /api/test (proxy → backend)"
RESULT1=$(curl -s http://localhost:8080/api/test)
run_test "GET /api/test returns JSON" '"method": "GET"' "$RESULT1"
echo "   Response: $RESULT1"
echo ""

# Test 2: Second GET → should hit the OTHER backend (Round Robin)
echo "📌 Test 2: GET /api/test (Round Robin verification)"
RESULT2=$(curl -s http://localhost:8080/api/test)
run_test "Round Robin alternates server" '"server"' "$RESULT2"

# Check if server changed
SERVER1=$(echo "$RESULT1" | grep -o '"server": "[^"]*"')
SERVER2=$(echo "$RESULT2" | grep -o '"server": "[^"]*"')
if [ "$SERVER1" != "$SERVER2" ]; then
    echo "  ✅ PASS: Round Robin switched from $SERVER1 → $SERVER2"
    PASS=$((PASS + 1))
else
    echo "  ❌ FAIL: Round Robin did NOT alternate (both: $SERVER1)"
    FAIL=$((FAIL + 1))
fi
echo ""

# Test 3: POST with JSON body
echo "📌 Test 3: POST /api/data with JSON body"
RESULT3=$(curl -s -X POST http://localhost:8080/api/data -H "Content-Type: application/json" -d '{"user":"niaina","action":"test"}')
run_test "POST body forwarded" '"body_received"' "$RESULT3"
run_test "POST body content intact" 'niaina' "$RESULT3"
echo "   Response: $RESULT3"
echo ""

# Test 4: X-Request-ID injection
echo "📌 Test 4: X-Request-ID header injection"
RESULT4=$(curl -s http://localhost:8080/api/headers)
run_test "X-Request-ID present" '"X-Request-ID"' "$RESULT4"
# Check UUID format (8-4-4-4-12)
REQ_ID=$(echo "$RESULT4" | grep -o '"X-Request-ID": "[^"]*"' | cut -d'"' -f4)
if echo "$REQ_ID" | grep -qE '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'; then
    echo "  ✅ PASS: X-Request-ID is valid UUID ($REQ_ID)"
    PASS=$((PASS + 1))
else
    echo "  ❌ FAIL: X-Request-ID is NOT a valid UUID ($REQ_ID)"
    FAIL=$((FAIL + 1))
fi
echo ""

# Test 5: X-Real-IP injection
echo "📌 Test 5: X-Real-IP header injection"
run_test "X-Real-IP present" '"X-Real-IP": "127.0.0.1"' "$RESULT4"
echo ""

# Test 6: X-Forwarded-For injection
echo "📌 Test 6: X-Forwarded-For header injection"
run_test "X-Forwarded-For present" '"X-Forwarded-For": "127.0.0.1"' "$RESULT4"
echo ""

# Test 7: 404 for unregistered route
echo "📌 Test 7: GET /unknown → 404"
RESULT7_STATUS=$(curl -s -o /dev/null -w "%{http_code}" http://localhost:8080/unknown)
if [ "$RESULT7_STATUS" = "404" ]; then
    echo "  ✅ PASS: Unregistered route returns 404"
    PASS=$((PASS + 1))
else
    echo "  ❌ FAIL: Expected 404, got $RESULT7_STATUS"
    FAIL=$((FAIL + 1))
fi
echo ""

# Test 8: Multiple rapid requests (connection pooling)
echo "📌 Test 8: 10 rapid requests (Connection Pool stress)"
ALL_OK=true
for i in $(seq 1 10); do
    STATUS=$(curl -s -o /dev/null -w "%{http_code}" http://localhost:8080/api/pool/$i)
    if [ "$STATUS" != "200" ]; then
        ALL_OK=false
        break
    fi
done
if $ALL_OK; then
    echo "  ✅ PASS: All 10 pooled requests returned 200"
    PASS=$((PASS + 1))
else
    echo "  ❌ FAIL: Some requests failed"
    FAIL=$((FAIL + 1))
fi
echo ""

# ──────────────────────────────────────────
# RESULTS
# ──────────────────────────────────────────

echo "╔══════════════════════════════════════════════════╗"
echo "║           📊 TEST RESULTS                       ║"
echo "╠══════════════════════════════════════════════════╣"
echo "║  ✅ Passed: $PASS                                "
echo "║  ❌ Failed: $FAIL                                "
echo "║  Total:    $((PASS + FAIL))                      "
echo "╚══════════════════════════════════════════════════╝"

if [ "$FAIL" -gt 0 ]; then
    exit 1
fi
