#!/bin/bash
# =============================================
# 🔥 BASTION MULTI-USER EXTREME STRESS TEST
# Simule 5 utilisateurs massifs pendant 40s
# =============================================

TARGET="http://127.0.0.1:8080/api"
DURATION=120
CONCURRENCY_PER_USER=1000 # 1000 connexions par utilisateur
USERS=5

echo "🔥🔥🔥 EXTREME DDOS SIMULATION 🔥🔥🔥"
echo "👥 Utilisateurs : $USERS"
echo "🔗 Connexions totales : $((USERS * CONCURRENCY_PER_USER))"
echo "⏱️  Durée : ${DURATION}s"
echo "🎯 Cible : $TARGET"
echo ""
echo "🚀 Lancement de l'attaque..."

# Les logs de chaque utilisateur iront dans un fichier /tmp
rm -f /tmp/bastion_stress_user_*.log

# Lancer les 5 utilisateurs en arrière-plan
for i in $(seq 1 $USERS); do
    echo "⚡ Démarrage Utilisateur $i..."
    # Lancer le stress tester Rust
    ./scripts/stress/target/release/stress "$TARGET" "$DURATION" "$CONCURRENCY_PER_USER" > "/tmp/bastion_stress_user_$i.log" &
    PIDS[$i]=$!
done

echo ""
echo "⏳ Attaque en cours pendant ${DURATION}s... (Regardez le dashboard http://127.0.0.1:8082)"

# Attendre que tous les process terminent
for pid in ${PIDS[*]}; do
    wait $pid
done

echo ""
echo "✅ Attaque terminée. Calcul des résultats globaux..."

TOTAL_REQ=0
TOTAL_OK=0
TOTAL_ERR=0

for i in $(seq 1 $USERS); do
    LOG_FILE="/tmp/bastion_stress_user_$i.log"
    # Extraire les valeurs depuis le log du stress tester
    REQ=$(grep "Total requêtes:" "$LOG_FILE" | awk '{print $3}' | tr -d '\r')
    OK=$(grep "Succès:" "$LOG_FILE" | awk '{print $2}' | tr -d '\r')
    ERR=$(grep "Erreurs réseau:" "$LOG_FILE" | awk '{print $3}' | tr -d '\r')
    
    TOTAL_REQ=$((TOTAL_REQ + ${REQ:-0}))
    TOTAL_OK=$((TOTAL_OK + ${OK:-0}))
    TOTAL_ERR=$((TOTAL_ERR + ${ERR:-0}))
done

RPS=$((TOTAL_REQ / DURATION))

echo "════════════════════════════════════════════"
echo "📊 RÉSULTATS GLOBAUX DU MEGA STRESS TEST"
echo "════════════════════════════════════════════"
echo "👥 Utilisateurs virtuels: $USERS"
echo "🔗 Total connexions:      $((USERS * CONCURRENCY_PER_USER))"
echo "⏱️  Durée:                 ${DURATION}s"
echo "📨 Total requêtes:        $TOTAL_REQ"
echo "🟢 Total Succès:          $TOTAL_OK"
echo "🔴 Total Erreurs:         $TOTAL_ERR"
echo "🚀 Req/sec (RPS) moyen:   $RPS"
echo "════════════════════════════════════════════"
