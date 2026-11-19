//! Dictation: capture microphone audio locally and transcribe it with the T3
//! server's speech service (`POST /api/voice/transcribe`), the same endpoint the
//! T3 Code GUI uses for environment voice input.
use std::{process::Stdio, time::Instant};

use tokio::{io::AsyncReadExt, process::Child, task::JoinHandle};

/// T3 accepts 16 kHz mono little-endian Float32 PCM, at most five minutes long.
pub const SAMPLE_RATE: usize = 16_000;
pub const MAX_SECONDS: u64 = 300;
const MAX_BYTES: usize = SAMPLE_RATE * 4 * MAX_SECONDS as usize;
/// Stop slightly before T3's limit so the final chunk still fits.
pub const AUTO_STOP_SECONDS: u64 = MAX_SECONDS - 5;
const MIN_SECONDS: f32 = 0.3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Start,
    Stop,
    Cancel,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Recording { program: &'static str },
    Level(f32),
    Transcribing,
    Finished(Result<String, String>),
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    #[default]
    Idle,
    Starting,
    Recording,
    Transcribing,
}

/// UI-side dictation state. Input only raises requests; the main loop forwards
/// them to the backend worker, which reports progress as [`Event`]s.
#[derive(Default)]
pub struct State {
    pub phase: Phase,
    pub started: Option<Instant>,
    pub level: f32,
    /// Thread whose draft receives the transcript, fixed when dictation starts.
    pub thread: Option<String>,
    pub toggle_requested: bool,
    pub cancel_requested: bool,
    pub send_on_finish: bool,
}

impl State {
    pub fn listening(&self) -> bool {
        matches!(self.phase, Phase::Starting | Phase::Recording)
    }

    /// Short label for the composer indicator, or `None` when idle.
    pub fn indicator(&self, now: Instant) -> Option<String> {
        match self.phase {
            Phase::Idle => None,
            Phase::Starting => Some(" ● Mic starting… ".into()),
            Phase::Recording => {
                let seconds = self
                    .started
                    .map_or(0, |started| now.duration_since(started).as_secs());
                Some(format!(
                    " ● Dictating {}:{:02} {} · Shift+D: stop · Esc: cancel ",
                    seconds / 60,
                    seconds % 60,
                    meter(self.level)
                ))
            }
            Phase::Transcribing => Some(" ◌ Transcribing… ".into()),
        }
    }
}

/// Five-cell level meter from an RMS value in 0..=1.
pub fn meter(level: f32) -> String {
    const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    // Speech RMS is usually well below 0.3; scale so normal talking fills the meter.
    let scaled = (level.max(0.0).sqrt() * 2.0).min(1.0);
    (0..5)
        .map(|cell| {
            let fill = (scaled * 5.0 - cell as f32).clamp(0.0, 1.0);
            if fill <= 0.0 {
                '·'
            } else {
                BARS[((fill * 7.0).round() as usize).min(7)]
            }
        })
        .collect()
}

/// Recorder commands that write raw 16 kHz mono f32le PCM to stdout, tried in order.
const RECORDERS: [(&str, &[&str]); 3] = [
    (
        "parecord",
        &[
            "--raw",
            "--format=float32le",
            "--rate=16000",
            "--channels=1",
            "--latency-msec=50",
        ],
    ),
    (
        "arecord",
        &[
            "-q", "-f", "FLOAT_LE", "-r", "16000", "-c", "1", "-t", "raw",
        ],
    ),
    (
        "ffmpeg",
        &[
            "-loglevel",
            "quiet",
            "-f",
            "pulse",
            "-i",
            "default",
            "-ac",
            "1",
            "-ar",
            "16000",
            "-f",
            "f32le",
            "-",
        ],
    ),
];

pub struct Recorder {
    child: Child,
    reader: JoinHandle<Vec<u8>>,
}

impl Recorder {
    /// Start the first available recorder. `level` receives RMS updates (~10/s).
    pub async fn start(
        level: impl Fn(f32) + Send + Sync + 'static,
    ) -> Result<(Self, &'static str), String> {
        let level = std::sync::Arc::new(level);
        let mut missing = true;
        for (program, args) in RECORDERS {
            let spawned = tokio::process::Command::new(program)
                .args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .spawn();
            let mut child = match spawned {
                Ok(child) => child,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => {
                    missing = false;
                    continue;
                }
            };
            missing = false;
            // A recorder without a usable input device exits almost immediately.
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            if !matches!(child.try_wait(), Ok(None)) {
                continue;
            }
            let Some(mut stdout) = child.stdout.take() else {
                let _ = child.start_kill();
                continue;
            };
            let level = level.clone();
            let reader = tokio::spawn(async move {
                let mut audio = Vec::new();
                let mut chunk = vec![0_u8; 8192];
                let mut last_level = Instant::now();
                while let Ok(read) = stdout.read(&mut chunk).await {
                    if read == 0 {
                        break;
                    }
                    let start = audio.len();
                    audio.extend_from_slice(&chunk[..read]);
                    if last_level.elapsed().as_millis() >= 100 {
                        last_level = Instant::now();
                        let aligned = start - start % 4;
                        level(rms(&audio[aligned..]));
                    }
                    if audio.len() >= MAX_BYTES {
                        audio.truncate(MAX_BYTES);
                        break;
                    }
                }
                audio
            });
            return Ok((Self { child, reader }, program));
        }
        Err(if missing {
            "Dictation needs a recorder: install PulseAudio/PipeWire tools (parecord), ALSA (arecord), or ffmpeg.".into()
        } else {
            "Could not open the microphone. Check that an input device is available.".into()
        })
    }

