use std::path::Path;
use thiserror::Error;
use super::model::GatewayConfig;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Failed to read config file: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Failed to parse TOML config: {0}")]
    Parse(#[from] toml::de::Error),

    #[error("Configuration validation failed: {0}")]
    Validation(String),
}

pub fn load_config<P: AsRef<Path>>(path: P) -> Result<GatewayConfig, ConfigError> {
    let content = std::fs::read_to_string(path.as_ref())?;
    let config: GatewayConfig = toml::from_str(&content)?;
    
    validate_config(&config)?;
    
    Ok(config)
}

pub fn validate_config(config: &GatewayConfig) -> Result<(), ConfigError> {
    // Basic validation logic
    if config.server.listen.is_empty() {
        return Err(ConfigError::Validation("server.listen cannot be empty".to_string()));
    }
    
    if config.server.admin_listen.is_empty() {
        return Err(ConfigError::Validation("server.admin_listen cannot be empty".to_string()));
    }
    
    Ok(())
}
