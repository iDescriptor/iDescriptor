//! THIS IS USED FOR DEBUGGING, DO NO DELETE THIS FILE
// // SPDX-FileCopyrightText: 2026 Uncore <https://github.com/uncor3>
// // SPDX-License-Identifier: AGPL-3.0-or-later
// use anyhow::{Context, Result, anyhow};
// use gstreamer::{self as gst, prelude::*};
// use gstreamer_app::AppSrc;
// use std::sync::{Arc, Mutex};
// use std::time::Duration;

// pub type CaptureResult = Result<String, String>;
// #[derive(Clone, Default)]
// struct SourceContext {
//     parameters: std::collections::BTreeMap<u8, Vec<u8>>,
//     replies: std::collections::BTreeMap<String, plist::Value>,
// }
// pub struct Capture {
//     source: Mutex<SourceContext>,
//     pending: Mutex<Option<String>>,
//     completed: Arc<dyn Fn(CaptureResult) + Send + Sync>,
// }
// impl Capture {
//     pub fn new(completed: impl Fn(CaptureResult) + Send + Sync + 'static) -> Self {
//         Self {
//             pending: Mutex::new(None),
//             source: Mutex::new(SourceContext::default()),
//             completed: Arc::new(completed),
//         }
//     }
//     pub fn observe_parameters(&self, bytes: &[u8]) {
//         let nals = super::visible_frame::parameter_sets(bytes);
//         if nals.is_empty() {
//             return;
//         }
//         let mut source = self.source.lock().unwrap();
//         for nal in nals {
//             source.parameters.insert((nal[0] >> 1) & 63, nal.to_vec());
//         }
//     }
//     pub fn set_reply(&self, name: &str, value: plist::Value) {
//         self.source
//             .lock()
//             .unwrap()
//             .replies
//             .insert(name.into(), value);
//     }
//     pub fn request(&self, geometry: String) {
//         *self.pending.lock().unwrap() = Some(geometry);
//     }
//     pub fn attach(self: &Arc<Self>, pad: &gst::Pad) {
//         let capture = self.clone();
//         pad.add_probe(gst::PadProbeType::BUFFER, move |pad, info| {
//             let Some(geometry) = capture.pending.lock().unwrap().take() else {
//                 return gst::PadProbeReturn::Ok;
//             };
//             if let Some(gst::PadProbeData::Buffer(buffer)) = &info.data
//                 && let Some(caps) = pad.current_caps()
//             {
//                 let caps = caps.to_owned();
//                 let buffer = buffer.to_owned();
//                 let completed = capture.completed.clone();
//                 let source = capture.source.lock().unwrap().clone();
//                 crate::RUNTIME.spawn_blocking(move || {
//                     let result = save_frame(buffer, caps, &geometry)
//                         .and_then(|path| {
//                             let mut parameters = Vec::new();
//                             for nal in source.parameters.values() {
//                                 parameters.extend_from_slice(&[0, 0, 0, 1]);
//                                 parameters.extend_from_slice(nal);
//                             }
//                             std::fs::write(
//                                 std::path::Path::new(&path).join("stream-parameters.h265"),
//                                 parameters,
//                             )?;
//                             for (name, value) in source.replies {
//                                 plist::to_file_xml(
//                                     std::path::Path::new(&path).join(format!("{name}.plist")),
//                                     &value,
//                                 )?;
//                             }
//                             Ok(path)
//                         })
//                         .map_err(|error: anyhow::Error| format!("{error:#}"));
//                     match &result {
//                         Ok(path) => log::info!("Mirroring diagnostics saved to {path}"),
//                         Err(error) => log::warn!("Mirroring diagnostic capture failed: {error}"),
//                     }
//                     completed(result);
//                 });
//             } else {
//                 (capture.completed)(Err("No CPU video frame/caps available".into()));
//             }
//             gst::PadProbeReturn::Ok
//         });
//     }
// }
// fn save_frame(buffer: gst::Buffer, caps: gst::Caps, geometry: &str) -> Result<String> {
//     let directory =
//         std::env::temp_dir().join(format!("idescriptor-mirroring-{}", uuid::Uuid::new_v4()));
//     std::fs::create_dir(&directory)?;
//     let mapped = buffer
//         .map_readable()
//         .context("Could not read decoded RGBA frame")?;
//     std::fs::write(directory.join("decoded-frame.rgba"), mapped.as_slice())?;
//     let metadata = buffer
//         .iter_meta::<gst::Meta>()
//         .map(|meta| meta.api().name().to_string())
//         .collect::<Vec<_>>();
//     let report = serde_json::json!({
//         "caps":caps.to_string(), "bytes":mapped.size(), "metadata_apis":metadata,
//         "pts_ns":buffer.pts().map(|pts|pts.nseconds()),
//         "qml_geometry":serde_json::from_str::<serde_json::Value>(geometry).unwrap_or_else(|_|serde_json::Value::String(geometry.into())),
//         "note":"Captured before glupload, window masks, and QML rotation. RGBA dump preserves buffer layout; caps dimensions alone do not establish stride or crop. No automatic cropping applied."
//     });
//     std::fs::write(
//         directory.join("geometry.json"),
//         serde_json::to_vec_pretty(&report)?,
//     )?;
//     // Let GStreamer handle video stride/metadata instead of guessing from length.
//     let pipeline = gst::parse::launch("appsrc name=source format=time ! videoconvert ! pngenc snapshot=true ! filesink name=output")?
//         .downcast::<gst::Pipeline>().map_err(|_|anyhow!("Invalid PNG pipeline"))?;
//     struct Stop(gst::Pipeline);
//     impl Drop for Stop {
//         fn drop(&mut self) {
//             let _ = self.0.set_state(gst::State::Null);
//         }
//     }
//     let _stop = Stop(pipeline.clone());
//     pipeline
//         .by_name("output")
//         .context("Missing PNG output")?
//         .set_property(
//             "location",
//             directory
//                 .join("decoded-frame.png")
//                 .to_str()
//                 .context("Invalid diagnostic path")?,
//         );
//     let source = pipeline
//         .by_name("source")
//         .context("Missing PNG source")?
//         .downcast::<AppSrc>()
//         .map_err(|_| anyhow!("Invalid PNG source"))?;
//     source.set_caps(Some(&caps));
//     pipeline.set_state(gst::State::Playing)?;
//     let mut frame = buffer.copy();
//     {
//         let frame = frame.make_mut();
//         frame.set_pts(gst::ClockTime::ZERO);
//         frame.set_dts(None);
//     }
//     source.push_buffer(frame)?;
//     source.end_of_stream()?;
//     let message = pipeline
//         .bus()
//         .context("Missing PNG bus")?
//         .timed_pop_filtered(
//             gst::ClockTime::from_nseconds(Duration::from_secs(5).as_nanos() as u64),
//             &[gst::MessageType::Eos, gst::MessageType::Error],
//         )
//         .context("Saving decoded PNG timed out; raw frame and geometry are available")?;
//     if let gst::MessageView::Error(error) = message.view() {
//         return Err(anyhow!("Saving decoded PNG: {}", error.error()));
//     }
//     Ok(directory.to_string_lossy().into_owned())
// }

