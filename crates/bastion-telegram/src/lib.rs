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

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::AUTHORIZATION,
        reqwest::header::HeaderValue::from_static("Bearer bastion-admin-secret"),
    );
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .default_headers(headers)
        .build()
        .unwrap();

    Command::repl(bot, move |bot: Bot, msg: Message, cmd: Command| {
        let ctx = ctx.clone();
        let client = client.clone();
        async move {
            let chat_id = msg.chat.id.0;
            if !ctx.admin_chat_ids.is_empty() && !ctx.admin_chat_ids.contains(&chat_id) {
                let _ = bot.send_message(msg.chat.id, "⛔ Accès non autorisé.").await;
                return respond(());
            }

            use teloxide::types::{KeyboardMarkup, KeyboardButton};
            let keyboard = KeyboardMarkup::new(vec![
                vec![KeyboardButton::new("/status"), KeyboardButton::new("/stats"), KeyboardButton::new("/toproutes")],
                vec![KeyboardButton::new("/backends"), KeyboardButton::new("/health"), KeyboardButton::new("/reload")],
                vec![KeyboardButton::new("/cachestats"), KeyboardButton::new("/cacheclear"), KeyboardButton::new("/help")],
            ]).resize_keyboard();

            match cmd {
                Command::Help => {
                    bot.send_message(msg.chat.id, Command::descriptions().to_string())
                        .reply_markup(keyboard).await?;
                }
                Command::Status => {
                    let reqs = ctx.metrics.total_requests.load(std::sync::atomic::Ordering::Relaxed);
                    let errs = ctx.metrics.total_errors.load(std::sync::atomic::Ordering::Relaxed);
                    let success = reqs.saturating_sub(errs);
                    let msg_text = format!("🛡️ *BASTION STATUS* 🛡️\n\n📈 *Requêtes Totales*: {}\n🟢 *Succès*: {}\n🔴 *Erreurs*: {}", reqs, success, errs);
                    bot.send_message(msg.chat.id, msg_text).parse_mode(teloxide::types::ParseMode::MarkdownV2).reply_markup(keyboard).await?;
                }
                Command::Stats => {
                    let (p50, p95, p99) = {
                        let hist = ctx.metrics.global_latency.lock();
                        (hist.value_at_quantile(0.50), hist.value_at_quantile(0.95), hist.value_at_quantile(0.99))
                    };
                    let reqs = ctx.metrics.total_requests.load(std::sync::atomic::Ordering::Relaxed);
                    let msg_text = format!("📊 *BASTION STATS*\n\n📈 *Requests*: {}\n⏱️ *P50 Latency*: {} µs\n⏱️ *P95 Latency*: {} µs\n⏱️ *P99 Latency*: {} µs", reqs, p50, p95, p99);
                    bot.send_message(msg.chat.id, msg_text).parse_mode(teloxide::types::ParseMode::MarkdownV2).reply_markup(keyboard).await?;
                }
                Command::Backends => {
                    let mut lines = Vec::new();
                    for entry in ctx.metrics.backends.iter() {
                        let bk = entry.key().replace(".", "\\.").replace("-", "\\-");
                        let reqs = entry.value().total_requests.load(std::sync::atomic::Ordering::Relaxed);
                        let errs = entry.value().total_errors.load(std::sync::atomic::Ordering::Relaxed);
                        lines.push(format!("🔌 `{}`: {} reqs \\({} errs\\)", bk, reqs, errs));
                    }
                    if lines.is_empty() { lines.push("Aucun backend actif\\.".to_string()); }
                    let msg_text = format!("*BACKENDS*\n{}", lines.join("\n"));
                    bot.send_message(msg.chat.id, msg_text).parse_mode(teloxide::types::ParseMode::MarkdownV2).reply_markup(keyboard).await?;
                }
                Command::Health(name) => {
                    if name.is_empty() {
                        match client.get("http://127.0.0.1:8081/admin/health").send().await {
                            Ok(res) => {
                                let body = res.text().await.unwrap_or_default().replace(".", "\\.").replace("-", "\\-");
                                bot.send_message(msg.chat.id, format!("🩺 *SANTÉ GLOBALE*\n\n```json\n{}\n```", body)).parse_mode(teloxide::types::ParseMode::MarkdownV2).reply_markup(keyboard).await?;
                            }
                            Err(_) => { bot.send_message(msg.chat.id, "🔴 Impossible de joindre l'Admin API.").reply_markup(keyboard).await?; }
                        }
                    } else {
                        bot.send_message(msg.chat.id, format!("Recherche de la santé pour {}... (non implémenté isolément, utilisez /health pour tout voir)", name)).reply_markup(keyboard).await?;
                    }
                }
                Command::TopRoutes => {
                    let mut routes: Vec<_> = ctx.metrics.routes.iter()
                        .map(|e| (e.key().clone(), e.value().total_requests.load(std::sync::atomic::Ordering::Relaxed)))
                        .collect();
                    routes.sort_by(|a, b| b.1.cmp(&a.1));
                    let top: Vec<_> = routes.into_iter().take(10).map(|(r, v)| format!("🛤️ `{}`: {}", r.replace(".", "\\.").replace("-", "\\-"), v)).collect();
                    let out = if top.is_empty() { "Aucune route.".to_string() } else { top.join("\n") };
                    bot.send_message(msg.chat.id, format!("🏆 *TOP ROUTES*\n\n{}", out)).parse_mode(teloxide::types::ParseMode::MarkdownV2).reply_markup(keyboard).await?;
                }
                Command::Toggle(backend) => {
                    if backend.is_empty() {
                        bot.send_message(msg.chat.id, "⚠️ Précisez un backend. Ex: /toggle http://127.0.0.1:8001").reply_markup(keyboard).await?;
                    } else {
                        let payload = serde_json::json!({ "url": backend, "drain": true });
                        match client.put("http://127.0.0.1:8081/admin/upstreams/backend/health").json(&payload).send().await {
                            Ok(_) => { bot.send_message(msg.chat.id, format!("🔄 Drain du backend {} demandé via API.", backend)).reply_markup(keyboard).await?; }
                            Err(e) => { bot.send_message(msg.chat.id, format!("🔴 Échec du toggle: {}", e)).reply_markup(keyboard).await?; }
                        }
                    }
                }
                Command::Reload => {
                    match client.post("http://127.0.0.1:8081/admin/config/reload").send().await {
                        Ok(_) => { bot.send_message(msg.chat.id, "⚡ Configuration rechargée via hot-reload !").reply_markup(keyboard).await?; }
                        Err(e) => { bot.send_message(msg.chat.id, format!("🔴 Échec du reload: {}", e)).reply_markup(keyboard).await?; }
                    }
                }
                Command::CacheStats => {
                    bot.send_message(msg.chat.id, "🧠 Le cache Metrics n'est pas encore exposé via l'Admin API.").reply_markup(keyboard).await?;
                }
                Command::CacheClear => {
                    bot.send_message(msg.chat.id, "🗑️ Le CacheClear global nécessite un endpoint Admin.").reply_markup(keyboard).await?;
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
