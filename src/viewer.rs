use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};

pub struct ViewerState {
    pub path: PathBuf,
    pub lines: Vec<String>,
    pub scroll: usize,
}

impl ViewerState {
    pub fn open(path: &Path) -> Result<Self> {
        let bytes =
            fs::read(path).with_context(|| format!("No se pudo leer {}", path.display()))?;

        if bytes.iter().take(4096).any(|byte| *byte == 0) {
            bail!("El archivo no parece ser texto legible");
        }

        let content = String::from_utf8_lossy(&bytes);
        let lines = content
            .lines()
            .map(|line| line.replace('\t', "    "))
            .collect::<Vec<_>>();

        Ok(Self {
            path: path.to_path_buf(),
            lines: if lines.is_empty() {
                vec![String::new()]
            } else {
                lines
            },
            scroll: 0,
        })
    }

    pub fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }

    pub fn scroll_down(&mut self) {
        self.scroll = self.scroll.saturating_add(1);
    }
}
