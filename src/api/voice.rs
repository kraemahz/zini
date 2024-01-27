use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rubato::{Resampler, SincFixedIn, SincInterpolationType, SincInterpolationParameters};
use serde::{Serialize, Deserialize};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::errors::Error as SymError;
use symphonia::core::probe::Hint;
use tokio::{sync::{mpsc, oneshot}, task::spawn, time::timeout};
use uuid::Uuid;

use super::prompts::ChatCompletion;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum AudioContext {
    Discover,
    Search,
    Instruct
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AudioContextResponse {
    Search(String),
    Instruct(ChatCompletion)
}
pub type AudioDataChannel = (AudioContext, AudioData);
pub type AudioEventChannel = (SpeechToText, oneshot::Sender<SpeechToTextResponse>);

#[derive(Clone, Debug)]
pub struct AudioData {
    pub payload: Vec<u8>,
    pub count: usize,
}

#[derive(Clone, Debug)]
pub struct SpeechToText{
    pub conversation_id: Uuid,
    pub payload: Vec<f32>,
    pub count: usize,
    pub finalize: bool
}

#[derive(Clone, Debug, Serialize)]
pub struct SpeechToTextRequest {
    pub conversation_id: Uuid,
    pub payload: Vec<f32>,
    pub count: usize,
    pub beam: String,
    pub finalize: bool
}

impl SpeechToTextRequest {
    pub fn extend_from(stt: SpeechToText, beam: String) -> SpeechToTextRequest {
        SpeechToTextRequest {
            conversation_id: stt.conversation_id,
            payload: stt.payload,
            count: stt.count,
            beam,
            finalize: stt.finalize
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SpeechToTextResponse {
    pub conversation_id: Uuid,
    pub count: usize,
    pub payload: String,
    pub finalized: bool
}

pub fn decode_webm_bytes_to_pcm(payload: Vec<u8>) -> Result<(u32, Vec<f32>), SymError> {
    let cursor = io::Cursor::new(payload);
    let media_stream = MediaSourceStream::new(Box::new(cursor), Default::default());
    let mut hint = Hint::new();
    hint.mime_type("audio/webm");
    let probed = symphonia::default::get_probe()
        .format(&hint, media_stream, &Default::default(), &Default::default())?;
    let mut format_reader = probed.format;
    let mut track = format_reader.default_track()
        .ok_or_else(|| SymError::Unsupported("No track"))?.clone();
    track.codec_params.max_frames_per_packet = Some(1024);
    let rate = track.codec_params.sample_rate.expect("No sample rate");
    let track_id = track.id;

    let mut decoder = symphonia::default::get_codecs().make(&track.codec_params, &Default::default())?;
    let mut pcm_data = None;
    let mut samples = Vec::new();

    while let Ok(packet) = format_reader.next_packet() {
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(audio_buf) => {
                if pcm_data.is_none() {
                    let spec = *audio_buf.spec();
                    let duration = audio_buf.capacity() as u64;
                    pcm_data = Some(SampleBuffer::<f32>::new(duration, spec));
                }
                if let Some(buf) = &mut pcm_data {
                    buf.copy_interleaved_ref(audio_buf);
                    samples.extend(buf.samples());
                }
            }
            Err(SymError::DecodeError(_)) => (),
            Err(_) => break,
        }
    }
    Ok((rate, samples))
}

pub fn resample_audio(input_pcm: Vec<f32>, from_rate: u32, to_rate: u32) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let ratio = to_rate as f64 / from_rate as f64;
    let output_size = (input_pcm.len() as f64 * ratio).round() as usize;
    let params = SincInterpolationParameters {
        sinc_len: 256,
        f_cutoff: 0.8,
        interpolation: SincInterpolationType::Nearest,
        oversampling_factor: 256,
        window: rubato::WindowFunction::BlackmanHarris2,
    };

    let mut resampler = SincFixedIn::<f32>::new(
        ratio,
        1.0,
        params,
        input_pcm.len(),
        1
    )?;

    // HACK: This random +10 made the 48k -> 16k version work
    let mut output_pcm = vec![0f32; output_size + 10];
    resampler.process_into_buffer(&[input_pcm], &mut [output_pcm.as_mut_slice()], None)?;
    Ok(output_pcm)
}

pub fn create_audio_timing_task(
    text_tx: mpsc::Sender<SpeechToTextResponse>,
    mut audio_rx: mpsc::Receiver<AudioDataChannel>,
    audio_stream: mpsc::Sender<AudioEventChannel>,
) {
    const SPEECH_TIMEOUT: Duration = Duration::from_secs(2);

    spawn(async move {
        let mut n_messages: usize = 0;
        let mut stream: Option<Vec<u8>> = None;
        let mut _context = AudioContext::Discover;

        loop {
            let conversation_id: Uuid = Uuid::new_v4();
            let result = timeout(SPEECH_TIMEOUT, audio_rx.recv()).await;
            let stt = match result {
                Ok(msg) => msg,
                Err(_) => {
                    // Timeout
                    // Collect stream and send as finalized message.
                    if let Some(payload) = stream.take() {
                        let (rate, payload) = match decode_webm_bytes_to_pcm(payload) {
                            Ok(payload) => payload,
                            Err(err) => {
                                tracing::error!("Decoding sample failed: {}", err);
                                continue;
                            }
                        };
                        let payload = resample_audio(payload, rate, 16_000).expect("Resampling failed");
                        let request = SpeechToText {
                            conversation_id,
                            count: n_messages,
                            payload,
                            finalize: true
                        };
                        n_messages += 1;
                        // TODO
                        let (tx, rx) = oneshot::channel();
                        if audio_stream.send((request, tx)).await.is_err() {
                            break;
                        }

                        let text_tx = text_tx.clone();
                        spawn(async move {
                            if let Ok(response) = rx.await {
                                text_tx.send(response).await.ok();
                            }
                        });
                    }
                    continue;
                }
            };

            if let Some((new_context, AudioData{payload, count: _})) = stt {
                stream = match stream.take() {
                    Some(mut audio) => {
                        audio.extend(payload);
                        Some(audio)
                    }
                    None => {
                        _context = new_context;
                        Some(payload)
                    }
                };
            }
        }
        tracing::warn!("Speech to instructions exited");
    });
}

#[derive(Clone, Debug)]
pub struct VoiceResponseCollection {
    inner: Arc<Mutex<HashMap<(Uuid, usize), oneshot::Sender<SpeechToTextResponse>>>>
}

impl VoiceResponseCollection {
    pub fn new() -> Self {
        Self { inner: Arc::new(Mutex::new(HashMap::new())) }
    }

    pub fn insert(&self,
                  conversation_id: Uuid,
                  count: usize,
                  sender: oneshot::Sender<SpeechToTextResponse>) {
        self.inner.lock().unwrap().insert((conversation_id, count), sender);
    }

    pub fn send_response(&self, voice_response: SpeechToTextResponse) {
        let lookup = &(voice_response.conversation_id, voice_response.count);
        if let Some(sender) = self.inner.lock().unwrap().remove(lookup) {
            sender.send(voice_response).ok();
        }
    }
}
