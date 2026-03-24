#!/bin/bash
# =============================================
# 🔥 BASTION DDoS STRESS TEST
# Simule 100 000+ requêtes/seconde
# =============================================

echo "🔥🔥🔥 BASTION DDoS STRESS TEST 🔥🔥🔥"
echo ""

TARGET="http://127.0.0.1:8080/api"
DURATION=15          # secondes
CONCURRENCY=500      # connexions simultanées
TOTAL=0

echo "🎯 Cible: $TARGET"
echo "⏱️  Durée: ${DURATION}s"
echo "🔗 Concurrence: $CONCURRENCY connexions"
echo ""
echo "📊 Ouvrez http://127.0.0.1:8082 dans votre navigateur pour voir le dashboard en temps réel !"
echo ""
echo "Démarrage dans 3 secondes..."
sleep 3

START=$(date +%s%N)

# Phase 1: Attaque massive avec xargs parallèle
echo "🚀 PHASE 1 — Attaque massive (${DURATION}s)..."

# Fonction pour envoyer des requêtes en boucle pendant $DURATION secondes
ddos_worker() {
    END=$(($(date +%s) + $1))
    COUNT=0
    while [ $(date +%s) -lt $END ]; do
        # Envoyer 10 requêtes en rafale
        curl -s -o /dev/null "$2" &
        curl -s -o /dev/null "$2" &
        curl -s -o /dev/null "$2" &
        curl -s -o /dev/null "$2" &
        curl -s -o /dev/null "$2" &
        curl -s -o /dev/null "$2" &
        curl -s -o /dev/null "$2" &
        curl -s -o /dev/null "$2" &
        curl -s -o /dev/null "$2" &
        curl -s -o /dev/null "$2" &
        wait
        COUNT=$((COUNT + 10))
    done
    echo $COUNT
}

export -f ddos_worker

# Lancer $CONCURRENCY workers en parallèle
echo "Lancement de $CONCURRENCY workers..."

RESULTS=$(seq $CONCURRENCY | xargs -P $CONCURRENCY -I {} bash -c "ddos_worker $DURATION $TARGET" 2>/dev/null)

END_TIME=$(date +%s%N)
ELAPSED=$(( (END_TIME - START) / 1000000 )) # en ms

# Calculer le total
TOTAL=0
for r in $RESULTS; do
    TOTAL=$((TOTAL + r))
done

ELAPSED_S=$((ELAPSED / 1000))
if [ $ELAPSED_S -gt 0 ]; then
    RPS=$((TOTAL / ELAPSED_S))
else
    RPS=$TOTAL
fi

echo ""
echo "════════════════════════════════════════"
echo "📊 RÉSULTATS DU STRESS TEST"
echo "════════════════════════════════════════"
echo "📨 Total requêtes: $TOTAL"
echo "⏱️  Durée: ${ELAPSED_S}s"
echo "🚀 Req/sec: $RPS"
echo "🔗 Concurrence: $CONCURRENCY"
echo "════════════════════════════════════════"
echo ""
echo "👉 Vérifiez le dashboard: http://127.0.0.1:8082"