    /// Stop recording and return validated PCM ready for T3.
    pub async fn stop(mut self) -> Result<Vec<u8>, String> {
        let _ = self.child.start_kill();
        let _ = self.child.wait().await;
        let audio = self
            .reader
            .await
            .map_err(|_| "Audio capture stopped unexpectedly.")?;
        prepare_pcm(audio)
    }

    pub async fn cancel(mut self) {
        let _ = self.child.start_kill();
        let _ = self.child.wait().await;
        self.reader.abort();
    }
}

/// Root-mean-square level of complete f32le samples.
fn rms(bytes: &[u8]) -> f32 {
    let samples = bytes.chunks_exact(4).map(|sample| {
        let value = f32::from_le_bytes(sample.try_into().expect("four bytes"));
        if value.is_finite() { value } else { 0.0 }
    });
    let (sum, count) = samples.fold((0.0_f64, 0_usize), |(sum, count), value| {
        (sum + f64::from(value) * f64::from(value), count + 1)
    });
    if count == 0 {
        0.0
    } else {
        (sum / count as f64).sqrt() as f32
    }
}

/// T3 rejects non-finite or out-of-range samples, so sanitize and bound the audio.
pub fn prepare_pcm(mut audio: Vec<u8>) -> Result<Vec<u8>, String> {
    audio.truncate((audio.len() - audio.len() % 4).min(MAX_BYTES));
    if (audio.len() as f32) < SAMPLE_RATE as f32 * 4.0 * MIN_SECONDS {
        return Err("Dictation was too short; nothing was recorded.".into());
    }
    for sample in audio.as_chunks_mut::<4>().0 {
        let value = f32::from_le_bytes(*sample);
        let value = if value.is_finite() {
            value.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        sample.copy_from_slice(&value.to_le_bytes());
    }
    Ok(audio)
}

/// Request body for `/api/voice/transcribe`: a little-endian u32 options length,
/// the JSON transcription options, then the PCM samples. The options are T3's
/// GUI defaults, since speech preferences are stored by the GUI client.
pub fn transcribe_body(pcm: &[u8]) -> Vec<u8> {
    let options = serde_json::json!({
        "speechLanguage": "auto",
        "speechTranslateToEnglish": false,
        "speechCustomWords": [],
        "speechRemoveFillerWords": true,
        "speechSilenceFiltering": true,
    })
    .to_string();
    let mut body = Vec::with_capacity(4 + options.len() + pcm.len());
    body.extend_from_slice(&(options.len() as u32).to_le_bytes());
    body.extend_from_slice(options.as_bytes());
    body.extend_from_slice(pcm);
    body
}

/// Join dictated text onto an existing draft with a single separating space.
pub fn join_text(cursor_prefix: &str, text: &str) -> String {
    let text = text.trim();
    if text.is_empty() {
        return String::new();
    }
    if cursor_prefix.is_empty() || cursor_prefix.ends_with(char::is_whitespace) {
        text.into()
    } else {
        format!(" {text}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm(samples: &[f32]) -> Vec<u8> {
        samples
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect()
    }

    #[test]
    fn body_has_length_prefixed_options_then_pcm() {
        let audio = pcm(&[0.5, -0.5]);
        let body = transcribe_body(&audio);
        let length = u32::from_le_bytes(body[..4].try_into().unwrap()) as usize;
        let options: serde_json::Value = serde_json::from_slice(&body[4..4 + length]).unwrap();
        assert_eq!(options["speechLanguage"], "auto");
        assert_eq!(&body[4 + length..], audio.as_slice());
    }

    #[test]
    fn prepare_sanitizes_samples_and_rejects_short_audio() {
        assert!(prepare_pcm(pcm(&[0.1; 100])).is_err());
        let mut samples = vec![0.0_f32; SAMPLE_RATE];
        samples[0] = 2.0;
        samples[1] = f32::NAN;
        samples[2] = -3.0;
        let mut bytes = pcm(&samples);
        bytes.push(7); // partial trailing sample
        let prepared = prepare_pcm(bytes).unwrap();
        assert_eq!(prepared.len(), SAMPLE_RATE * 4);
        let first: Vec<f32> = prepared[..12]
            .chunks_exact(4)
            .map(|chunk| f32::from_le_bytes(chunk.try_into().unwrap()))
            .collect();
        assert_eq!(first, vec![1.0, 0.0, -1.0]);
    }

    #[test]
    fn joins_with_a_space_only_when_needed() {
        assert_eq!(join_text("", " hello "), "hello");
        assert_eq!(join_text("fix", "this"), " this");
        assert_eq!(join_text("fix ", "this"), "this");
        assert_eq!(join_text("x", "   "), "");
    }

    #[test]
    fn indicator_and_meter() {
        let state = State {
            phase: Phase::Recording,
            started: Some(Instant::now()),
            level: 0.0,
            ..Default::default()
        };
        let label = state.indicator(Instant::now()).unwrap();
        assert!(label.contains("Dictating 0:00"));
        assert_eq!(meter(0.0), "·····");
        assert_eq!(meter(1.0).chars().filter(|c| *c == '█').count(), 5);
        assert!(State::default().indicator(Instant::now()).is_none());
    }
}
