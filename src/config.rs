use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub bind_address: String,
    pub api_key: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseConfig {
    pub backend: DbBackend,
    #[serde(default)]
    pub sqlite_path: String,
    #[serde(default)]
    pub postgres_url: String,
    #[serde(default = "default_max_conns")]
    pub max_connections: u32,
}

fn default_max_conns() -> u32 {
    10
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DbBackend {
    Sqlite,
    Postgres,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config file at {}", path.display()))?;
        let cfg: Config = toml::from_str(&text).context("parsing config TOML")?;
        cfg.validate()?;
        Ok(cfg)
    }

    fn validate(&self) -> Result<()> {
        if self.server.api_key.trim().is_empty()
            || self.server.api_key == "CHANGE_ME_TO_A_LONG_RANDOM_STRING"
        {
            anyhow::bail!("server.api_key must be set to a non-default value");
        }
        match self.database.backend {
            DbBackend::Sqlite => {
                if self.database.sqlite_path.trim().is_empty() {
                    anyhow::bail!("database.sqlite_path must be set when backend = sqlite");
                }
            }
            DbBackend::Postgres => {
                if self.database.postgres_url.trim().is_empty() {
                    anyhow::bail!("database.postgres_url must be set when backend = postgres");
                }
            }
        }
        Ok(())
    }
}
