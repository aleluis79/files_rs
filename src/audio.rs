use std::{
    fs,
    fs::File,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result};
use lofty::{file::TaggedFileExt, prelude::Accessor, probe::Probe};
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Source};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AudioPlaybackStatus {
    Playing,
    Paused,
    Stopped,
}

#[derive(Clone, Debug, Default)]
pub struct SongMetadata {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub genre: Option<String>,
}

pub struct AudioPlayerState {
    pub path: PathBuf,
    pub status: AudioPlaybackStatus,
    playlist: Vec<PathBuf>,
    playlist_labels: Vec<String>,
    current_index: usize,
    loop_enabled: bool,
    current_duration: Option<Duration>,
    metadata: SongMetadata,
    _sink: MixerDeviceSink,
    player: Player,
}

impl AudioPlayerState {
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_playlist_from_tracks(vec![path.to_path_buf()], 0, false)
    }

    pub fn open_playlist_from_directory(selected_path: &Path) -> Result<Self> {
        let parent = selected_path.parent().unwrap_or_else(|| Path::new("."));
        let mut playlist = fs::read_dir(parent)
            .with_context(|| format!("No se pudo leer {}", parent.display()))?
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_file() && is_supported_audio_path(path))
            .collect::<Vec<_>>();

        playlist.sort_by(|a, b| {
            let left = a
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let right = b
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            left.cmp(&right)
        });

        if playlist.is_empty() {
            return Err(anyhow::anyhow!("No hay archivos de audio en la carpeta"));
        }

        let start_index = playlist
            .iter()
            .position(|path| path == selected_path)
            .unwrap_or(0);

        Self::open_playlist_from_tracks(playlist, start_index, true)
    }

    fn open_playlist_from_tracks(
        playlist: Vec<PathBuf>,
        current_index: usize,
        loop_enabled: bool,
    ) -> Result<Self> {
        let playlist_labels = playlist
            .iter()
            .map(|path| display_label_for_track(path))
            .collect::<Vec<_>>();

        let mut state = Self {
            path: playlist[current_index].clone(),
            status: AudioPlaybackStatus::Playing,
            playlist,
            playlist_labels,
            current_index,
            loop_enabled,
            current_duration: None,
            metadata: SongMetadata::default(),
            _sink: DeviceSinkBuilder::open_default_sink()
                .context("No se pudo abrir el dispositivo de audio por defecto")?,
            player: Player::new().0,
        };
        state._sink.log_on_drop(false);
        state.load_current_track()?;
        Ok(state)
    }

    fn build_player() -> Result<(MixerDeviceSink, Player)> {
        let mut sink = DeviceSinkBuilder::open_default_sink()
            .context("No se pudo abrir el dispositivo de audio por defecto")?;
        sink.log_on_drop(false);
        let player = Player::connect_new(&sink.mixer());

        Ok((sink, player))
    }

    fn load_current_track(&mut self) -> Result<()> {
        let path = self.playlist[self.current_index].clone();
        let (sink, player) = Self::build_player()?;

        let file = File::open(&path)
            .with_context(|| format!("No se pudo abrir {}", path.display()))?;
        let source = Decoder::try_from(file)
            .with_context(|| format!("Formato de audio no soportado para {}", path.display()))?;
        self.current_duration = source.total_duration();
        player.append(source);
        self.metadata = read_song_metadata(&path);

        self._sink = sink;
        self.player = player;
        self.path = path;
        self.status = AudioPlaybackStatus::Playing;
        Ok(())
    }

    pub fn toggle_pause(&mut self) -> AudioPlaybackStatus {
        match self.status {
            AudioPlaybackStatus::Playing => {
                self.player.pause();
                self.status = AudioPlaybackStatus::Paused;
            }
            AudioPlaybackStatus::Paused => {
                self.player.play();
                self.status = AudioPlaybackStatus::Playing;
            }
            AudioPlaybackStatus::Stopped => {}
        }
        self.status
    }

    pub fn stop(&mut self) {
        self.player.stop();
        self.status = AudioPlaybackStatus::Stopped;
    }

    pub fn restart_current(&mut self) -> Result<()> {
        self.load_current_track()
    }

    pub fn next_track(&mut self) -> Result<bool> {
        if self.current_index + 1 >= self.playlist.len() {
            if self.loop_enabled && self.playlist.len() > 1 {
                self.current_index = 0;
                self.load_current_track()?;
                return Ok(true);
            }
            return Ok(false);
        }

        self.current_index += 1;
        self.load_current_track()?;
        Ok(true)
    }

    pub fn previous_track(&mut self) -> Result<bool> {
        if self.current_index == 0 {
            if self.loop_enabled && self.playlist.len() > 1 {
                self.current_index = self.playlist.len().saturating_sub(1);
                self.load_current_track()?;
                return Ok(true);
            }
            return Ok(false);
        }

        self.current_index -= 1;
        self.load_current_track()?;
        Ok(true)
    }

    pub fn advance_finished_track(&mut self) -> Result<bool> {
        if self.current_index + 1 < self.playlist.len() {
            self.current_index += 1;
            self.load_current_track()?;
            return Ok(true);
        }

        if self.loop_enabled && self.playlist.len() > 1 {
            self.current_index = 0;
            self.load_current_track()?;
            return Ok(true);
        }

        self.status = AudioPlaybackStatus::Stopped;
        Ok(false)
    }

    pub fn seek_by_seconds(&mut self, delta_seconds: i64) -> Result<()> {
        let current = self.position().as_secs() as i64;
        let target = (current + delta_seconds).max(0) as u64;
        self.player
            .try_seek(Duration::from_secs(target))
            .map_err(|error| anyhow::anyhow!("No se pudo mover la reproduccion: {error}"))
    }

    pub fn position(&self) -> Duration {
        self.player.get_pos()
    }

    pub fn total_duration(&self) -> Option<Duration> {
        self.current_duration
    }

    pub fn metadata(&self) -> &SongMetadata {
        &self.metadata
    }

    pub fn should_advance_track(&self) -> bool {
        if self.status != AudioPlaybackStatus::Playing {
            return false;
        }

        let Some(duration) = self.current_duration else {
            return false;
        };

        let threshold = duration.saturating_sub(Duration::from_millis(200));
        self.position() >= threshold
    }

    pub fn current_track_name(&self) -> String {
        self.path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("archivo")
            .to_string()
    }

    pub fn current_track_number(&self) -> usize {
        self.current_index.saturating_add(1)
    }

    pub fn total_tracks(&self) -> usize {
        self.playlist.len()
    }

    pub fn current_track_index(&self) -> usize {
        self.current_index
    }

    pub fn playlist_track_names(&self) -> Vec<String> {
        self.playlist_labels.clone()
    }

    pub fn toggle_loop(&mut self) -> bool {
        self.loop_enabled = !self.loop_enabled;
        self.loop_enabled
    }

    pub fn loop_enabled(&self) -> bool {
        self.loop_enabled
    }

    pub fn status_label(&self) -> &'static str {
        match self.status {
            AudioPlaybackStatus::Playing => "Reproduciendo",
            AudioPlaybackStatus::Paused => "Pausado",
            AudioPlaybackStatus::Stopped => "Detenido",
        }
    }
}

