#!/bin/bash
# Cleanup first
pkill -f "python3 -m http.server 800" 2>/dev/null
sleep 1

echo "🚀 Démarrage de 3 backends..."
mkdir -p /tmp/backend1 && echo "OK" > /tmp/backend1/api
python3 -m http.server 8001 --directory /tmp/backend1 > /dev/null 2>&1 &
python3 -m http.server 8002 --directory /tmp/backend1 > /dev/null 2>&1 &
python3 -m http.server 8003 --directory /tmp/backend1 > /dev/null 2>&1 &
sleep 1

echo "⏳ Attente de 12 secondes pour que le Health Checker marque les backends comme HEALTHY..."
sleep 12

echo "===== PHASE 1 : SUCCÈS ====="
echo "🟢 Envoi de 150 requêtes..."
for i in {1..150}; do
    RESULT=$(curl -s -o /dev/null -w "%{http_code}" -H "Cache-Control: no-cache" http://127.0.0.1:8080/api 2>/dev/null)
    if [ "$i" -le 3 ] || [ "$i" -eq 75 ] || [ "$i" -eq 150 ]; then
        echo "  Req #$i -> HTTP $RESULT"
    fi
    sleep 0.02
done
echo "✅ Phase 1 terminée."
sleep 2

echo "===== PHASE 2 : ERREURS ====="
echo "💀 Extinction de TOUS les backends..."
pkill -f "python3 -m http.server 800"
sleep 2

echo "⏳ Attente de 12 secondes pour que le Health Checker détecte les backends MORTS..."
sleep 12

echo "🔴 Envoi de 150 requêtes vers des backends MORTS..."
for i in {1..150}; do
    RESULT=$(curl -s -o /dev/null -w "%{http_code}" -H "Cache-Control: no-cache" http://127.0.0.1:8080/api 2>/dev/null)
    if [ "$i" -le 3 ] || [ "$i" -eq 75 ] || [ "$i" -eq 150 ]; then
        echo "  Req #$i -> HTTP $RESULT"
    fi
    sleep 0.02
done
echo ""
echo "🛑 Test terminé ! Envoyez /status sur Telegram !"
