// SPDX-FileCopyrightText: 2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later
use anyhow::{Context, Result, ensure};
use idevice::{
    ReadWrite, RsdService,
    core_device::{
        ButtonState, IndigoHidClient, TOUCHSCREEN_STATE_CONTACT, TOUCHSCREEN_STATE_RELEASE,
        UniversalHidServiceClient,
    },
    rsd::RsdHandshake,
    tcp::handle::AdapterHandle,
};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::{
    sync::Notify,
    time::{Duration, sleep, timeout},
};
use tokio_util::sync::CancellationToken;

type TouchClient = UniversalHidServiceClient<Box<dyn ReadWrite>>;
type ButtonClient = IndigoHidClient<Box<dyn ReadWrite>>;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point(pub u16, pub u16);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Down(Point),
    Move(Point),
    Up(Point),
    Scroll(Point, i32, i32),
    Home,
    Switcher,
    PhoneVolume(bool),
    Release,
}
pub enum Event {
    Ready,
    Failed(String),
}
pub struct InputQueue {
    queue: Mutex<VecDeque<Command>>,
    notify: Notify,
    event: Box<dyn Fn(Event) + Send + Sync>,
    restore_audio: AtomicBool,
}
impl InputQueue {
    pub fn new(event: impl Fn(Event) + Send + Sync + 'static) -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            notify: Notify::new(),
            event: Box::new(event),
            restore_audio: AtomicBool::new(false),
        }
    }
    pub fn set_restore_audio(&self, enabled: bool) {
        self.restore_audio.store(enabled, Ordering::Release);
    }
    pub fn submit(&self, command: Command) {
        let mut queue = self.queue.lock().unwrap();
        if matches!(command, Command::Release) {
            queue.clear();
        }
        let command = match (command, queue.back().copied()) {
            (Command::Scroll(point, dx, dy), Some(Command::Scroll(_, old_x, old_y))) => {
                queue.pop_back();
                Command::Scroll(
                    point,
                    old_x.saturating_add(dx).clamp(-30000, 30000),
                    old_y.saturating_add(dy).clamp(-30000, 30000),
                )
            }
            _ => command,
        };
        if matches!(command, Command::Move(_)) && matches!(queue.back(), Some(Command::Move(_))) {
            queue.pop_back();
        }
        // Never lose a release behind a full queue. A reset is safer than
        // replaying a backlog of stale gestures or leaving a finger held down.
        if queue.len() >= 64 {
            queue.clear();
            queue.push_back(Command::Release);
        } else {
            queue.push_back(command);
        }
        drop(queue);
        self.notify.notify_one();
    }
    async fn next(&self) -> Command {
        loop {
            let notified = self.notify.notified();
            if let Some(command) = self.queue.lock().unwrap().pop_front() {
                return command;
            }
            notified.await;
        }
    }
}

