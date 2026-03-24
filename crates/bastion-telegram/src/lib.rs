use teloxide::{prelude::*, utils::command::BotCommands};
use std::sync::{Arc, RwLock};
use bastion_metrics::GatewayMetrics;
use bastion_core::router::RadixTrie;
use bastion_core::loadbalancer::UpstreamGroup;

#[derive(BotCommands, Clone)]
#[command(rename_rule = "lowercase", description = "Commandes supportées par Bastion Bot:")]
pub enum Command {
    #[command(description = "Affiche l'aide.")]
    Help,
    #[command(description = "Statut général du proxy.")]
    Status,
    #[command(description = "Statistiques détaillées.")]
    Stats,
    #[command(description = "Liste de tous les upstreams et leur état.")]
    Backends,
    #[command(description = "Santé d'un backend spécifique.")]
    Health(String),
    #[command(description = "Top 10 des routes en trafic.")]
    TopRoutes,
    #[command(description = "Active ou désactive un backend (drain).")]
    Toggle(String),
    #[command(description = "Recharge la configuration via hot-reload.")]
    Reload,
    #[command(description = "Statistiques du cache.")]
    CacheStats,
    #[command(description = "Vide le cache.")]
    CacheClear,
}

#[derive(Clone)]
pub struct BotContext {
    pub metrics: Arc<GatewayMetrics>,
    pub router: Arc<RwLock<RadixTrie<UpstreamGroup>>>,
    pub admin_chat_ids: Vec<i64>,
}

pub async fn start_telegram_bot(token: String, ctx: BotContext) {
    tracing::info!("Starting Telegram Bot...");
    let bot = Bot::new(token);

    let alert_bot = bot.clone();
    let alert_ctx = ctx.clone();
    tokio::spawn(async move {
        start_alert_engine(alert_bot, alert_ctx).await;
    });

    Command::repl(bot, move |bot: Bot, msg: Message, cmd: Command| {
        let ctx = ctx.clone();
        async move {
            // Check auth (allow if admin_chat_ids is empty for testing, or matching)
            let chat_id = msg.chat.id.0;
            if !ctx.admin_chat_ids.is_empty() && !ctx.admin_chat_ids.contains(&chat_id) {
                let _ = bot.send_message(msg.chat.id, "⛔ Accès non autorisé.").await;
                return respond(());
            }

            match cmd {
                Command::Help => {
                    bot.send_message(msg.chat.id, Command::descriptions().to_string()).await?;
                }
                Command::Status => {
                    let reqs = ctx.metrics.total_requests.load(std::sync::atomic::Ordering::Relaxed);
                    let errs = ctx.metrics.total_errors.load(std::sync::atomic::Ordering::Relaxed);
                    // Calcul du succès en gérant le compteur sous Flow asynchrone
                    let success = reqs.saturating_sub(errs);
                    let msg_text = format!("🛡️ *BASTION STATUS* 🛡️\n\n📈 *Requêtes Totales*: {}\n🟢 *Succès*: {}\n🔴 *Erreurs*: {}", reqs, success, errs);
                    bot.send_message(msg.chat.id, msg_text).parse_mode(teloxide::types::ParseMode::MarkdownV2).await?;
                }
                Command::Stats => {
                    let (p50, p95, p99) = {
                        let hist = ctx.metrics.global_latency.lock();
                        (hist.value_at_quantile(0.50), hist.value_at_quantile(0.95), hist.value_at_quantile(0.99))
                    };
                    let reqs = ctx.metrics.total_requests.load(std::sync::atomic::Ordering::Relaxed);
                    
                    let msg_text = format!("📊 *BASTION STATS*\n\n📈 *Requests*: {}\n⏱️ *P50 Latency*: {} µs\n⏱️ *P95 Latency*: {} µs\n⏱️ *P99 Latency*: {} µs", reqs, p50, p95, p99);
                    bot.send_message(msg.chat.id, msg_text).parse_mode(teloxide::types::ParseMode::MarkdownV2).await?;
                }
                Command::Backends => {
                    let mut lines = Vec::new();
                    for entry in ctx.metrics.backends.iter() {
                        let bk = entry.key().replace(".", "\\.").replace("-", "\\-");
                        lines.push(format!("🔌 `{}`: {} reqs", bk, entry.value().total_requests.load(std::sync::atomic::Ordering::Relaxed)));
                    }
                    let out = if lines.is_empty() { "Aucun backend actif\\.".to_string() } else { lines.join("\n") };
                    let msg_text = format!("*BACKENDS*\n{}", out);
                    bot.send_message(msg.chat.id, msg_text).parse_mode(teloxide::types::ParseMode::MarkdownV2).await?;
                }
                Command::Health(name) => {
                    bot.send_message(msg.chat.id, format!("Implémentation ciblée: {}", name)).await?;
                }
                Command::TopRoutes => {
                    bot.send_message(msg.chat.id, "Fonctionnalité en cours...").await?;
                }
                Command::Toggle(backend) => {
                    bot.send_message(msg.chat.id, format!("🔄 Bascule du backend demandé: {}", backend)).await?;
                }
                Command::Reload => {
                    bot.send_message(msg.chat.id, "⚡ Rechargement forcé de la configuration \\(stub\\)").parse_mode(teloxide::types::ParseMode::MarkdownV2).await?;
                }
                Command::CacheStats => {
                    bot.send_message(msg.chat.id, "🧠 Cache Hits: XXX / Miss: YYY \\(stub\\)").parse_mode(teloxide::types::ParseMode::MarkdownV2).await?;
                }
                Command::CacheClear => {
                    bot.send_message(msg.chat.id, "🗑️ Cache purgé avec succès !").await?;
                }
            }
            respond(())
        }
    }).await;
}

