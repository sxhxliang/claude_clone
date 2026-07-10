use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use sensevoice::{Recognizer, RecognizerConfig};
use tokio::runtime::Runtime;

const MIN_AUDIO_BYTES: u64 = 1024;
const STOP_WAIT: Duration = Duration::from_secs(2);
const STOP_POLL: Duration = Duration::from_millis(100);
const MODELS_DIR_ENV: &str = "CLAUDE_CLONE_SENSEVOICE_MODELS";
const THREADS_ENV: &str = "CLAUDE_CLONE_SENSEVOICE_THREADS";
const DEFAULT_MODELS_RELATIVE_PATH: [&str; 3] = ["Code", "SenseVoice", "models"];

static RECOGNIZER: OnceLock<Mutex<Option<Recognizer>>> = OnceLock::new();

pub(crate) struct VoiceRecorder {
    child: Child,
    audio_path: PathBuf,
}

impl VoiceRecorder {
    pub(crate) fn start() -> Result<Self, String> {
        let ffmpeg = ffmpeg_command()?;
        let audio_path = temp_audio_path();
        let mut command = Command::new(ffmpeg);
        command.args(record_args(&audio_path));
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());

        let child = command
            .spawn()
            .map_err(|err| format!("Failed to start ffmpeg for local voice recording: {err}"))?;

        Ok(Self { child, audio_path })
    }

    pub(crate) fn stop_and_transcribe(mut self) -> Result<String, String> {
        let result = (|| {
            self.stop_recorder()?;
            validate_audio_file(&self.audio_path)?;
            transcribe_audio(&self.audio_path)
        })();
        let _ = fs::remove_file(&self.audio_path);
        result
    }

    fn stop_recorder(&mut self) -> Result<(), String> {
        if let Some(stdin) = self.child.stdin.as_mut() {
            let _ = stdin.write_all(b"q\n");
        }

        let polls = (STOP_WAIT.as_millis() / STOP_POLL.as_millis()).max(1);
        for _ in 0..polls {
            match self.child.try_wait() {
                Ok(Some(status)) if status.success() => return Ok(()),
                Ok(Some(status)) => {
                    return Err(format!("ffmpeg stopped with status {status}"));
                }
                Ok(None) => thread::sleep(STOP_POLL),
                Err(err) => return Err(format!("Failed to check ffmpeg status: {err}")),
            }
        }

        self.child
            .kill()
            .map_err(|err| format!("Failed to stop ffmpeg: {err}"))?;
        self.child
            .wait()
            .map_err(|err| format!("Failed to wait for ffmpeg: {err}"))?;
        Ok(())
    }
}

impl Drop for VoiceRecorder {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        let _ = fs::remove_file(&self.audio_path);
    }
}

fn ffmpeg_command() -> Result<PathBuf, String> {
    env::var_os("CLAUDE_CLONE_FFMPEG")
        .map(PathBuf::from)
        .or_else(|| find_command(&["ffmpeg"]))
        .ok_or_else(|| {
            "Local voice input needs ffmpeg. Install ffmpeg or set CLAUDE_CLONE_FFMPEG.".to_string()
        })
}

fn temp_audio_path() -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    env::temp_dir().join(format!(
        "claude_clone_voice_{}_{}.wav",
        std::process::id(),
        now
    ))
}

#[cfg(target_os = "macos")]
fn record_args(audio_path: &Path) -> Vec<OsString> {
    let mut args = [
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-f",
        "avfoundation",
        "-i",
    ]
    .into_iter()
    .map(OsString::from)
    .collect::<Vec<_>>();
    args.push(audio_input(":0"));
    args.extend(["-ac", "1", "-ar", "16000"].into_iter().map(OsString::from));
    args.push(audio_path.as_os_str().to_os_string());
    args
}

#[cfg(target_os = "windows")]
fn record_args(audio_path: &Path) -> Vec<OsString> {
    let mut args = [
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-f",
        "dshow",
        "-i",
    ]
    .into_iter()
    .map(OsString::from)
    .collect::<Vec<_>>();
    args.push(audio_input("audio=default"));
    args.extend(["-ac", "1", "-ar", "16000"].into_iter().map(OsString::from));
    args.push(audio_path.as_os_str().to_os_string());
    args
}

#[cfg(all(unix, not(target_os = "macos")))]
fn record_args(audio_path: &Path) -> Vec<OsString> {
    let mut args = [
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-f",
        "pulse",
        "-i",
    ]
    .into_iter()
    .map(OsString::from)
    .collect::<Vec<_>>();
    args.push(audio_input("default"));
    args.extend(["-ac", "1", "-ar", "16000"].into_iter().map(OsString::from));
    args.push(audio_path.as_os_str().to_os_string());
    args
}

fn audio_input(default: &str) -> OsString {
    env::var_os("CLAUDE_CLONE_AUDIO_INPUT").unwrap_or_else(|| OsString::from(default))
}

