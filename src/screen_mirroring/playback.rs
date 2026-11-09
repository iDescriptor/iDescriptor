// SPDX-FileCopyrightText: 2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later
use anyhow::{Context, Result, anyhow, ensure};
use gstreamer::{self as gst, prelude::*};
use gstreamer_app::AppSrc;
use std::{
    ffi::c_void,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

pub struct Playback {
    // Serialize startup with shutdown: the worker must never restart a sink
    // after the QML owner has detached its borrowed QQuickItem pointer.
    pipeline: Mutex<Option<gst::Pipeline>>,
    source: AppSrc,
    first_frame: Arc<AtomicBool>,
    // capture: Arc<super::diagnostics::Capture>,
    picture: Arc<Mutex<Option<super::visible_frame::Picture>>>,
    crop: gst::Element,
    display_size: Arc<Mutex<Option<(u32, u32)>>>,
}
impl Playback {
    pub fn new(
        video_item: usize,
        first_frame: impl Fn() + Send + Sync + 'static,
        size_changed: impl Fn(u32, u32) + Send + Sync + 'static,
        // capture: Arc<super::diagnostics::Capture>,
    ) -> Result<Self> {
        ensure!(
            video_item != 0,
            "The screen mirroring video item is unavailable"
        );
        gst::init()?;
        // QML fits the item to decoded dimensions and maps input against that
        // same rectangle. Avoid a second independent letterbox inside the sink.
        let description = concat!(
            "appsrc name=source is-live=true format=time do-timestamp=true block=false max-bytes=8388608 ",
            "! h265parse config-interval=-1 ! decodebin ! videoconvert ! video/x-raw,format=RGBA ",
            "! videocrop name=visible_crop ! queue name=decoded_video leaky=downstream max-size-buffers=2 max-size-bytes=0 max-size-time=0 ",
            "! glupload ! qml6glsink name=video_sink sync=false force-aspect-ratio=false"
        );
        let pipeline = gst::parse::launch(description)
            .context(
                "Could not create the mirroring pipeline; check GStreamer HEVC and Qt GL plugins",
            )?
            .downcast::<gst::Pipeline>()
            .map_err(|_| anyhow!("Invalid mirroring pipeline"))?;
        //         capture.attach(
        //             &pipeline
        //                 .by_name("visible_crop")
        //                 .context("Missing decoded video queue")?
        //                 .static_pad("sink")
        //                 .context("Missing decoded video pad")?,
        //         );
        let picture = Arc::new(Mutex::new(None));
        let crop = pipeline
            .by_name("visible_crop")
            .context("Missing video crop")?;
        let picture_for_caps = picture.clone();
        let display_size = Arc::new(Mutex::new(None));
        let display_for_caps = display_size.clone();
        let crop_weak = crop.downgrade();
        crop.static_pad("sink")
            .context("Missing video crop input")?
            .add_probe(gst::PadProbeType::EVENT_DOWNSTREAM, move |_, info| {
                if let Some(gst::PadProbeData::Event(event)) = &info.data
                    && let gst::EventView::Caps(caps) = event.view()
                    && let Some(crop) = crop_weak.upgrade()
                {
                    apply_crop(
                        &crop,
                        caps.caps(),
                        *picture_for_caps.lock().unwrap(),
                        *display_for_caps.lock().unwrap(),
                    );
                }
                gst::PadProbeReturn::Ok
            });
        let source = pipeline
            .by_name("source")
            .context("Missing video source")?
            .downcast::<AppSrc>()
            .map_err(|_| anyhow!("Invalid video source"))?;
        source.set_caps(Some(
            &gst::Caps::builder("video/x-h265")
                .field("stream-format", "byte-stream")
                .build(),
        ));
        let sink = pipeline
            .by_name("video_sink")
            .context("Missing video sink")?;
        sink.set_property("widget", video_item as *mut c_void);
        sink.static_pad("sink")
            .context("Missing video sink pad")?
            .add_probe(gst::PadProbeType::EVENT_DOWNSTREAM, move |_, info| {
                if let Some(gst::PadProbeData::Event(event)) = &info.data
                    && let gst::EventView::Caps(caps) = event.view()
                    && let Some(format) = caps.caps().structure(0)
                    && let (Ok(width), Ok(height)) =
                        (format.get::<i32>("width"), format.get::<i32>("height"))
                    && width > 0
                    && height > 0
                {
                    size_changed(width as u32, height as u32);
                }
                gst::PadProbeReturn::Ok
            });
        let received = Arc::new(AtomicBool::new(false));
        let frame_flag = received.clone();
        sink.static_pad("sink")
            .context("Missing video sink pad")?
            .add_probe(gst::PadProbeType::BUFFER, move |_, _| {
                frame_flag.store(true, Ordering::Relaxed);
                first_frame();
                gst::PadProbeReturn::Remove
            });
        let playback = Self {
            pipeline: Mutex::new(Some(pipeline)),
            source,
            first_frame: received,
            // capture,
            picture,
            crop,
            display_size,
        };
        // Initialize the Qt GL sink on the Qt thread, as in the AirPlay player.
        playback
            .pipeline
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .set_state(gst::State::Ready)
            .context("Could not initialize the Qt video sink")?;
        Ok(playback)
    }
    pub fn diagnostic_reply(&self, name: &str, value: plist::Value) {
        if name == "device-display-info" {
            let size = super::visible_frame::display_size(&value, 1);
            log::info!("Mirroring requested display 1 current mode: {size:?}");
            *self.display_size.lock().unwrap() = size;
        }
        // self.capture.set_reply(name, value);
    }
    //     pub fn capture_frame(&self, geometry: String) {
    //         self.capture.request(geometry);
    //     }
    pub fn has_frame(&self) -> bool {
        self.first_frame.load(Ordering::Relaxed)
    }
    pub fn start(&self) -> Result<()> {
        let guard = self.pipeline.lock().unwrap();
        let pipeline = guard.as_ref().context("Mirroring was stopped")?;
        pipeline
            .set_state(gst::State::Playing)
            .context("Could not start video decoding")?;
        Ok(())
    }
    pub fn push(&self, bytes: Vec<u8>) -> Result<()> {
        // self.capture.observe_parameters(&bytes);
        if let Some(picture) = super::visible_frame::picture(&bytes) {
            let mut current = self.picture.lock().unwrap();
            if *current != Some(picture) {
                log::info!("Mirroring HEVC picture geometry: {picture:?}");
                *current = Some(picture);
                if let Some(caps) = self
                    .crop
                    .static_pad("sink")
                    .and_then(|pad| pad.current_caps())
                {
                    apply_crop(
                        &self.crop,
                        &caps,
                        Some(picture),
                        *self.display_size.lock().unwrap(),
                    );
                }
            }
        }

        // Never drop arbitrary compressed slices: that corrupts reference frames.
        // The downstream queue drops decoded frames; stop if decoding is stuck.
        ensure!(
            self.source.current_level_bytes() < 8 * 1024 * 1024,
            "The HEVC decoder cannot keep up with the device stream"
        );
        self.source
            .push_buffer(gst::Buffer::from_mut_slice(bytes))
            .context("Could not feed the HEVC decoder")?;
        Ok(())
    }
    pub fn check_error(&self) -> Result<()> {
        let guard = self.pipeline.lock().unwrap();
        if let Some(pipeline) = guard.as_ref() {
            let bus = pipeline.bus().context("Missing video pipeline bus")?;
            while let Some(message) = bus.pop() {
                if let gst::MessageView::Error(error) = message.view() {
                    return Err(anyhow!(
                        "Video decoder: {} ({:?})",
                        error.error(),
                        error.debug()
                    ));
                }
            }
        }
        Ok(())
    }
    pub fn stop(&self) {
        if let Some(pipeline) = self.pipeline.lock().unwrap().take() {
            if let Err(error) = pipeline.set_state(gst::State::Null) {
                log::warn!("Could not stop mirroring playback: {error:?}");
            }
            if let Some(sink) = pipeline.by_name("video_sink") {
                sink.set_property("widget", std::ptr::null_mut::<c_void>());
            }
        }
    }
}
impl Drop for Playback {
    fn drop(&mut self) {
        self.stop();
    }
}

fn apply_crop(
    crop: &gst::Element,
    caps: &gst::CapsRef,
    picture: Option<super::visible_frame::Picture>,
    display: Option<(u32, u32)>,
) {
    let Some(format) = caps.structure(0) else {
        return;
    };
    let (Ok(width), Ok(height)) = (format.get::<i32>("width"), format.get::<i32>("height")) else {
        return;
    };
    if width <= 0 || height <= 0 {
        return;
    }
    let offsets = picture
        .and_then(|p| super::visible_frame::visible_crop(p, display, width as u32, height as u32))
        .unwrap_or([0; 4]);
    for (property, value) in ["left", "right", "top", "bottom"].into_iter().zip(offsets) {
        crop.set_property(property, value);
    }
    log::info!(
        "Mirroring decoder geometry {width}x{height}, visible crop left={} right={} top={} bottom={}",
        offsets[0],
        offsets[1],
        offsets[2],
        offsets[3]
    );
}

#[cfg(test)]
mod crop_tests {
    use super::*;
    #[test]
    fn sps_crop_negotiates_visible_output_and_keeps_black_content() {
        gst::init().unwrap();
        let pipeline = gst::parse::launch(
            "appsrc name=source format=time ! videocrop name=crop ! appsink name=result sync=false",
        )
        .unwrap()
        .downcast::<gst::Pipeline>()
        .unwrap();
        let source = pipeline
            .by_name("source")
            .unwrap()
            .downcast::<AppSrc>()
            .unwrap();
        let sink = pipeline
            .by_name("result")
            .unwrap()
            .downcast::<gstreamer_app::AppSink>()
            .unwrap();
        let crop = pipeline.by_name("crop").unwrap();
        let caps = gst::Caps::builder("video/x-raw")
            .field("format", "RGBA")
            .field("width", 80_i32)
            .field("height", 112_i32)
            .field("framerate", gst::Fraction::new(30, 1))
            .build();
        let picture = super::super::visible_frame::Picture {
            width: 80,
            height: 112,
            left: 0,
            top: 0,
            visible_width: 80,
            visible_height: 112,
            chroma_width: 2,
            chroma_height: 2,
        };
        apply_crop(&crop, &caps, Some(picture), Some((68, 100)));
        source.set_caps(Some(&caps));
        pipeline.set_state(gst::State::Playing).unwrap();
        // The entire visible picture is black. Cropping depends exclusively on
        // SPS geometry; there is no content-color detector to erase this image.
        let mut pixels = vec![0_u8; 80 * 112 * 4];
        for pixel in pixels.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        source
            .push_buffer(gst::Buffer::from_mut_slice(pixels))
            .unwrap();
        source.end_of_stream().unwrap();
        let sample = sink
            .try_pull_sample(gst::ClockTime::from_seconds(2))
            .expect("cropped frame");
        let structure = sample.caps().unwrap().structure(0).unwrap();
        assert_eq!(structure.get::<i32>("width").unwrap(), 68);
        assert_eq!(structure.get::<i32>("height").unwrap(), 100);
        assert_eq!(sample.buffer().unwrap().size(), 68 * 100 * 4);
        pipeline.set_state(gst::State::Null).unwrap();
    }
}