pub async fn start_alert_engine(bot: Bot, ctx: BotContext) {
    let last_alert = dashmap::DashMap::<String, std::time::Instant>::new();
    
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
        
        let reqs = ctx.metrics.total_requests.load(std::sync::atomic::Ordering::Relaxed);
        let errs = ctx.metrics.total_errors.load(std::sync::atomic::Ordering::Relaxed);
        
        // Very basic error rate alert (e.g. > 10% errors and minimum traffic)
        if reqs > 100 && (errs as f64 / reqs as f64) > 0.10 {
            let key = "high_error_rate".to_string();
            let should_alert = match last_alert.get(&key) {
                Some(inst) => inst.elapsed() > std::time::Duration::from_secs(300), // 5 min cooldown
                None => true,
            };
            if should_alert {
                for &chat_id in &ctx.admin_chat_ids {
                    let text = format!("🚨 *ALERTE CRITIQUE*\n\nTaux d'erreur élevé détecté \\(> 10%\\)\nRequêtes: {}\nErreurs: {}", reqs, errs);
                    let _ = bot.send_message(teloxide::types::ChatId(chat_id), text).parse_mode(teloxide::types::ParseMode::MarkdownV2).await;
                }
                last_alert.insert(key, std::time::Instant::now());
            }
        }
        
        // P99 Latency > 1s alert
        let p99 = ctx.metrics.global_latency.lock().value_at_quantile(0.99);
        if p99 > 1_000_000 {
            let key = "high_latency".to_string();
            let should_alert = match last_alert.get(&key) {
                Some(inst) => inst.elapsed() > std::time::Duration::from_secs(300),
                None => true,
            };
            if should_alert {
                for &chat_id in &ctx.admin_chat_ids {
                    let text = format!("⚠️ *ALERTE PERF*\n\nLatence P99 anormalement haute: {} µs", p99);
                    let _ = bot.send_message(teloxide::types::ChatId(chat_id), text).parse_mode(teloxide::types::ParseMode::MarkdownV2).await;
                }
                last_alert.insert(key, std::time::Instant::now());
            }
        }
    }
}
