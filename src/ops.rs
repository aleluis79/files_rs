use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

#[derive(Clone, Debug)]
pub enum OverwriteOperation {
    Copy,
    Move,
}

#[derive(Clone, Debug)]
pub struct OverwriteBatchState {
    pub remaining_sources: Vec<PathBuf>,
    pub destination_dir: PathBuf,
    pub processed: usize,
    pub skipped: usize,
    pub current_conflict_source: Option<PathBuf>,
    pub operation: OverwriteOperation,
}

impl OverwriteBatchState {
    pub fn destination_for(&self, source: &Path) -> Option<PathBuf> {
        source.file_name().map(|name| self.destination_dir.join(name))
    }
}

pub fn apply_batch_operation(
    operation: OverwriteOperation,
    source: &Path,
    destination: &Path,
) -> Result<()> {
    match operation {
        OverwriteOperation::Copy => {
            copy_path_recursive(source, destination)?;
        }
        OverwriteOperation::Move => {
            fs::rename(source, destination).with_context(|| {
                format!(
                    "No se pudo mover {} a {}",
                    source.display(),
                    destination.display()
                )
            })?;
        }
    }
    Ok(())
}

pub fn remove_path_recursive(path: &Path) -> Result<()> {
    let metadata =
        fs::symlink_metadata(path).with_context(|| format!("No se pudo leer metadata de {}", path.display()))?;

    if metadata.is_dir() {
        fs::remove_dir_all(path)
            .with_context(|| format!("No se pudo eliminar directorio {}", path.display()))?;
        return Ok(());
    }

    fs::remove_file(path).with_context(|| format!("No se pudo eliminar {}", path.display()))?;
    Ok(())
}

fn copy_path_recursive(source: &Path, destination: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(source)
        .with_context(|| format!("No se pudo leer metadata de {}", source.display()))?;

    if metadata.is_dir() {
        fs::create_dir(destination)
            .with_context(|| format!("No se pudo crear {}", destination.display()))?;
        for entry in
            fs::read_dir(source).with_context(|| format!("No se pudo listar {}", source.display()))?
        {
            let entry = entry?;
            let child_source = entry.path();
            let child_destination = destination.join(entry.file_name());
            copy_path_recursive(&child_source, &child_destination)?;
        }
        return Ok(());
    }

    fs::copy(source, destination).with_context(|| {
        format!(
            "No se pudo copiar {} a {}",
            source.display(),
            destination.display()
        )
    })?;
    Ok(())
}