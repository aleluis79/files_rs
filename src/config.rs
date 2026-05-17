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

#[derive(Debug, Default, Serialize, Deserialize)]
struct AppConfigFile {
    #[serde(default)]
    connections: Vec<SavedConnection>,
}

#[derive(Clone, Debug)]
pub struct ConfigStore {
    path: PathBuf,
}

impl ConfigStore {
    pub fn new() -> Result<Self> {
        let base = dirs::config_dir().context("No se pudo resolver directorio de configuracion")?;
        let path = base.join("files-rs").join("connections.toml");
        Ok(Self { path })
    }

    pub fn load_connections(&self) -> Result<Vec<SavedConnection>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(&self.path)
            .with_context(|| format!("No se pudo leer {}", self.path.display()))?;

        let parsed: AppConfigFile = toml::from_str(&content)
            .with_context(|| format!("No se pudo parsear {}", self.path.display()))?;
        Ok(parsed.connections)
    }

    pub fn save_connections(&self, connections: &[SavedConnection]) -> Result<()> {
        let data = AppConfigFile {
            connections: connections.to_vec(),
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
}
