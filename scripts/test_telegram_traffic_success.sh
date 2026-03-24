#!/bin/bash
echo "🚀 Démarrage de 3 backends Python locaux (ports 8001, 8002, 8003)..."
mkdir -p /tmp/backend1 && echo "Hello depuis Backend 1" > /tmp/backend1/api
mkdir -p /tmp/backend2 && echo "Hello depuis Backend 2" > /tmp/backend2/api
mkdir -p /tmp/backend3 && echo "Hello depuis Backend 3" > /tmp/backend3/api

echo "⏳ Lancement des serveurs..."
python3 -m http.server 8001 --directory /tmp/backend1 > /dev/null 2>&1 &
PID1=$!
python3 -m http.server 8002 --directory /tmp/backend2 > /dev/null 2>&1 &
PID2=$!
python3 -m http.server 8003 --directory /tmp/backend3 > /dev/null 2>&1 &
PID3=$!

sleep 1

echo "🟢 Génération de 500 requêtes vers la route valide /api (http://127.0.0.1:8080/api)..."

for i in {1..500}; do
    curl -s http://127.0.0.1:8080/api > /dev/null
    sleep 0.05
done

echo "🛑 Fin du test réussi. Extinction des backends..."
kill $PID1 $PID2 $PID3
echo "Nettoyage terminé."