fn read_song_metadata(path: &Path) -> SongMetadata {
    let tagged_file = match Probe::open(path).and_then(|probe| probe.read()) {
        Ok(file) => file,
        Err(_) => return SongMetadata::default(),
    };

    let Some(tag) = tagged_file.primary_tag().or_else(|| tagged_file.first_tag()) else {
        return SongMetadata::default();
    };

    SongMetadata {
        title: tag.title().map(|value| value.to_string()),
        artist: tag.artist().map(|value| value.to_string()),
        album: tag.album().map(|value| value.to_string()),
        genre: tag.genre().map(|value| value.to_string()),
    }
}

fn display_label_for_track(path: &Path) -> String {
    let metadata = read_song_metadata(path);
    if let Some(title) = metadata.title {
        if !title.trim().is_empty() {
            return title;
        }
    }

    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("archivo")
        .to_string()
}

pub fn is_supported_audio_path(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };

    matches!(
        extension.to_ascii_lowercase().as_str(),
        "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" | "opus"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_supported_audio_extensions() {
        assert!(is_supported_audio_path(Path::new("song.mp3")));
        assert!(is_supported_audio_path(Path::new("track.WAV")));
        assert!(is_supported_audio_path(Path::new("audio.flac")));
        assert!(!is_supported_audio_path(Path::new("notes.txt")));
    }
}
