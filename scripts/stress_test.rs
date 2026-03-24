use reqwest::Client;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::task::JoinSet;

#[tokio::main]
async fn main() {
    let target = std::env::args().nth(1).unwrap_or_else(|| "http://127.0.0.1:8080/api".to_string());
    let duration_secs: u64 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(15);
    let concurrency: usize = std::env::args().nth(3).and_then(|s| s.parse().ok()).unwrap_or(1000);

    println!("🔥🔥🔥 BASTION DDoS STRESS TEST (Rust Async) 🔥🔥🔥");
    println!("");
    println!("🎯 Cible: {}", target);
    println!("⏱️  Durée: {}s", duration_secs);
    println!("🔗 Concurrence: {} tâches async", concurrency);
    println!("📊 Dashboard: http://127.0.0.1:8082");
    println!("");
    println!("Démarrage dans 2s...");
    tokio::time::sleep(Duration::from_secs(2)).await;

    let client = Client::builder()
        .pool_max_idle_per_host(concurrency)
        .tcp_keepalive(Some(Duration::from_secs(30)))
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();

    let total_ok = Arc::new(AtomicU64::new(0));
    let total_err = Arc::new(AtomicU64::new(0));
    let deadline = Instant::now() + Duration::from_secs(duration_secs);

    println!("🚀 LANCEMENT DE L'ATTAQUE !");
    let start = Instant::now();

    let mut set = JoinSet::new();
    for _ in 0..concurrency {
        let client = client.clone();
        let url = target.clone();
        let ok = total_ok.clone();
        let err = total_err.clone();

        set.spawn(async move {
            while Instant::now() < deadline {
                match client.get(&url).send().await {
                    Ok(_) => { ok.fetch_add(1, Ordering::Relaxed); }
                    Err(_) => { err.fetch_add(1, Ordering::Relaxed); }
                }
            }
        });
    }

    // Affichage temps réel pendant l'attaque
    let total_ok_display = total_ok.clone();
    let total_err_display = total_err.clone();
    let display_handle = tokio::spawn(async move {
        let mut last_total = 0u64;
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let ok = total_ok_display.load(Ordering::Relaxed);
            let err = total_err_display.load(Ordering::Relaxed);
            let current_total = ok + err;
            let rps = current_total - last_total;
            last_total = current_total;
            let elapsed = start.elapsed().as_secs();
            println!("  ⚡ [{}s] {} req/s | Total: {} | 🟢 {} | 🔴 {}", elapsed, rps, current_total, ok, err);
            if Instant::now() >= deadline {
                break;
            }
        }
    });

    // Attendre la fin de toutes les tâches
    while let Some(_) = set.join_next().await {}
    display_handle.abort();

    let elapsed = start.elapsed();
    let ok = total_ok.load(Ordering::Relaxed);
    let err = total_err.load(Ordering::Relaxed);
    let total = ok + err;
    let rps = if elapsed.as_secs() > 0 { total / elapsed.as_secs() } else { total };

    println!("");
    println!("════════════════════════════════════════════");
    println!("📊 RÉSULTATS DU STRESS TEST");
    println!("════════════════════════════════════════════");
    println!("📨 Total requêtes:  {}", total);
    println!("🟢 Succès:          {}", ok);
    println!("🔴 Erreurs réseau:  {}", err);
    println!("⏱️  Durée:           {:.2}s", elapsed.as_secs_f64());
    println!("🚀 Req/sec moyen:   {}", rps);
    println!("🔗 Concurrence:     {}", concurrency);
    println!("════════════════════════════════════════════");
    println!("");
    println!("👉 Vérifiez le dashboard: http://127.0.0.1:8082");
}