// #[cfg(test)]
// mod tests {
//     use super::*;
//     #[test]
//     fn capture_preserves_frame_pixels_before_qml_rendering() {
//         gst::init().unwrap();
//         let width = 16_u32;
//         let height = 12_u32;
//         let mut pixels = vec![255_u8; (width * height * 4) as usize];
//         for y in 0..height {
//             for x in 0..width {
//                 if x >= 14 || y >= 9 {
//                     let i = ((y * width + x) * 4) as usize;
//                     pixels[i..i + 3].fill(0);
//                 }
//             }
//         }
//         let caps = gst::Caps::builder("video/x-raw")
//             .field("format", "RGBA")
//             .field("width", width as i32)
//             .field("height", height as i32)
//             .field("framerate", gst::Fraction::new(30, 1))
//             .build();
//         let directory = save_frame(
//             gst::Buffer::from_mut_slice(pixels.clone()),
//             caps,
//             "{\"viewportWidth\":16}",
//         )
//         .unwrap();
//         let path = std::path::Path::new(&directory);
//         assert_eq!(
//             std::fs::read(path.join("decoded-frame.rgba")).unwrap(),
//             pixels
//         );
//         let image = image::ImageReader::open(path.join("decoded-frame.png"))
//             .unwrap()
//             .decode()
//             .unwrap()
//             .to_rgba8();
//         assert_eq!(image.dimensions(), (width, height));
//         assert_eq!(image.into_raw(), pixels);
//         let report: serde_json::Value =
//             serde_json::from_slice(&std::fs::read(path.join("geometry.json")).unwrap()).unwrap();
//         assert_eq!(report["qml_geometry"]["viewportWidth"], 16);
//         std::fs::remove_dir_all(directory).unwrap();
//     }
// }
