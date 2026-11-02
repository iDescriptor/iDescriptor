// SPDX-FileCopyrightText: 2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later
use anyhow::Context;
use idevice::{
    IdeviceService,
    springboardservices::{InterfaceOrientation, SpringBoardServicesClient},
};
use tokio::time::{Duration, timeout};
use tokio_util::sync::CancellationToken;

fn degrees(orientation: InterfaceOrientation) -> Option<i32> {
    // SpringBoard's interface-orientation names differ from devicecontrol's
    // physical orientation names. Home button to the right means CCW on screen.
    match orientation {
        InterfaceOrientation::Portrait => Some(0),
        InterfaceOrientation::PortraitUpsideDown => Some(180),
        InterfaceOrientation::LandscapeRight => Some(270),
        InterfaceOrientation::LandscapeLeft => Some(90),
        InterfaceOrientation::Unknown => None,
    }
}
pub fn video_rotation(orientation: i32, width: u32, height: u32) -> i32 {
    // Some devices emit already-oriented landscape frames. Do not rotate them
    // again, as in pymobiledevice3's viewer drawPending() dimension check.
    if matches!(orientation, 90 | 270) && width > height {
        0
    } else {
        orientation
    }
}
pub struct Task {
    cancellation: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}
impl Task {
    pub fn start(udid: String, changed: impl Fn(i32) + Send + Sync + 'static) -> Self {
        let cancellation = CancellationToken::new();
        let stop = cancellation.clone();
        let task = crate::RUNTIME.spawn(async move {
            let watch = async {
                let device = crate::device_ctx::get_device(udid).await?;
                let mut client = None;
                let mut last = None;
                let mut warned = false;
                loop {
                    // Drop a timed-out connection before reconnecting: an old
                    // pending plist reply must not become the next poll's result.
                    let result = timeout(Duration::from_secs(5), async {
                        if client.is_none() {
                            let provider = device.provider.lock().await;
                            client =
                                Some(SpringBoardServicesClient::connect(provider.as_ref()).await?);
                        }
                        Ok::<_, anyhow::Error>(
                            client.as_mut().unwrap().get_interface_orientation().await?,
                        )
                    })
                    .await
                    .context("Reading interface orientation timed out")
                    .and_then(|result| result);
                    match result {
                        Ok(orientation) => {
                            warned = false;
                            if let Some(value) = degrees(orientation)
                                && last != Some(value)
                            {
                                last = Some(value);
                                log::debug!(
                                    "Mirroring interface orientation changed to {value} degrees"
                                );
                                changed(value);
                            }
                            tokio::time::sleep(Duration::from_millis(500)).await;
                        }
                        Err(error) => {
                            client = None;
                            if !warned {
                                log::warn!(
                                    "Could not read mirroring orientation; retrying: {error:#}"
                                );
                                warned = true;
                            }
                            tokio::time::sleep(Duration::from_secs(2)).await;
                        }
                    }
                }
                #[allow(unreachable_code)]
                Ok::<(), anyhow::Error>(())
            };
            tokio::select! {
                biased;
                _ = stop.cancelled() => {},
                result = watch => if let Err(error) = result {
                    log::warn!("Could not follow mirroring orientation: {error:#}");
                }
            }
        });
        Self { cancellation, task }
    }
    pub async fn stop(mut self) {
        self.cancellation.cancel();
        if timeout(Duration::from_secs(3), &mut self.task)
            .await
            .is_err()
        {
            self.task.abort();
            let _ = (&mut self.task).await;
        }
    }
}
impl Drop for Task {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interface_orientation_uses_the_home_button_side() {
        assert_eq!(degrees(InterfaceOrientation::LandscapeRight), Some(270));
        assert_eq!(degrees(InterfaceOrientation::LandscapeLeft), Some(90));
        assert_eq!(degrees(InterfaceOrientation::Unknown), None);
        assert_eq!(degrees(InterfaceOrientation::PortraitUpsideDown), Some(180));
    }
    #[test]
    fn landscape_frames_are_not_rotated_twice() {
        assert_eq!(video_rotation(270, 600, 1200), 270);
        assert_eq!(video_rotation(270, 1200, 600), 0);
        assert_eq!(video_rotation(90, 1200, 600), 0);
        assert_eq!(video_rotation(0, 600, 1200), 0);
    }
}
