#!/bin/bash
echo "🚀 Démarrage de 3 backends Python locaux (ports 8001, 8002, 8003)..."
mkdir -p /tmp/backend1 && echo "Hello depuis Backend 1" > /tmp/backend1/index.html
mkdir -p /tmp/backend2 && echo "Hello depuis Backend 2" > /tmp/backend2/index.html
mkdir -p /tmp/backend3 && echo "Hello depuis Backend 3" > /tmp/backend3/index.html

echo "⏳ Lancement des serveurs..."
python3 -m http.server 8001 --directory /tmp/backend1 > /dev/null 2>&1 &
PID1=$!
python3 -m http.server 8002 --directory /tmp/backend2 > /dev/null 2>&1 &
PID2=$!
python3 -m http.server 8003 --directory /tmp/backend3 > /dev/null 2>&1 &
PID3=$!

sleep 1

echo "🔥 Génération de trafic massif vers Bastion (http://127.0.0.1:8080/)..."
echo "🚨 Une requête sur 15 sera volontairement cassée pour observer le taux d'erreur."

for i in {1..300}; do
    curl -s http://127.0.0.1:8080/ > /dev/null
    sleep 0.1
    
    # Génération d'erreurs
    if (( i % 15 == 0 )); then
        curl -s http://127.0.0.1:8080/test-fail404 > /dev/null
    fi
done

echo "🛑 Fin du test. Extinction des backends..."
kill $PID1 $PID2 $PID3
echo "Nettoyage terminé."