// Same aspect-fit rectangle used by qml6glsink. Input outside the displayed
// image is ignored on press, and clamped when finishing an existing drag.
pub fn map_point(
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    frame_width: u32,
    frame_height: u32,
    clamp: bool,
) -> Option<Point> {
    if ![x, y, width, height].iter().all(|n| n.is_finite())
        || width <= 0.0
        || height <= 0.0
        || frame_width == 0
        || frame_height == 0
    {
        return None;
    }
    let scale = (width / frame_width as f64).min(height / frame_height as f64);
    let painted_width = frame_width as f64 * scale;
    let painted_height = frame_height as f64 * scale;
    let nx = (x - (width - painted_width) / 2.0) / painted_width;
    let ny = (y - (height - painted_height) / 2.0) / painted_height;
    if !clamp && (!(0.0..=1.0).contains(&nx) || !(0.0..=1.0).contains(&ny)) {
        return None;
    }
    Some(Point(
        (nx.clamp(0.0, 1.0) * 65535.0).round() as u16,
        (ny.clamp(0.0, 1.0) * 65535.0).round() as u16,
    ))
}
pub fn map_rotated_point(
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    frame_width: u32,
    frame_height: u32,
    rotation: i32,
    clamp: bool,
) -> Option<Point> {
    let sideways = matches!(rotation, 90 | 270);
    let (fw, fh) = if sideways {
        (frame_height, frame_width)
    } else {
        (frame_width, frame_height)
    };
    let Point(x, y) = map_point(x, y, width, height, fw, fh, clamp)?;
    Some(match rotation {
        90 => Point(y, 65535 - x),
        180 => Point(65535 - x, 65535 - y),
        270 => Point(65535 - y, x),
        _ => Point(x, y),
    })
}
pub fn inverse_delta(dx: i32, dy: i32, rotation: i32) -> (i32, i32) {
    match rotation {
        90 => (dy, -dx),
        180 => (-dx, -dy),
        270 => (-dy, dx),
        _ => (dx, dy),
    }
}
struct Controller {
    touch: TouchClient,
    buttons: ButtonClient,
    held: Option<Point>,
    home_held: bool,
    volume_held: Option<u64>,
}
impl Controller {
    async fn release(&mut self) -> Result<()> {
        // Attempt both releases even if one connection has failed.
        let touch = if let Some(point) = self.held {
            let result = self
                .touch
                .send_touchscreen(TOUCHSCREEN_STATE_RELEASE, point.0, point.1, None)
                .await;
            if result.is_ok() {
                self.held = None;
            }
            result
        } else {
            Ok(())
        };
        let button = if self.home_held {
            let result = self.buttons.send_button(0x0c, 0x40, ButtonState::Up).await;
            if result.is_ok() {
                self.home_held = false;
            }
            result
        } else {
            Ok(())
        };
        let volume = if let Some(code) = self.volume_held {
            let result = self.buttons.send_button(0x0c, code, ButtonState::Up).await;
            if result.is_ok() {
                self.volume_held = None;
            }
            result
        } else {
            Ok(())
        };
        touch?;
        button?;
        volume?;
        Ok(())
    }
    async fn contact(&mut self, point: Point) -> Result<()> {
        self.held = Some(point);
        self.touch
            .send_touchscreen(TOUCHSCREEN_STATE_CONTACT, point.0, point.1, None)
            .await?;
        Ok(())
    }
    async fn drag(&mut self, from: Point, to: Point, hold: bool) -> Result<()> {
        self.release().await?;
        for i in 0..=16 {
            let interpolate = |a: u16, b: u16| (a as i32 + (b as i32 - a as i32) * i / 16) as u16;
            self.contact(Point(interpolate(from.0, to.0), interpolate(from.1, to.1)))
                .await?;
            sleep(Duration::from_millis(12)).await;
        }
        if hold {
            sleep(Duration::from_millis(450)).await;
        }
        self.release().await
    }
    async fn press_home(&mut self) -> Result<()> {
        self.release().await?;
        self.home_held = true;
        self.buttons
            .send_button(0x0c, 0x40, ButtonState::Down)
            .await?;
        sleep(Duration::from_millis(50)).await;
        self.release().await
    }
    async fn command(&mut self, command: Command) -> Result<()> {
        match command {
            Command::Down(point) => {
                self.release().await?;
                self.contact(point).await?;
                log::debug!("Mirroring touch CONTACT sent ({},{})", point.0, point.1);
            }
            Command::Move(point) if self.held.is_some() => self.contact(point).await?,
            Command::Up(point) if self.held.is_some() => {
                self.contact(point).await?;
                self.release().await?;
                log::debug!("Mirroring touch RELEASE sent ({},{})", point.0, point.1);
            }
            Command::PhoneVolume(silence) => {
                self.release().await?;
                let code = if silence { 0xea } else { 0xe9 };
                for _ in 0..20 {
                    self.volume_held = Some(code);
                    self.buttons
                        .send_button(0x0c, code, ButtonState::Down)
                        .await?;
                    sleep(Duration::from_millis(50)).await;
                    self.release().await?;
                    sleep(Duration::from_millis(20)).await;
                }
            }
            Command::Home => self.press_home().await?,
            Command::Switcher => {
                // Reuse the same Consumer/Menu HID event as pymobiledevice3's
                // Home button, twice. An ordinary touchscreen swipe does not
                // reliably activate SpringBoard's system app-switcher gesture.
                self.press_home().await?;
                sleep(Duration::from_millis(100)).await;
                self.press_home().await?;
            }
            Command::Scroll(point, dx, dy) if self.held.is_none() => {
                let x = point.0.clamp(12000, 53535);
                let y = point.1.clamp(12000, 53535);
                let to = Point(
                    (x as i32 + dx).clamp(1000, 64535) as u16,
                    (y as i32 + dy).clamp(1000, 64535) as u16,
                );
                self.drag(Point(x, y), to, false).await?;
            }
            Command::Release => self.release().await?,
            _ => {}
        }
        Ok(())
    }
}

