use crate::{load_config, GatewayConfig};
use arc_swap::ArcSwap;
use notify::{Event, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::Duration;

#[derive(Clone)]
pub struct ConfigWatcher {
    pub current_config: Arc<ArcSwap<GatewayConfig>>,
    path: PathBuf,
}

impl ConfigWatcher {
    pub fn new<P: AsRef<Path>>(path: P, initial_config: GatewayConfig) -> Self {
        Self {
            current_config: Arc::new(ArcSwap::from_pointee(initial_config)),
            path: path.as_ref().to_path_buf(),
        }
    }

    pub fn start_watching<F>(&self, on_reload: F)
    where 
        F: Fn(Arc<GatewayConfig>) + Send + Sync + 'static,
    {
        // Channel to receive raw file modification events
        let (tx, mut rx) = mpsc::channel::<()>(100);
        
        let path_clone = self.path.clone();
        let config_swap = self.current_config.clone();

        // We run a background task that owns the watcher so it isn't dropped
        tokio::spawn(async move {
            let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
                if let Ok(event) = res {
                    if event.kind.is_modify() {
                        // ignore errors if receiver is dropping/full
                        let _ = tx.blocking_send(());
                    }
                }
            }).expect("Failed to create notify watcher");

            let parent = path_clone.parent().unwrap_or_else(|| Path::new("."));
            watcher.watch(parent, RecursiveMode::NonRecursive).expect("Failed to watch config directory");

            // Debouncing loop
            while rx.recv().await.is_some() {
                // Wait 500ms for multiple filesystem events to settle
                tokio::time::sleep(Duration::from_millis(500)).await;
                // Drain any additional events queued during the 500ms window
                while rx.try_recv().is_ok() {}

                tracing::info!("Config change detected on disk! Validating new configuration...");
                
                // Load, Parse and Validate the configuration
                match load_config(path_clone.to_str().unwrap()) {
                    Ok(new_config) => {
                        let arc_cfg = Arc::new(new_config);
                        // Atomically swap the router / global config pointer
                        config_swap.store(arc_cfg.clone());
                        tracing::info!("✅ Config hot-reloaded and swapped successfully!");
                        
                        // Fire user callback (e.g to rebuild router or send telegram alert)
                        on_reload(arc_cfg);
                    }
                    Err(e) => {
                        // Keep the old configuration actively routing traffic cleanly
                        tracing::error!("❌ Invalid configuration during hot-reload! Kept old config active. Parser Error: {}", e);
                    }
                }
            }
        });
    }
}
