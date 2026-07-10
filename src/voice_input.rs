use std::env;
use std::fs;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use sensevoice::{Recognizer, RecognizerConfig};
use tokio::runtime::Runtime;

const OUTPUT_SAMPLE_RATE: f64 = 16_000.0;
const OUTPUT_SAMPLE_RATE_USIZE: usize = 16_000;
const PRE_ROLL_SAMPLES: usize = OUTPUT_SAMPLE_RATE_USIZE / 4;
const MIN_PREVIEW_SAMPLES: usize = OUTPUT_SAMPLE_RATE_USIZE / 2;
const PREVIEW_INTERVAL: Duration = Duration::from_millis(900);
const SPEECH_RMS_THRESHOLD: f32 = 0.006;
const SPEECH_PEAK_THRESHOLD: f32 = 0.02;
const AUTO_FLUSH_SILENCE: usize = OUTPUT_SAMPLE_RATE_USIZE;
const MAX_UNFLUSHED_AUDIO: usize = OUTPUT_SAMPLE_RATE_USIZE * 10;
const MODELS_DIR_ENV: &str = "CLAUDE_CLONE_SENSEVOICE_MODELS";
const THREADS_ENV: &str = "CLAUDE_CLONE_SENSEVOICE_THREADS";
const AUDIO_INPUT_ENV: &str = "CLAUDE_CLONE_AUDIO_INPUT";
const DEFAULT_MODELS_RELATIVE_PATH: [&str; 3] = ["Code", "SenseVoice", "models"];

static RECOGNIZER: OnceLock<Mutex<Option<Recognizer>>> = OnceLock::new();

#[derive(Debug)]
pub(crate) enum VoiceEvent {
    Ready { device_name: String },
    Preview(String),
    Commit(String),
    Error(String),
    Finished,
}

pub(crate) struct VoiceRecorder {
    stop_tx: mpsc::Sender<()>,
    events: Option<UnboundedReceiver<VoiceEvent>>,
}

impl VoiceRecorder {
    pub(crate) fn start(configured_device: Option<String>) -> Result<Self, String> {
        let (event_tx, event_rx) = unbounded();
        let (stop_tx, stop_rx) = mpsc::channel();

        thread::Builder::new()
            .name("claude-clone-voice-input".to_string())
            .spawn(move || {
                if let Err(err) = run_realtime_session(stop_rx, event_tx.clone(), configured_device)
                {
                    let _ = event_tx.unbounded_send(VoiceEvent::Error(err));
                }
            })
            .map_err(|err| format!("Failed to start local voice input thread: {err}"))?;

        Ok(Self {
            stop_tx,
            events: Some(event_rx),
        })
    }

    pub(crate) fn take_events(&mut self) -> Option<UnboundedReceiver<VoiceEvent>> {
        self.events.take()
    }

    pub(crate) fn stop(&self) {
        let _ = self.stop_tx.send(());
    }
}

impl Drop for VoiceRecorder {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(());
    }
}

