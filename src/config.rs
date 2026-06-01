use std::{fs, path::PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct SavedConnection {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
}

fn default_theme() -> String {
    "dark".to_string()
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AppConfigFile {
    #[serde(default)]
    pub connections: Vec<SavedConnection>,
    #[serde(default = "default_theme")]
    pub theme_name: String,
}

#[derive(Clone, Debug)]
pub struct ConfigStore {
    path: PathBuf,
}

impl ConfigStore {
    pub fn new() -> Result<Self> {
        let base = dirs::config_dir().context("No se pudo resolver directorio de configuracion")?;
        let path = base.join("files-rs").join("config.toml");
        Ok(Self { path })
    }

    pub fn load_config(&self) -> Result<AppConfigFile> {
        if !self.path.exists() {
            // Create default config file if it doesn't exist
            if let Some(parent) = self.path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("No se pudo crear {}", parent.display()))?;
            }

            let mut default_config = AppConfigFile::default();
            default_config.theme_name = "dark".to_string();

            let serialized = toml::to_string_pretty(&default_config)
                .context("No se pudo serializar configuracion por defecto")?;
            fs::write(&self.path, serialized)
                .with_context(|| format!("No se pudo escribir {}", self.path.display()))?;

            return Ok(default_config);
        }

        let content = fs::read_to_string(&self.path)
            .with_context(|| format!("No se pudo leer {}", self.path.display()))?;

        let parsed: AppConfigFile = toml::from_str(&content)
            .with_context(|| format!("No se pudo parsear {}", self.path.display()))?;
        Ok(parsed)
    }

    pub fn save_connections(&self, connections: &[SavedConnection]) -> Result<()> {
        // Load existing config to preserve theme_name
        let existing = self.load_config().unwrap_or_default();
        
        let data = AppConfigFile {
            connections: connections.to_vec(),
            theme_name: existing.theme_name,
        };
        let serialized = toml::to_string_pretty(&data).context("No se pudo serializar configuracion")?;

        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("No se pudo crear {}", parent.display()))?;
        }

        fs::write(&self.path, serialized)
            .with_context(|| format!("No se pudo escribir {}", self.path.display()))?;
        Ok(())
    }

    pub fn config_path(&self) -> &PathBuf {
        &self.path
    }

    pub fn themes_dir(&self) -> PathBuf {
        self.path
            .parent()
            .map(|parent| parent.join("themes"))
            .unwrap_or_else(|| PathBuf::from("themes"))
    }
}