fn validate_audio_file(path: &Path) -> Result<(), String> {
    let metadata = fs::metadata(path)
        .map_err(|err| format!("Voice recording was not written to disk: {err}"))?;
    if metadata.len() < MIN_AUDIO_BYTES {
        return Err(
            "Voice recording is empty. Check microphone permissions and ffmpeg input device."
                .to_string(),
        );
    }
    Ok(())
}

fn transcribe_audio(audio_path: &Path) -> Result<String, String> {
    let text = with_recognizer(|recognizer| recognizer.transcribe_file(audio_path))?;
    let text = clean_transcript_text(&text);
    if text.is_empty() {
        Err("No speech text was recognized.".to_string())
    } else {
        Ok(text)
    }
}

fn with_recognizer<T>(
    f: impl FnOnce(&mut Recognizer) -> sensevoice::Result<T>,
) -> Result<T, String> {
    let recognizer = RECOGNIZER.get_or_init(|| Mutex::new(None));
    let mut recognizer = recognizer
        .lock()
        .map_err(|_| "SenseVoice recognizer lock was poisoned.".to_string())?;
    if recognizer.is_none() {
        *recognizer = Some(load_recognizer()?);
    }

    f(recognizer
        .as_mut()
        .expect("recognizer is initialized above"))
    .map_err(|err| format!("SenseVoice transcription failed: {err}"))
}

fn load_recognizer() -> Result<Recognizer, String> {
    let models_dir = sensevoice_models_dir();
    let mut config = RecognizerConfig::from_models_dir(&models_dir).map_err(|err| {
        format!(
            "Failed to load SenseVoice models from {}: {err}. Set {MODELS_DIR_ENV} to override.",
            models_dir.display()
        )
    })?;
    if let Some(threads) = sensevoice_threads()? {
        config = config.with_threads(threads);
    }
    Recognizer::new(config).map_err(|err| {
        format!(
            "Failed to initialize SenseVoice recognizer from {}: {err}",
            models_dir.display()
        )
    })
}

/// Progress events emitted while downloading the SenseVoice model.
pub(crate) enum DownloadProgress {
    Progress { downloaded: u64, total: Option<u64> },
    Done,
    Error(String),
}

/// Dedicated Tokio runtime for the model download (mirrors `genai_backend`).
fn runtime() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| Runtime::new().expect("failed to start voice download runtime"))
}

/// Download the SenseVoice model from `url` into the models directory. Progress
/// (and the final `Done`/`Error`) is forwarded over a runtime-agnostic channel
/// that a GPUI `cx.spawn_in` can consume.
pub(crate) fn download_model(url: String) -> UnboundedReceiver<DownloadProgress> {
    let (tx, rx) = mpsc::unbounded();
    runtime().spawn(async move {
        match download_model_inner(url, &tx).await {
            Ok(()) => {
                let _ = tx.unbounded_send(DownloadProgress::Done);
            }
            Err(err) => {
                let _ = tx.unbounded_send(DownloadProgress::Error(err));
            }
        }
    });
    rx
}

async fn download_model_inner(
    url: String,
    tx: &UnboundedSender<DownloadProgress>,
) -> Result<(), String> {
    let dir = sensevoice_models_dir();
    fs::create_dir_all(&dir)
        .map_err(|err| format!("Failed to create model directory {}: {err}", dir.display()))?;

    let file_name = model_file_name_from_url(&url);
    let dest = dir.join(&file_name);
    let temp = dir.join(format!("{file_name}.part"));

    let client = reqwest::Client::builder()
        .tls_backend_rustls()
        .build()
        .map_err(|err| format!("Failed to create HTTP client: {err}"))?;
    let mut response = client
        .get(&url)
        .send()
        .await
        .map_err(|err| format!("Failed to request {url}: {err}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Download failed with HTTP status {}",
            response.status()
        ));
    }

    let total = response.content_length();
    let mut file = fs::File::create(&temp)
        .map_err(|err| format!("Failed to create {}: {err}", temp.display()))?;
    let mut downloaded: u64 = 0;
    let _ = tx.unbounded_send(DownloadProgress::Progress { downloaded, total });

    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|err| format!("Download interrupted: {err}"))?
    {
        file.write_all(&chunk)
            .map_err(|err| format!("Failed to write model file: {err}"))?;
        downloaded += chunk.len() as u64;
        let _ = tx.unbounded_send(DownloadProgress::Progress { downloaded, total });
    }
    file.flush()
        .map_err(|err| format!("Failed to flush model file: {err}"))?;
    drop(file);

    // Move the completed file into place. A half-written `.part` is never picked
    // up by `find_gguf`, so an interrupted download leaves the model uninstalled.
    fs::rename(&temp, &dest)
        .map_err(|err| format!("Failed to finalize model file {}: {err}", dest.display()))?;

    reset_recognizer();
    Ok(())
}

/// Pick the on-disk file name for a download URL so `find_gguf` will locate it:
/// the name must end with `.gguf` and contain `sensevoice`.
fn model_file_name_from_url(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let raw = path
        .rsplit('/')
        .find(|segment| !segment.is_empty())
        .unwrap_or_default();

    if !raw.to_ascii_lowercase().ends_with(".gguf") {
        return "sensevoice-model.gguf".to_string();
    }
    if raw.to_ascii_lowercase().contains("sensevoice") {
        raw.to_string()
    } else {
        format!("sensevoice-{raw}")
    }
}

