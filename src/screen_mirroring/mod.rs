// SPDX-FileCopyrightText: 2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later
mod audio;
// mod diagnostics;
mod input;
mod orientation;
mod playback;
mod session;
mod visible_frame;

use crate::{RUNTIME, qt_threading::QtThreading};
use audio::{AudioEvent, AudioOutput};
use cpp::cpp;
use input::{Command, InputQueue};
use macros::QtThreading;
use playback::Playback;
use qmetaobject::prelude::*;
use std::collections::HashSet;
use std::sync::{
    Arc, LazyLock, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
struct Ui {
    thread: crate::qt_threading::QtThread<ScreenMirroring>,
    alive: Arc<AtomicBool>,
}
impl Ui {
    fn queue(&self, update: impl FnOnce(&mut ScreenMirroring) + Send + 'static) {
        if self.alive.load(Ordering::Acquire) {
            self.thread.queue(update);
        }
    }
}

cpp! {{
    #include <QQuickItem>
    #include <QQuickWindow>
    #include <QPainterPath>
    #include <QRegion>
}}
//remove this shit
static ACTIVE_DEVICES: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Mutex::default);
struct DeviceLease(String);
impl DeviceLease {
    fn acquire(udid: &str) -> anyhow::Result<Self> {
        anyhow::ensure!(
            ACTIVE_DEVICES.lock().unwrap().insert(udid.to_owned()),
            "Screen mirroring is already running for this device"
        );
        Ok(Self(udid.to_owned()))
    }
}
impl Drop for DeviceLease {
    fn drop(&mut self) {
        ACTIVE_DEVICES.lock().unwrap().remove(&self.0);
    }
}