pub async fn run(
    mut adapter: AdapterHandle,
    mut handshake: RsdHandshake,
    queue: Arc<InputQueue>,
    cancellation: CancellationToken,
) {
    let connect = async {
        ensure!(
            handshake
                .services
                .contains_key("com.apple.coredevice.hid.universalhidservice"),
            "Device does not advertise touchscreen input"
        );
        let touch = UniversalHidServiceClient::connect_rsd(&mut adapter, &mut handshake).await?;
        let buttons = IndigoHidClient::connect_rsd(&mut adapter, &mut handshake).await?;
        Ok::<_, anyhow::Error>(Controller {
            touch,
            buttons,
            held: None,
            home_held: false,
            volume_held: None,
        })
    };
    let mut controller = tokio::select! {
        biased;
        _ = cancellation.cancelled() => return,
        result = timeout(Duration::from_secs(10),connect) => match result.context("Connecting device input timed out").and_then(|r|r) {
            Ok(controller) => controller,
            Err(error) => { (queue.event)(Event::Failed(format!("{error:#}"))); return; }
        }
    };
    (queue.event)(Event::Ready);
    let result: Result<()> = tokio::select! {
        biased;
        _ = cancellation.cancelled() => Ok(()),
        result = async {
            loop {
                let command = queue.next().await;
                timeout(Duration::from_secs(5),controller.command(command)).await.context("Device input timed out")??;
            }
        } => result,
    };
    // Restore while the media session still authenticates HID, before closing
    // the tunnel. This runs even when the Qt owner has already been destroyed.
    if queue.restore_audio.load(Ordering::Acquire) {
        match timeout(
            Duration::from_secs(3),
            controller.command(Command::PhoneVolume(false)),
        )
        .await
        {
            Ok(Ok(())) => log::info!("Enabled iPhone audio after mirroring ended"),
            Ok(Err(error)) => {
                log::warn!("Could not enable iPhone audio after disconnect: {error:#}")
            }
            Err(_) => log::warn!("Enabling iPhone audio after disconnect timed out"),
        }
    }
    match timeout(Duration::from_secs(2), controller.release()).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => log::warn!("Could not release device input: {error:#}"),
        Err(error) => log::warn!("Device input release timed out: {error}"),
    }
    if let Err(error) = result {
        (queue.event)(Event::Failed(format!("{error:#}")));
    }
    log::debug!("Screen mirroring input stopped");
}

pub struct InputTask {
    cancellation: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}