fn run_realtime_session(
    stop_rx: mpsc::Receiver<()>,
    event_tx: UnboundedSender<VoiceEvent>,
    configured_device: Option<String>,
) -> Result<(), String> {
    let mut recognizer = acquire_recognizer()?;
    let recognizer = recognizer
        .as_mut()
        .expect("recognizer is initialized by acquire_recognizer");
    if stop_rx.try_recv().is_ok() {
        let _ = event_tx.unbounded_send(VoiceEvent::Finished);
        return Ok(());
    }

    let host = cpal::default_host();
    let environment_device = env::var(AUDIO_INPUT_ENV)
        .ok()
        .filter(|device| !device.trim().is_empty());
    let selected_device = environment_device
        .as_deref()
        .or(configured_device.as_deref());
    let device = select_input_device(&host, selected_device)?;
    let device_name = device
        .name()
        .unwrap_or_else(|_| "Default microphone".to_string());
    let supported_config = device
        .default_input_config()
        .map_err(|err| format!("Failed to read default microphone config: {err}"))?;
    let sample_format = supported_config.sample_format();
    let config: cpal::StreamConfig = supported_config.into();
    let input_sample_rate = config.sample_rate.0;
    let channels = usize::from(config.channels);

    let (audio_tx, audio_rx) = mpsc::channel();
    let stream = build_input_stream(
        &device,
        &config,
        sample_format,
        input_sample_rate,
        channels,
        audio_tx,
    )?;
    stream
        .play()
        .map_err(|err| format!("Failed to start microphone stream: {err}"))?;
    let _ = event_tx.unbounded_send(VoiceEvent::Ready { device_name });

    let mut endpoint = EndpointState::new();
    let mut current_audio = Vec::new();
    let mut pre_roll = Vec::new();
    let mut last_preview = Instant::now();
    let mut transcript = TranscriptState::default();

    loop {
        if stop_rx.try_recv().is_ok() {
            break;
        }

        match audio_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(samples) => {
                let level = Level::from_samples(&samples);
                let sounds_like_speech = level.sounds_like_speech();
                if sounds_like_speech && current_audio.is_empty() {
                    current_audio.extend_from_slice(&pre_roll);
                }
                if sounds_like_speech || endpoint.heard_speech() {
                    current_audio.extend_from_slice(&samples);
                } else {
                    append_with_limit(&mut pre_roll, &samples, PRE_ROLL_SAMPLES);
                }

                if endpoint.observe(samples.len(), level) {
                    commit_current_utterance(
                        recognizer,
                        &mut transcript,
                        &event_tx,
                        &current_audio,
                    )?;
                    endpoint.reset();
                    current_audio.clear();
                    pre_roll.clear();
                    last_preview = Instant::now();
                } else if endpoint.heard_speech()
                    && current_audio.len() >= MIN_PREVIEW_SAMPLES
                    && last_preview.elapsed() >= PREVIEW_INTERVAL
                {
                    let text = recognizer
                        .transcribe_pcm_16k(&current_audio)
                        .map_err(|err| format!("SenseVoice preview failed: {err}"))?;
                    transcript.preview(&text, &event_tx);
                    last_preview = Instant::now();
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    if !current_audio.is_empty() {
        commit_current_utterance(recognizer, &mut transcript, &event_tx, &current_audio)?;
    }

    let _ = event_tx.unbounded_send(VoiceEvent::Finished);
    Ok(())
}

fn commit_current_utterance(
    recognizer: &mut Recognizer,
    transcript: &mut TranscriptState,
    event_tx: &UnboundedSender<VoiceEvent>,
    audio: &[f32],
) -> Result<(), String> {
    if audio.len() < MIN_PREVIEW_SAMPLES {
        transcript.clear_preview(event_tx);
        return Ok(());
    }

    let text = recognizer
        .transcribe_pcm_16k(audio)
        .map_err(|err| format!("SenseVoice transcription failed: {err}"))?;
    transcript.commit(&text, event_tx);
    Ok(())
}

#[derive(Debug, Default)]
struct TranscriptState {
    preview: String,
}

impl TranscriptState {
    fn preview(&mut self, text: &str, event_tx: &UnboundedSender<VoiceEvent>) {
        let text = clean_transcript_text(text);
        if text.is_empty() || text == self.preview {
            return;
        }
        self.preview = text.clone();
        let _ = event_tx.unbounded_send(VoiceEvent::Preview(text));
    }

    fn clear_preview(&mut self, event_tx: &UnboundedSender<VoiceEvent>) {
        if self.preview.is_empty() {
            return;
        }
        self.preview.clear();
        let _ = event_tx.unbounded_send(VoiceEvent::Preview(String::new()));
    }

    fn commit(&mut self, text: &str, event_tx: &UnboundedSender<VoiceEvent>) {
        let text = clean_transcript_text(text);
        self.preview.clear();
        if text.is_empty() {
            let _ = event_tx.unbounded_send(VoiceEvent::Preview(String::new()));
        } else {
            let _ = event_tx.unbounded_send(VoiceEvent::Commit(text));
        }
    }
}

fn load_recognizer() -> Result<Recognizer, String> {
    let models_dir = sensevoice_models_dir();
    let mut config = RecognizerConfig::from_models_dir(&models_dir).map_err(|err| {
        format!(
            "Failed to load SenseVoice models from {}: {err}. Set {MODELS_DIR_ENV} to override.",
            models_dir.display()
        )
    })?;
    config.vad_model = None;
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

fn acquire_recognizer() -> Result<MutexGuard<'static, Option<Recognizer>>, String> {
    let recognizer = RECOGNIZER.get_or_init(|| Mutex::new(None));
    let mut recognizer = recognizer
        .lock()
        .map_err(|_| "SenseVoice recognizer lock was poisoned.".to_string())?;
    if recognizer.is_none() {
        *recognizer = Some(load_recognizer()?);
    }
    Ok(recognizer)
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
    let (tx, rx) = unbounded();
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

fn select_input_device(host: &cpal::Host, selector: Option<&str>) -> Result<cpal::Device, String> {
    if let Some(selector) = selector.filter(|selector| !selector.trim().is_empty()) {
        let devices = input_devices(host)?;
        if let Ok(index) = selector.parse::<usize>() {
            return devices
                .into_iter()
                .nth(index)
                .ok_or_else(|| format!("No input device at index {index}"));
        }

        let selector = selector.to_ascii_lowercase();
        return devices
            .into_iter()
            .find(|device| {
                device
                    .name()
                    .map(|name| name.to_ascii_lowercase().contains(&selector))
                    .unwrap_or(false)
            })
            .ok_or_else(|| format!("No input device matching {selector:?}"));
    }

    host.default_input_device()
        .ok_or_else(|| "No default input device available.".to_string())
}

fn input_devices(host: &cpal::Host) -> Result<Vec<cpal::Device>, String> {
    host.input_devices()
        .map(|devices| devices.collect())
        .map_err(|err| format!("Failed to enumerate input devices: {err}"))
}

pub(crate) fn input_device_names() -> Result<Vec<String>, String> {
    let host = cpal::default_host();
    let mut names = input_devices(&host)?
        .into_iter()
        .filter_map(|device| device.name().ok())
        .collect::<Vec<_>>();
    names.sort_by_key(|name| name.to_ascii_lowercase());
    names.dedup();
    if names.is_empty() {
        return Err("No audio input devices are available.".to_string());
    }
    Ok(names)
}

#[derive(Clone, Copy, Debug, Default)]
struct Level {
    rms: f32,
    peak: f32,
}

impl Level {
    fn from_samples(samples: &[f32]) -> Self {
        if samples.is_empty() {
            return Self::default();
        }

        let mut sum_squares = 0.0_f64;
        let mut peak = 0.0_f32;
        for sample in samples {
            let sample = sample.abs();
            peak = peak.max(sample);
            sum_squares += f64::from(sample * sample);
        }
        Self {
            rms: (sum_squares / samples.len() as f64).sqrt() as f32,
            peak,
        }
    }

    fn sounds_like_speech(self) -> bool {
        self.rms >= SPEECH_RMS_THRESHOLD || self.peak >= SPEECH_PEAK_THRESHOLD
    }
}

#[derive(Debug, Default)]
struct EndpointState {
    heard_speech: bool,
    silence_samples: usize,
    unflushed_samples: usize,
}

impl EndpointState {
    fn new() -> Self {
        Self::default()
    }

    fn observe(&mut self, sample_count: usize, level: Level) -> bool {
        if level.sounds_like_speech() {
            self.heard_speech = true;
            self.silence_samples = 0;
        } else if self.heard_speech {
            self.silence_samples += sample_count;
        }

        if self.heard_speech {
            self.unflushed_samples += sample_count;
        }

        self.heard_speech
            && (self.silence_samples >= AUTO_FLUSH_SILENCE
                || self.unflushed_samples >= MAX_UNFLUSHED_AUDIO)
    }

    fn reset(&mut self) {
        *self = Self::default();
    }

    fn heard_speech(&self) -> bool {
        self.heard_speech
    }
}

fn build_input_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    input_sample_rate: u32,
    channels: usize,
    tx: mpsc::Sender<Vec<f32>>,
) -> Result<cpal::Stream, String> {
    match sample_format {
        cpal::SampleFormat::F32 => build_typed_input_stream::<f32, _>(
            device,
            config,
            input_sample_rate,
            channels,
            tx,
            |sample| sample,
        ),
        cpal::SampleFormat::F64 => build_typed_input_stream::<f64, _>(
            device,
            config,
            input_sample_rate,
            channels,
            tx,
            |sample| sample.clamp(-1.0, 1.0) as f32,
        ),
        cpal::SampleFormat::I8 => build_typed_input_stream::<i8, _>(
            device,
            config,
            input_sample_rate,
            channels,
            tx,
            |sample| (f32::from(sample) / f32::from(i8::MAX)).clamp(-1.0, 1.0),
        ),
        cpal::SampleFormat::I16 => build_typed_input_stream::<i16, _>(
            device,
            config,
            input_sample_rate,
            channels,
            tx,
            |sample| (f32::from(sample) / f32::from(i16::MAX)).clamp(-1.0, 1.0),
        ),
        cpal::SampleFormat::I32 => build_typed_input_stream::<i32, _>(
            device,
            config,
            input_sample_rate,
            channels,
            tx,
            |sample| (sample as f32 / i32::MAX as f32).clamp(-1.0, 1.0),
        ),
        cpal::SampleFormat::I64 => build_typed_input_stream::<i64, _>(
            device,
            config,
            input_sample_rate,
            channels,
            tx,
            |sample| (sample as f64 / i64::MAX as f64).clamp(-1.0, 1.0) as f32,
        ),
        cpal::SampleFormat::U8 => build_typed_input_stream::<u8, _>(
            device,
            config,
            input_sample_rate,
            channels,
            tx,
            |sample| (f32::from(sample) - 128.0) / 128.0,
        ),
        cpal::SampleFormat::U16 => build_typed_input_stream::<u16, _>(
            device,
            config,
            input_sample_rate,
            channels,
            tx,
            |sample| (f32::from(sample) - 32_768.0) / 32_768.0,
        ),
        cpal::SampleFormat::U32 => build_typed_input_stream::<u32, _>(
            device,
            config,
            input_sample_rate,
            channels,
            tx,
            |sample| ((sample as f64 - 2_147_483_648.0) / 2_147_483_648.0) as f32,
        ),
        cpal::SampleFormat::U64 => build_typed_input_stream::<u64, _>(
            device,
            config,
            input_sample_rate,
            channels,
            tx,
            |sample| {
                ((sample as f64 - 9_223_372_036_854_775_808.0) / 9_223_372_036_854_775_808.0) as f32
            },
        ),
        other => Err(format!("Unsupported input sample format: {other:?}")),
    }
}

fn build_typed_input_stream<T, F>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    input_sample_rate: u32,
    channels: usize,
    tx: mpsc::Sender<Vec<f32>>,
    convert: F,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + Copy,
    F: Fn(T) -> f32 + Send + Copy + 'static,
{
    let mut resampler = DownmixResampler::new(f64::from(input_sample_rate), channels);
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                let samples = resampler.push_interleaved(data, convert);
                if !samples.is_empty() {
                    let _ = tx.send(samples);
                }
            },
            |error| eprintln!("audio input stream error: {error}"),
            None,
        )
        .map_err(|err| format!("Failed to build microphone stream: {err}"))
}

struct DownmixResampler {
    input_sample_rate: f64,
    channels: usize,
    mono: Vec<f32>,
    next_input_pos: f64,
}

impl DownmixResampler {
    fn new(input_sample_rate: f64, channels: usize) -> Self {
        Self {
            input_sample_rate,
            channels: channels.max(1),
            mono: Vec::new(),
            next_input_pos: 0.0,
        }
    }

    fn push_interleaved<T>(&mut self, input: &[T], convert: impl Fn(T) -> f32) -> Vec<f32>
    where
        T: Copy,
    {
        for frame in input.chunks_exact(self.channels) {
            let sum = frame
                .iter()
                .map(|sample| convert(*sample))
                .fold(0.0_f32, |acc, sample| acc + sample);
            self.mono.push(sum / self.channels as f32);
        }

        let mut output = Vec::new();
        let step = self.input_sample_rate / OUTPUT_SAMPLE_RATE;
        while self.next_input_pos + 1.0 < self.mono.len() as f64 {
            let index = self.next_input_pos.floor() as usize;
            let frac = (self.next_input_pos - index as f64) as f32;
            let current = self.mono[index];
            let next = self.mono[index + 1];
            output.push(current + (next - current) * frac);
            self.next_input_pos += step;
        }

        let len = self.mono.len();
        let consumed = if self.next_input_pos >= len as f64 {
            len
        } else {
            self.next_input_pos.floor() as usize
        };
        if consumed > 0 {
            self.mono.drain(..consumed);
            self.next_input_pos -= consumed as f64;
        }

        output
    }
}

fn append_with_limit(buffer: &mut Vec<f32>, samples: &[f32], max_len: usize) {
    buffer.extend_from_slice(samples);
    if buffer.len() > max_len {
        let extra = buffer.len() - max_len;
        buffer.drain(..extra);
    }
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

#[cfg(test)]
mod tests {
    use super::{
        AUTO_FLUSH_SILENCE, DownmixResampler, EndpointState, Level, append_with_limit,
        clean_transcript_text, expand_home_dir, model_file_name_from_url,
    };
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
    fn resampler_does_not_panic_on_44100_stereo_512_frame_chunks() {
        let mut resampler = DownmixResampler::new(44_100.0, 2);
        let input = vec![0.0_f32; 512 * 2];

        for _ in 0..20 {
            let samples = resampler.push_interleaved(&input, |sample| sample);
            assert!(!samples.is_empty());
        }
    }

    #[test]
    fn endpoint_flushes_after_speech_then_silence() {
        let mut endpoint = EndpointState::new();

        assert!(!endpoint.observe(
            1_600,
            Level {
                rms: 0.02,
                peak: 0.1,
            }
        ));
        assert!(endpoint.observe(
            AUTO_FLUSH_SILENCE,
            Level {
                rms: 0.0,
                peak: 0.0,
            }
        ));
    }

    #[test]
    fn append_with_limit_keeps_newest_samples() {
        let mut buffer = vec![1.0, 2.0, 3.0];

        append_with_limit(&mut buffer, &[4.0, 5.0, 6.0], 4);

        assert_eq!(buffer, vec![3.0, 4.0, 5.0, 6.0]);
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
