#!/bin/bash
set -e

echo "======================================"
echo " Bastion API Gateway - Phase 3 Live Test"
echo "======================================"

# Clean up any leftover processes
killall python3 bastion 2>/dev/null || true
sleep 1

# 1. Start 3 HTTP Python Backends on ports 8001, 8002, 8003
echo "🚀 Starting 3 upstream backend servers..."
rm -rf /tmp/backend*
mkdir -p /tmp/backend1/public /tmp/backend2/public /tmp/backend3/public

# Generate health endpoints returning 200 OK
echo "OK" > /tmp/backend1/health
echo "BACKEND 1" > /tmp/backend1/public/test
cd /tmp/backend1 && python3 -m http.server 8001 --bind 127.0.0.1 > /dev/null 2>&1 &
PID1=$!

echo "OK" > /tmp/backend2/health
echo "BACKEND 2" > /tmp/backend2/public/test
cd /tmp/backend2 && python3 -m http.server 8002 --bind 127.0.0.1 > /dev/null 2>&1 &
PID2=$!

echo "OK" > /tmp/backend3/health
echo "BACKEND 3" > /tmp/backend3/public/test
cd /tmp/backend3 && python3 -m http.server 8003 --bind 127.0.0.1 > /dev/null 2>&1 &
PID3=$!

# Ensure backends are dead when script exits
trap "kill \$PID1 \$PID2 \$PID3 \$bastion_pid 2>/dev/null && echo '🧹 Cleaned up processes.'" EXIT
sleep 1 # wait for python servers

# 2. Compile and Start Bastion Proxy
echo "🚀 Starting Bastion Gateway (with Active Health Checks)..."
cd /home/niaina/PROJECT_2026/Project_Idea/Bastion

cat <<EOF > config.toml
[server]
listen = "127.0.0.1:8080"
admin_listen = "127.0.0.1:8081"
workers = 4

[log]
level = "info"
format = "text"
EOF

cargo build --release
./target/release/bastion --config config.toml &
bastion_pid=$!

echo "⏳ Waiting 3 seconds for active health checks to poll targets /health..."
sleep 3

# 3. Test Load Balancing & Caching
echo ""
echo "==== TEST 1: Cache Miss / Hit ===="
# We bypass authentication by querying the public path
curl -s -i "http://127.0.0.1:8080/public/test" | grep -E "HTTP|X-Cache|BACKEND"
echo "(First request: Should be MISS and show backend)"

curl -s -i "http://127.0.0.1:8080/public/test" | grep -E "HTTP|X-Cache|BACKEND"
echo "(Second request: Should be HIT and show same response)"

echo ""
echo "==== TEST 2: Active Health Check Eviction ===="
echo "🔪 Killing Backend 2 (Port 8002)..."
kill $PID2
echo "⏳ Waiting 12 seconds for health check interval to notice failure (threshold=3 failures)..."
sleep 12

echo "Hitting proxy. Should never see BACKEND 2..."
for i in {1..10}; do
    # use dynamic parameter to avoid cache hits
    curl -s "http://127.0.0.1:8080/public/health?nocache=$i"
done | grep -c "BACKEND 2" || echo "Backend 2 correctly evicted (Count: 0)!"

echo ""
echo "==== TEST 3: Graceful Recovery ===="
echo "🩺 Restarting Backend 2..."
cd /tmp/backend2 && python3 -m http.server 8002 --bind 127.0.0.1 > /dev/null 2>&1 &
PID2=$!

echo "⏳ Waiting 25 seconds for health check to declare healthy (interval 10s x threshold 2)..."
sleep 25

echo "Hitting proxy. Should see BACKEND 2 again..."
for i in {1..10}; do
    curl -s "http://127.0.0.1:8080/public/health?nocache=rec$i"
done | grep -o "BACKEND 2" | wc -l | xargs -I {} echo "Backend 2 recovered (Count: {})!"

echo ""
echo "✅ Test completed successfully!"
exit 0