impl InputTask {
    pub fn start(adapter: AdapterHandle, handshake: RsdHandshake, queue: Arc<InputQueue>) -> Self {
        let cancellation = CancellationToken::new();
        let task = crate::RUNTIME.spawn(run(adapter, handshake, queue, cancellation.clone()));
        Self { cancellation, task }
    }
    pub async fn stop(mut self) {
        self.cancellation.cancel();
        if timeout(Duration::from_secs(6), &mut self.task)
            .await
            .is_err()
        {
            self.task.abort();
            let _ = (&mut self.task).await;
        }
    }
}
impl Drop for InputTask {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn input_mapping_accounts_for_bars_and_clamps_releases() {
        assert_eq!(
            map_point(50.0, 100.0, 100.0, 200.0, 100, 100, false),
            Some(Point(32768, 32768))
        );
        assert_eq!(map_point(50.0, 10.0, 100.0, 200.0, 100, 100, false), None);
        assert_eq!(
            map_point(50.0, 10.0, 100.0, 200.0, 100, 100, true),
            Some(Point(32768, 0))
        );
        assert_eq!(map_point(10.0, 50.0, 200.0, 100.0, 100, 100, false), None);
        assert_eq!(
            map_point(f64::NAN, 0.0, 100.0, 100.0, 100, 100, false),
            None
        );
    }
    #[test]
    fn moves_coalesce_without_losing_touch_boundaries() {
        let queue = InputQueue::new(|_| {});
        queue.submit(Command::Down(Point(1, 1)));
        queue.submit(Command::Move(Point(2, 2)));
        queue.submit(Command::Move(Point(3, 3)));
        queue.submit(Command::Up(Point(4, 4)));
        assert_eq!(
            *queue.queue.lock().unwrap(),
            VecDeque::from([
                Command::Down(Point(1, 1)),
                Command::Move(Point(3, 3)),
                Command::Up(Point(4, 4))
            ])
        );
        queue.submit(Command::Release);
        assert_eq!(
            *queue.queue.lock().unwrap(),
            VecDeque::from([Command::Release])
        );
    }
    #[test]
    fn queue_overflow_resets_input_instead_of_losing_a_release() {
        let queue = InputQueue::new(|_| {});
        for _ in 0..64 {
            queue.submit(Command::Home);
        }
        queue.submit(Command::Up(Point(10, 20)));
        assert_eq!(
            *queue.queue.lock().unwrap(),
            VecDeque::from([Command::Release])
        );
    }
    #[test]
    fn wheel_moves_coalesce_without_crossing_other_commands() {
        let queue = InputQueue::new(|_| {});
        queue.submit(Command::Scroll(Point(100, 200), 0, 1000));
        queue.submit(Command::Scroll(Point(100, 200), 0, 2000));
        queue.submit(Command::Home);
        queue.submit(Command::Scroll(Point(100, 200), 0, 4000));
        assert_eq!(
            *queue.queue.lock().unwrap(),
            VecDeque::from([
                Command::Scroll(Point(100, 200), 0, 3000),
                Command::Home,
                Command::Scroll(Point(100, 200), 0, 4000)
            ])
        );
    }
    #[test]
    fn rotated_touch_coordinates_match_the_original_device_buffer() {
        assert_eq!(
            map_rotated_point(0.0, 0.0, 200.0, 100.0, 100, 200, 90, false),
            Some(Point(0, 65535))
        );
        assert_eq!(
            map_rotated_point(200.0, 100.0, 200.0, 100.0, 100, 200, 90, false),
            Some(Point(65535, 0))
        );
        assert_eq!(
            map_rotated_point(0.0, 0.0, 200.0, 100.0, 100, 200, 270, false),
            Some(Point(65535, 0))
        );
        assert_eq!(
            map_rotated_point(200.0, 100.0, 200.0, 100.0, 100, 200, 270, false),
            Some(Point(0, 65535))
        );
        assert_eq!(
            map_rotated_point(0.0, 0.0, 100.0, 200.0, 100, 200, 180, false),
            Some(Point(65535, 65535))
        );
        assert_eq!(
            map_rotated_point(0.0, 10.0, 300.0, 100.0, 100, 200, 90, false),
            None
        );
        assert_eq!(inverse_delta(10, 20, 90), (20, -10));
        assert_eq!(inverse_delta(10, 20, 180), (-10, -20));
        assert_eq!(inverse_delta(10, 20, 270), (-20, 10));
    }

