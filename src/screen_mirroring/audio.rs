// SPDX-FileCopyrightText: 2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later
use anyhow::{Context, Result, anyhow, ensure};
use gstreamer::{self as gst, prelude::*};
use gstreamer_app::AppSrc;
use idevice::core_device::RtpPacket;
use std::sync::{Arc, Mutex};

// Device Hub's AAC-ELD configuration: 48 kHz, stereo, 480 samples/AU.
// See pymobiledevice3/remote/core_device/aac_eld.py.
// Explicit short-frame flag for software decoders. The reference AudioToolbox
// client uses f8 e6 40 00 but separately forces mFramesPerPacket=480; libav
// needs that 480-sample choice in AudioSpecificConfig itself.
const COOKIE: [u8; 4] = [0xf8, 0xe6, 0x50, 0x00];
const RATE: u64 = 48_000;
#[derive(Clone)]
pub enum AudioEvent {
    Playing,
    Failed(String),
}
type Notify = Arc<dyn Fn(AudioEvent) + Send + Sync>;

#[derive(Default)]
struct Timeline {
    sequence: Option<u16>,
    timestamp: Option<u32>,
}
impl Timeline {
    fn packet(&mut self, sequence: u16, timestamp: u32) -> Option<(u64, bool)> {
        let distance = self.sequence.map(|last| sequence.wrapping_sub(last));
        if distance.is_some_and(|distance| distance == 0 || distance >= 0x8000) {
            return None; // Do not replay duplicates or late packets.
        }
        let first = *self.timestamp.get_or_insert(timestamp);
        let ticks = timestamp.wrapping_sub(first);
        if ticks >= 0x8000_0000 {
            return None;
        }
        self.sequence = Some(sequence);
        Some((
            u64::from(ticks) * 1_000_000_000 / RATE,
            distance.is_some_and(|d| d > 1),
        ))
    }
}
struct Renderer {
    pipeline: gst::Pipeline,
    source: AppSrc,
    volume: gst::Element,
    payload_type: u8,
    timeline: Timeline,
    origin: Option<gst::ClockTime>,
}
impl Drop for Renderer {
    fn drop(&mut self) {
        if let Err(error) = self.pipeline.set_state(gst::State::Null) {
            log::warn!("Could not stop mirroring audio: {error:?}");
        }
    }
}
struct State {
    renderer: Option<Renderer>,
    volume: f64,
    stopped: bool,
}
pub struct AudioOutput {
    state: Mutex<State>,
    notify: Notify,
}
impl AudioOutput {
    pub fn new(volume: f64, notify: impl Fn(AudioEvent) + Send + Sync + 'static) -> Self {
        Self {
            state: Mutex::new(State {
                renderer: None,
                volume,
                stopped: false,
            }),
            notify: Arc::new(notify),
        }
    }
    pub fn start(&self, answer: &plist::Value) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        if state.stopped {
            return Ok(());
        }
        let config = answer
            .as_dictionary()
            .and_then(|value| value.get("connection"))
            .and_then(plist::Value::as_dictionary)
            .and_then(|value| value.get("streamConfig"))
            .and_then(plist::Value::as_dictionary)
            .context("Device returned no audio configuration")?;
        ensure!(
            config
                .get("AudioStreamMode")
                .and_then(plist::Value::as_unsigned_integer)
                == Some(8),
            "Device negotiated an unsupported audio mode"
        );
        let payload_type: u8 = config
            .get("RxPayloadType")
            .and_then(plist::Value::as_unsigned_integer)
            .context("Device returned no audio RTP payload type")?
            .try_into()?;
        let renderer = build_renderer(
            state.volume,
            payload_type,
            self.notify.clone(),
            "autoaudiosink",
        )?;
        renderer
            .pipeline
            .set_state(gst::State::Playing)
            .context("Could not start PC audio output")?;
        state.renderer = Some(renderer);
        log::info!("Screen mirroring audio started: AAC-ELD, 48000 Hz, stereo, 480 samples/frame");
        Ok(())
    }
    pub fn fail(&self, error: anyhow::Error) {
        self.state.lock().unwrap().renderer = None;
        log::warn!("Screen mirroring audio unavailable: {error:#}");
        (self.notify)(AudioEvent::Failed(format!("{error:#}")));
    }
    pub fn set_volume(&self, volume: f64) {
        let mut state = self.state.lock().unwrap();
        state.volume = volume;
        if let Some(renderer) = &state.renderer {
            renderer.volume.set_property("volume", volume);
        }
    }
    pub fn push(&self, packet: &RtpPacket<'_>) {
        let result = (|| -> Result<()> {
            let mut state = self.state.lock().unwrap();
            let Some(renderer) = state.renderer.as_mut() else {
                return Ok(());
            };
            if packet.payload_type != renderer.payload_type || packet.payload.is_empty() {
                return Ok(());
            }
            let Some((elapsed, gap)) = renderer
                .timeline
                .packet(packet.sequence_number, packet.timestamp)
            else {
                return Ok(());
            };
            ensure!(
                renderer.source.current_level_bytes() < 128 * 1024,
                "The audio decoder cannot keep up with the device stream"
            );
            let origin = *renderer.origin.get_or_insert_with(|| {
                renderer
                    .pipeline
                    .current_running_time()
                    .unwrap_or(gst::ClockTime::ZERO)
            });
            let mut buffer = gst::Buffer::from_mut_slice(packet.payload.to_vec());
            let writable = buffer.get_mut().unwrap();
            writable.set_pts(origin + gst::ClockTime::from_nseconds(elapsed));
            writable.set_duration(gst::ClockTime::from_mseconds(10));
            if gap {
                writable.set_flags(gst::BufferFlags::DISCONT);
            }
            renderer
                .source
                .push_buffer(buffer)
                .context("Could not feed the AAC-ELD decoder")?;
            Ok(())
        })();
        if let Err(error) = result {
            self.fail(error);
        }
    }
    pub fn check_error(&self) {
        let result = (|| -> Result<()> {
            let state = self.state.lock().unwrap();
            if let Some(renderer) = &state.renderer {
                let bus = renderer
                    .pipeline
                    .bus()
                    .context("Missing audio pipeline bus")?;
                while let Some(message) = bus.pop() {
                    if let gst::MessageView::Error(error) = message.view() {
                        return Err(anyhow!("PC audio: {} ({:?})", error.error(), error.debug()));
                    }
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.fail(error);
        }
    }
    pub fn stop(&self) {
        let mut state = self.state.lock().unwrap();
        state.stopped = true;
        state.renderer = None;
    }
}
impl Drop for AudioOutput {
    fn drop(&mut self) {
        self.stop();
    }
}

fn build_renderer(volume: f64, payload_type: u8, notify: Notify, sink: &str) -> Result<Renderer> {
    let pipeline = gst::parse::launch(&format!(
        "appsrc name=source is-live=true format=time block=false max-bytes=131072 ! avdec_aac max-errors=5 ! audioconvert ! audioresample ! queue name=audio_queue max-size-time=100000000 max-size-buffers=0 max-size-bytes=0 leaky=downstream ! volume name=volume ! {sink}"
    )).context("Could not create audio playback; check the GStreamer AAC decoder and audio output plugins")?
        .downcast::<gst::Pipeline>().map_err(|_| anyhow!("Invalid audio pipeline"))?;
    let source = pipeline
        .by_name("source")
        .context("Missing audio source")?
        .downcast::<AppSrc>()
        .map_err(|_| anyhow!("Invalid audio source"))?;
    source.set_caps(Some(
        &gst::Caps::builder("audio/mpeg")
            .field("mpegversion", 4_i32)
            .field("stream-format", "raw")
            .field("framed", true)
            .field("rate", RATE as i32)
            .field("channels", 2_i32)
            .field("codec_data", gst::Buffer::from_slice(COOKIE))
            .build(),
    ));
    let gain = pipeline
        .by_name("volume")
        .context("Missing audio volume control")?;
    gain.set_property("volume", volume);
    gain.static_pad("src")
        .context("Missing audio output pad")?
        .add_probe(gst::PadProbeType::BUFFER, move |_, _| {
            notify(AudioEvent::Playing);
            gst::PadProbeReturn::Remove
        });
    Ok(Renderer {
        pipeline,
        source,
        volume: gain,
        payload_type,
        timeline: Timeline::default(),
        origin: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn audio_timestamps_handle_wrap_loss_duplicates_and_reordering() {
        let mut timeline = Timeline::default();
        assert_eq!(timeline.packet(65535, u32::MAX - 479), Some((0, false)));
        assert_eq!(timeline.packet(0, 0), Some((10_000_000, false)));
        assert_eq!(timeline.packet(0, 0), None);
        assert_eq!(timeline.packet(65535, u32::MAX - 479), None);
        assert_eq!(timeline.packet(2, 960), Some((30_000_000, true)));
    }
    #[test]
    fn stopped_output_does_not_restart_or_emit_sound() {
        let output = AudioOutput::new(0.5, |_| {});
        output.stop();
        assert!(
            output
                .start(&plist::Value::Dictionary(Default::default()))
                .is_ok()
        );
        assert!(output.state.lock().unwrap().renderer.is_none());
    }
    #[test]
    fn decodes_synthetic_480_sample_aac_eld_frames() {
        gst::init().unwrap();
        let fixture = include_bytes!("testdata/aac-eld-480.bin");
        let renderer = build_renderer(
            1.0,
            101,
            Arc::new(|_| {}),
            "appsink name=decoded sync=false",
        )
        .unwrap();
        // Disable realtime dropping while the test feeds a burst faster than
        // a physical stream, so every generated access unit can be checked.
        renderer
            .pipeline
            .by_name("audio_queue")
            .unwrap()
            .set_property("max-size-time", 0_u64);
        let sink = renderer
            .pipeline
            .by_name("decoded")
            .unwrap()
            .downcast::<gstreamer_app::AppSink>()
            .unwrap();
        renderer.pipeline.set_state(gst::State::Playing).unwrap();
        let mut bytes = fixture.as_slice();
        let mut count = 0;
        while !bytes.is_empty() {
            let size = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
            renderer
                .source
                .push_buffer(gst::Buffer::from_mut_slice(bytes[4..4 + size].to_vec()))
                .unwrap();
            bytes = &bytes[4 + size..];
            count += 1;
        }
        renderer.source.end_of_stream().unwrap();
        let mut decoded = 0;
        let mut peak = 0.0_f32;
        while let Some(sample) = sink.try_pull_sample(gst::ClockTime::from_seconds(2)) {
            let caps = sample.caps().unwrap();
            let format = caps.structure(0).unwrap();
            assert_eq!(format.get::<i32>("rate").unwrap(), 48_000);
            assert_eq!(format.get::<i32>("channels").unwrap(), 2);
            assert_eq!(format.get::<String>("format").unwrap(), "F32LE");
            let buffer = sample.buffer().unwrap();
            let mapped = buffer.map_readable().unwrap();
            for bytes in mapped.as_slice().chunks_exact(4) {
                let value = f32::from_le_bytes(bytes.try_into().unwrap());
                assert!(value.is_finite());
                peak = peak.max(value.abs());
            }
            let size = buffer.size();
            assert_eq!(size, 480 * 2 * 4, "wrong decoded frame size");
            decoded += 1;
        }
        if decoded == 0 {
            let bus = renderer.pipeline.bus().unwrap();
            while let Some(message) = bus.pop() {
                if let gst::MessageView::Error(e) = message.view() {
                    panic!("{} {:?}", e.error(), e.debug());
                }
            }
        }
        assert_eq!(decoded, count);
        assert!(
            peak > 0.01 && peak < 0.2,
            "expected quiet sine wave, got peak {peak}"
        );
    }
}