#[allow(non_snake_case)]
#[derive(QObject, Default, QtThreading)]
pub struct ScreenMirroring {
    base: qt_base_class!(trait QObject),
    // 0 stopped, 1 starting, 2 displaying video, 3 stopping, 4 failed.
    state: qt_property!(i32; NOTIFY stateChanged),
    stateChanged: qt_signal!(),
    error: qt_property!(QString; NOTIFY errorChanged),
    errorChanged: qt_signal!(),
    audio_active: qt_property!(bool; NOTIFY audioActiveChanged),
    audioActiveChanged: qt_signal!(),
    audio_error: qt_property!(QString; NOTIFY audioErrorChanged),
    audioErrorChanged: qt_signal!(),
    set_volume: qt_method!(fn(&mut self, volume: f64)),
    volume: f64,
    audio_output: Option<Arc<AudioOutput>>,
    video_width: qt_property!(u32; NOTIFY videoSizeChanged),
    video_height: qt_property!(u32; NOTIFY videoSizeChanged),
    videoSizeChanged: qt_signal!(),
    display_rotation: qt_property!(i32; NOTIFY displayRotationChanged),
    displayRotationChanged: qt_signal!(),
    interface_orientation: i32,
    input_ready: qt_property!(bool; NOTIFY inputReadyChanged),
    inputReadyChanged: qt_signal!(),
    input_error: qt_property!(QString; NOTIFY inputErrorChanged),
    inputErrorChanged: qt_signal!(),
    touch: qt_method!(fn(&self, phase: i32, x: f64, y: f64, width: f64, height: f64) -> bool),
    scroll: qt_method!(fn(&self, x: f64, y: f64, width: f64, height: f64, dx: f64, dy: f64)),
    home: qt_method!(fn(&self)),
    //     capture_diagnostics: qt_method!(fn(&self, geometry: QString)),
    //     diagnosticsReady: qt_signal!(success: bool, detail: QString),
    shape_window: qt_method!(fn(&self, item: QVariant, radius: f64)),
    restore_phone_audio: bool,
    set_restore_phone_audio: qt_method!(fn(&mut self, enabled: bool)),
    silence_phone: qt_method!(fn(&self, silence: bool)),
    app_switcher: qt_method!(fn(&self)),
    release_input: qt_method!(fn(&self)),
    input_queue: Option<Arc<InputQueue>>,
    start: qt_method!(fn(&mut self, udid: QString, video_item: QVariant)),
    stop: qt_method!(fn(&mut self)),
    playback: Option<Arc<Playback>>,
    cancellation: Option<CancellationToken>,
    session_id: u64,
    alive: Arc<AtomicBool>,
}
impl ScreenMirroring {
    fn fail(&mut self, error: anyhow::Error) {
        log::error!("Screen mirroring failed: {error:#}");
        self.error = QString::from(format!("{error:#}"));
        self.state = 4;
        self.errorChanged();
        self.stateChanged();
    }
    fn start(&mut self, udid: QString, video_item: QVariant) {
        if matches!(self.state, 1..=3) {
            return;
        }
        let lease = match DeviceLease::acquire(&udid.to_string()) {
            Ok(lease) => lease,
            Err(error) => {
                self.fail(error);
                return;
            }
        };
        self.session_id = self.session_id.wrapping_add(1);
        let id = self.session_id;
        self.alive.store(true, Ordering::Release);
        let thread = Ui {
            thread: self.qt_thread(),
            alive: self.alive.clone(),
        };
        let frame_thread = thread.clone();
        let size_thread = thread.clone();
        //         let capture_thread = thread.clone();
        //         let capture = Arc::new(diagnostics::Capture::new(move |result| {
        //             capture_thread.queue(move |backend| {
        //                 if backend.session_id != id {
        //                     return;
        //                 }
        //                 let (success, detail) = match result {
        //                     Ok(path) => (true, path),
        //                     Err(error) => (false, error),
        //                 };
        //                 backend.diagnosticsReady(success, QString::from(detail));
        //             });
        //         }));
        let playback = match Playback::new(
            crate::utils::qvariant_to_ptr(video_item),
            move || {
                frame_thread.queue(move |backend| {
                    if backend.session_id == id && backend.state == 1 {
                        backend.state = 2;
                        backend.stateChanged();
                    }
                });
            },
            move |width, height| {
                size_thread.queue(move |backend| {
                    if backend.session_id == id && matches!(backend.state, 1 | 2) {
                        backend.video_width = width;
                        backend.video_height = height;
                        backend.update_rotation();
                        backend.videoSizeChanged();
                    }
                });
            },
            // capture,
        ) {
            Ok(playback) => Arc::new(playback),
            Err(error) => {
                self.fail(error);
                return;
            }
        };
        let audio_thread = thread.clone();
        let audio = Arc::new(AudioOutput::new(self.volume, move |event| {
            audio_thread.queue(move |backend| {
                if backend.session_id != id || !matches!(backend.state, 1 | 2) {
                    return;
                }
                match event {
                    AudioEvent::Playing => backend.audio_active = true,
                    AudioEvent::Failed(error) => {
                        backend.audio_active = false;
                        backend.audio_error = QString::from(error);
                        backend.audioErrorChanged();
                    }
                }
                backend.audioActiveChanged();
            });
        }));
        self.audio_active = false;
        self.audio_error = QString::default();
        self.audioActiveChanged();
        self.audioErrorChanged();
        self.audio_output = Some(audio.clone());
        let input_thread = thread.clone();
        let input = Arc::new(InputQueue::new(move |event| {
            input_thread.queue(move |backend| {
                if backend.session_id != id || !matches!(backend.state, 1 | 2) {
                    return;
                }
                match event {
                    input::Event::Ready => backend.input_ready = true,
                    input::Event::Failed(error) => {
                        log::warn!("Mirroring controls unavailable: {error}");
                        backend.input_ready = false;
                        backend.input_error = QString::from(error);
                        backend.inputErrorChanged();
                    }
                }
                backend.inputReadyChanged();
            });
        }));
        self.input_ready = false;
        self.input_error = QString::default();
        self.inputReadyChanged();
        self.inputErrorChanged();
        input.set_restore_audio(self.restore_phone_audio);
        self.input_queue = Some(input.clone());
        let cancellation = CancellationToken::new();
        self.cancellation = Some(cancellation.clone());
        self.playback = Some(playback.clone());
        self.error = QString::default();
        self.state = 1;
        self.errorChanged();
        self.stateChanged();
        RUNTIME.spawn(async move {
            let orientation_thread = thread.clone();
            let orientation = orientation::Task::start(udid.to_string(), move |degrees| {
                orientation_thread.queue(move |backend| {
                    if backend.session_id == id && matches!(backend.state, 1 | 2) {
                        backend.interface_orientation = degrees;
                        backend.update_rotation();
                    }
                });
            });
            let result =
                session::run(&udid.to_string(), &playback, &audio, &input, &cancellation).await;
            orientation.stop().await;
            audio.stop();
            playback.stop();
            drop(lease);
            thread.queue(move |backend| {
                if backend.session_id != id {
                    return;
                }
                backend.input_queue = None;
                backend.input_ready = false;
                backend.inputReadyChanged();
                backend.playback = None;
                backend.audio_output = None;
                backend.audio_active = false;
                backend.audioActiveChanged();
                backend.cancellation = None;
                if cancellation.is_cancelled() {
                    backend.state = 0;
                    backend.stateChanged();
                } else if let Err(error) = result {
                    backend.fail(error);
                } else {
                    backend.state = 0;
                    backend.stateChanged();
                }
            });
        });
    }
    fn update_rotation(&mut self) {
        let rotation = orientation::video_rotation(
            self.interface_orientation,
            self.video_width,
            self.video_height,
        );
        if rotation != self.display_rotation {
            self.release_input();
            self.display_rotation = rotation;
            self.displayRotationChanged();
        }
    }
    fn touch(&self, phase: i32, x: f64, y: f64, width: f64, height: f64) -> bool {
        if !self.input_ready {
            return false;
        }
        let Some(point) = input::map_rotated_point(
            x,
            y,
            width,
            height,
            self.video_width,
            self.video_height,
            self.display_rotation,
            phase != 0,
        ) else {
            log::debug!(
                "Mirroring touch rejected phase={phase} pointer=({x:.2},{y:.2}) viewport={width:.2}x{height:.2}"
            );
            if phase == 2 {
                self.release_input();
            }
            return false;
        };
        if phase == 0 || phase == 2 {
            log::debug!(
                "Mirroring touch phase={phase} pointer=({x:.2},{y:.2}) viewport={width:.2}x{height:.2} decoded={}x{} rotation={} mapped=({},{})",
                self.video_width,
                self.video_height,
                self.display_rotation,
                point.0,
                point.1
            );
        }
        let command = match phase {
            0 => Command::Down(point),
            1 => Command::Move(point),
            2 => Command::Up(point),
            _ => return false,
        };
        if let Some(queue) = &self.input_queue {
            queue.submit(command);
            return true;
        }
        false
    }
    fn scroll(&self, x: f64, y: f64, width: f64, height: f64, dx: f64, dy: f64) {
        if !self.input_ready || !dx.is_finite() || !dy.is_finite() {
            return;
        }
        if let Some(point) = input::map_rotated_point(
            x,
            y,
            width,
            height,
            self.video_width,
            self.video_height,
            self.display_rotation,
            false,
        ) {
            let (fw, fh) = if matches!(self.display_rotation, 90 | 270) {
                (self.video_height, self.video_width)
            } else {
                (self.video_width, self.video_height)
            };
            let scale = (width / fw as f64).min(height / fh as f64);
            let dx = (dx / (fw as f64 * scale) * 65535.0).clamp(-30000.0, 30000.0) as i32;
            let dy = (dy / (fh as f64 * scale) * 65535.0).clamp(-30000.0, 30000.0) as i32;
            let (dx, dy) = input::inverse_delta(dx, dy, self.display_rotation);
            if dx == 0 && dy == 0 {
                return;
            }
            if let Some(queue) = &self.input_queue {
                queue.submit(Command::Scroll(point, dx, dy));
            }
        }
    }
    fn home(&self) {
        if self.input_ready
            && let Some(queue) = &self.input_queue
        {
            queue.submit(Command::Home);
        }
    }
    //     fn capture_diagnostics(&self, geometry: QString) {
    //         if let Some(playback) = &self.playback {
    //             playback.capture_frame(geometry.to_string());
    //         }
    //     }
    // TODO: this might be unnecessary
    fn shape_window(&self, item: QVariant, radius: f64) {
        let item = crate::utils::qvariant_to_ptr(item);
        if item == 0 || !radius.is_finite() {
            return;
        }
        unsafe {
            cpp!([item as "size_t", radius as "double"] {
                auto quickItem = reinterpret_cast<QQuickItem*>(item);
                if (auto window = quickItem->window()) {
                    QPainterPath path;
                    path.addRoundedRect(QRectF(0,0,window->width(),window->height()),radius,radius);
                    window->setMask(QRegion(path.toFillPolygon().toPolygon()));
                }
            });
        }
    }
    fn set_restore_phone_audio(&mut self, enabled: bool) {
        self.restore_phone_audio = enabled;
        if let Some(queue) = &self.input_queue {
            queue.set_restore_audio(enabled);
        }
    }
    fn silence_phone(&self, silence: bool) {
        if self.input_ready
            && let Some(queue) = &self.input_queue
        {
            queue.submit(Command::PhoneVolume(silence));
        }
    }
    fn app_switcher(&self) {
        if self.input_ready
            && let Some(queue) = &self.input_queue
        {
            queue.submit(Command::Switcher);
        }
    }
    fn release_input(&self) {
        if let Some(queue) = &self.input_queue {
            queue.submit(Command::Release);
        }
    }
    fn set_volume(&mut self, volume: f64) {
        if !volume.is_finite() {
            return;
        }
        self.volume = volume.clamp(0.0, 1.0);
        if let Some(audio) = &self.audio_output {
            audio.set_volume(self.volume);
        }
    }
    fn stop(&mut self) {
        self.release_input();
        self.input_ready = false;
        self.inputReadyChanged();
        if let Some(audio) = self.audio_output.take() {
            audio.stop();
        }
        self.audio_active = false;
        self.audioActiveChanged();
        if let Some(cancellation) = &self.cancellation {
            cancellation.cancel();
            self.state = 3;
            self.stateChanged();
        }
        // Stop synchronously before the QML-owned video item can be destroyed.
        if let Some(playback) = self.playback.take() {
            playback.stop();
        }
    }
}
impl Drop for ScreenMirroring {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Release);
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn one_session_per_device_until_cleanup_finishes() {
        let lease = DeviceLease::acquire("mirroring-lease-test").unwrap();
        assert!(DeviceLease::acquire("mirroring-lease-test").is_err());
        drop(lease);
        assert!(DeviceLease::acquire("mirroring-lease-test").is_ok());
    }
}