    #[tokio::test]
    async fn switcher_sends_two_complete_home_presses() {
        use tokio::io::AsyncReadExt;
        let (touch_socket, _touch_peer) = tokio::io::duplex(16384);
        let (button_socket, mut button_peer) = tokio::io::duplex(16384);
        let touch = UniversalHidServiceClient::new(
            idevice::RemoteXpcClient::new(Box::new(touch_socket) as Box<dyn ReadWrite>)
                .await
                .unwrap(),
        );
        let buttons = IndigoHidClient::new(
            idevice::RemoteXpcClient::new(Box::new(button_socket) as Box<dyn ReadWrite>)
                .await
                .unwrap(),
        );
        let mut controller = Controller {
            touch,
            buttons,
            held: None,
            home_held: false,
            volume_held: None,
        };
        let sender = async {
            controller.command(Command::Switcher).await.unwrap();
        };
        let receiver = async {
            let mut magic = [0_u8; 24];
            button_peer.read_exact(&mut magic).await.unwrap();
            for state in [1_u64, 2, 1, 2] {
                let mut header = [0_u8; 9];
                button_peer.read_exact(&mut header).await.unwrap();
                assert_eq!(header[3], 0); // HTTP/2 DATA
                let length = u32::from_be_bytes([0, header[0], header[1], header[2]]) as usize;
                let mut bytes = vec![0; length];
                button_peer.read_exact(&mut bytes).await.unwrap();
                let message = idevice::xpc::XPCMessage::decode(&bytes)
                    .unwrap()
                    .message
                    .unwrap()
                    .to_plist();
                let payload = message
                    .as_dictionary()
                    .unwrap()
                    .get("payload")
                    .unwrap()
                    .as_dictionary()
                    .unwrap();
                assert_eq!(
                    payload.get("state").unwrap().as_unsigned_integer(),
                    Some(state)
                );
                assert_eq!(
                    payload.get("usagePage").unwrap().as_unsigned_integer(),
                    Some(0x0c)
                );
                assert_eq!(
                    payload.get("usageCode").unwrap().as_unsigned_integer(),
                    Some(0x40)
                );
            }
        };
        timeout(Duration::from_secs(2), async {
            tokio::join!(sender, receiver);
        })
        .await
        .unwrap();
        assert!(controller.held.is_none());
        assert!(!controller.home_held);
    }
    #[tokio::test]
    async fn phone_volume_sends_complete_down_and_up_presses() {
        use tokio::io::AsyncReadExt;
        let (touch_socket, _touch_peer) = tokio::io::duplex(16384);
        let (button_socket, mut button_peer) = tokio::io::duplex(16384);
        let touch = UniversalHidServiceClient::new(
            idevice::RemoteXpcClient::new(Box::new(touch_socket) as Box<dyn ReadWrite>)
                .await
                .unwrap(),
        );
        let buttons = IndigoHidClient::new(
            idevice::RemoteXpcClient::new(Box::new(button_socket) as Box<dyn ReadWrite>)
                .await
                .unwrap(),
        );
        let mut controller = Controller {
            touch,
            buttons,
            held: None,
            home_held: false,
            volume_held: None,
        };
        let sender = async {
            controller
                .command(Command::PhoneVolume(true))
                .await
                .unwrap();
            controller
                .command(Command::PhoneVolume(false))
                .await
                .unwrap();
        };
        let receiver = async {
            let mut magic = [0_u8; 24];
            button_peer.read_exact(&mut magic).await.unwrap();
            for (code, state) in [0xea_u64, 0xe9]
                .into_iter()
                .flat_map(|code| (0..20).flat_map(move |_| [(code, 1_u64), (code, 2)]))
            {
                let mut header = [0_u8; 9];
                button_peer.read_exact(&mut header).await.unwrap();
                assert_eq!(header[3], 0); // HTTP/2 DATA
                let length = u32::from_be_bytes([0, header[0], header[1], header[2]]) as usize;
                let mut bytes = vec![0; length];
                button_peer.read_exact(&mut bytes).await.unwrap();
                let message = idevice::xpc::XPCMessage::decode(&bytes)
                    .unwrap()
                    .message
                    .unwrap()
                    .to_plist();
                let payload = message
                    .as_dictionary()
                    .unwrap()
                    .get("payload")
                    .unwrap()
                    .as_dictionary()
                    .unwrap();
                assert_eq!(
                    payload.get("state").unwrap().as_unsigned_integer(),
                    Some(state)
                );
                assert_eq!(
                    payload.get("usagePage").unwrap().as_unsigned_integer(),
                    Some(0x0c)
                );
                assert_eq!(
                    payload.get("usageCode").unwrap().as_unsigned_integer(),
                    Some(code)
                );
            }
        };
        timeout(Duration::from_secs(5), async {
            tokio::join!(sender, receiver);
        })
        .await
        .unwrap();
        assert!(controller.held.is_none());
        assert!(!controller.home_held);
        assert!(controller.volume_held.is_none());
    }
}