/// Drop the cached recognizer so the next transcription reloads from disk (e.g.
/// after a freshly downloaded model replaces the previous one).
fn reset_recognizer() {
    if let Some(lock) = RECOGNIZER.get()
        && let Ok(mut recognizer) = lock.lock()
    {
        *recognizer = None;
    }
}

/// Whether a SenseVoice model file is present in the models directory.
pub(crate) fn model_installed() -> bool {
    RecognizerConfig::from_models_dir(sensevoice_models_dir()).is_ok()
}

/// The directory the model is downloaded to / loaded from, for display.
pub(crate) fn models_dir_display() -> String {
    sensevoice_models_dir().display().to_string()
}

fn sensevoice_models_dir() -> PathBuf {
    env::var_os(MODELS_DIR_ENV)
        .map(PathBuf::from)
        .map(expand_home_dir)
        .unwrap_or_else(default_models_dir)
}

fn default_models_dir() -> PathBuf {
    let mut path = env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    for part in DEFAULT_MODELS_RELATIVE_PATH {
        path.push(part);
    }
    path
}

fn expand_home_dir(path: PathBuf) -> PathBuf {
    let path_text = path.to_string_lossy().to_string();
    if path_text == "~" {
        return env::var_os("HOME").map(PathBuf::from).unwrap_or(path);
    }
    if let Some(rest) = path_text.strip_prefix("~/")
        && let Some(home) = env::var_os("HOME").map(PathBuf::from)
    {
        return home.join(rest);
    }
    path
}

fn sensevoice_threads() -> Result<Option<i32>, String> {
    let Some(value) = env::var(THREADS_ENV).ok() else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    let threads = value
        .parse::<i32>()
        .map_err(|err| format!("{THREADS_ENV} must be a positive integer: {err}"))?;
    if threads <= 0 {
        return Err(format!("{THREADS_ENV} must be a positive integer"));
    }
    Ok(Some(threads))
}

fn clean_transcript_text(text: &str) -> String {
    let mut without_tags = String::new();
    let mut in_tag = false;
    for ch in text.chars() {
        if in_tag {
            if ch == '>' {
                in_tag = false;
            }
            continue;
        }
        if ch == '<' {
            in_tag = true;
            continue;
        }
        without_tags.push(ch);
    }

    without_tags
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn find_command(names: &[&str]) -> Option<PathBuf> {
    for name in names {
        let candidate = PathBuf::from(name);
        if candidate.components().count() > 1 && is_executable_file(&candidate) {
            return Some(candidate);
        }

        let Some(paths) = env::var_os("PATH") else {
            continue;
        };
        for dir in env::split_paths(&paths) {
            for candidate in executable_candidates(&dir, name) {
                if is_executable_file(&candidate) {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

#[cfg(windows)]
fn executable_candidates(dir: &Path, name: &str) -> Vec<PathBuf> {
    let has_ext = Path::new(name).extension().is_some();
    if has_ext {
        return vec![dir.join(name)];
    }
    env::var_os("PATHEXT")
        .map(|exts| {
            env::split_paths(&exts)
                .map(|ext| dir.join(format!("{name}{}", ext.to_string_lossy())))
                .collect()
        })
        .unwrap_or_else(|| vec![dir.join(format!("{name}.exe")), dir.join(name)])
}

#[cfg(not(windows))]
fn executable_candidates(dir: &Path, name: &str) -> Vec<PathBuf> {
    vec![dir.join(name)]
}

fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::{clean_transcript_text, expand_home_dir, model_file_name_from_url};
    use std::path::PathBuf;

    #[test]
    fn cleans_transcript_lines() {
        assert_eq!(
            clean_transcript_text("\n hello world \n\n from voice \n"),
            "hello world from voice"
        );
    }

    #[test]
    fn strips_sensevoice_tags() {
        assert_eq!(
            clean_transcript_text("<|zh|><|NEUTRAL|><|Speech|> 你好\nworld"),
            "你好 world"
        );
    }

    #[test]
    fn leaves_non_home_paths_unchanged() {
        let path = PathBuf::from("/tmp/models");
        assert_eq!(expand_home_dir(path.clone()), path);
    }

    #[test]
    fn derives_gguf_file_name_from_url() {
        // A well-named gguf is kept as-is.
        assert_eq!(
            model_file_name_from_url("https://example.com/models/SenseVoiceSmall-q8.gguf"),
            "SenseVoiceSmall-q8.gguf"
        );
        // A gguf without "sensevoice" gets the prefix (and query is stripped).
        assert_eq!(
            model_file_name_from_url("https://example.com/small-q8.gguf?download=1"),
            "sensevoice-small-q8.gguf"
        );
        // A non-gguf URL falls back to a canonical, discoverable name.
        assert_eq!(
            model_file_name_from_url("https://example.com/download"),
            "sensevoice-model.gguf"
        );
    }
}
