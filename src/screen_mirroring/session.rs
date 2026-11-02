// SPDX-FileCopyrightText: 2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later
use super::{
    audio::AudioOutput,
    input::{InputQueue, InputTask},
    playback::Playback,
};
use anyhow::{Context, Result, ensure};
use idevice::{
    IdeviceService, ReadWrite, RsdService,
    core_device::{
        CallInfoBlob, DisplayServiceClient, HevcDepacketizer, ReportBlock, RtpPacket,
        build_frame_ack, build_keyframe_request, build_liveness, build_screen_audio_offer,
        build_screen_video_offer, build_start_audio_parameters, build_start_video_parameters,
    },
    core_device_proxy::CoreDeviceProxy,
    rsd::RsdHandshake,
    tcp::handle::{AdapterHandle, UdpSocketHandle},
};
use std::sync::Arc;
use tokio::time::{Duration, Instant, timeout};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

type Client = DisplayServiceClient<Box<dyn ReadWrite>>;
const FEATURES: u64 = 140; // Controller features, as in upstream's Device Hub offer.

pub async fn run(
    udid: &str,
    playback: &Playback,
    audio_output: &AudioOutput,
    input: &Arc<InputQueue>,
    cancellation: &CancellationToken,
) -> Result<()> {
    let setup = async {
        let device = crate::device_ctx::get_device(udid.to_owned()).await?;
        ensure!(
            device.ios_version.major >= 27,
            "Screen mirroring requires iOS 27 or later; use Live Screen on older devices"
        );
        let proxy = {
            let provider = device.provider.lock().await;
            CoreDeviceProxy::connect(provider.as_ref())
                .await
                .context("Could not open the CoreDevice software tunnel for mirroring")?
        };
        let port = proxy.tunnel_info().server_rsd_port;
        let mut adapter = proxy.create_software_tunnel()?.to_async_handle();
        let stream = adapter.connect(port).await?;
        let mut handshake = RsdHandshake::new(stream).await?;
        ensure!(
            handshake
                .services
                .contains_key("com.apple.coredevice.displayservice"),
            "This device does not advertise Screen Mirroring. Enable Developer Mode and prepare its developer image, or use Live Screen"
        );
        let client = DisplayServiceClient::connect_rsd(&mut adapter, &mut handshake).await?;
        Ok::<_, anyhow::Error>((adapter, handshake, client))
    };
    let (mut adapter, mut handshake, mut client) = tokio::select! {
        biased;
        _ = cancellation.cancelled() => return Ok(()),
        result = timeout(Duration::from_secs(30), setup) => result.context("Connecting to Screen Mirroring timed out")??,
    };
    let mut started = false;
    let mut input_task = None;
    let result = tokio::select! {
        biased;
        _ = cancellation.cancelled() => Ok(()),
        result = stream(&mut adapter, &mut handshake, &mut client, &mut started, playback, audio_output, input, &mut input_task) => result,
    };
    if let Some(task) = input_task {
        task.stop().await;
    }
    // Keep the adapter alive through shutdown, including partially started sessions.
    // No stopAll is sent unless we successfully started our own session.
    if started {
        match timeout(Duration::from_secs(3), client.stop_media_stream()).await {
            Ok(Ok(_)) => log::debug!("Screen mirroring session stopped"),
            error => log::warn!("Could not confirm screen mirroring shutdown: {error:?}"),
        }
    }
    drop(client);
    if let Err(error) = adapter.close().await {
        log::debug!("Developer tunnel already closed: {error}");
    }
    result
}

async fn stream(
    adapter: &mut AdapterHandle,
    handshake: &mut RsdHandshake,
    client: &mut Client,
    started: &mut bool,
    playback: &Playback,
    audio_output: &AudioOutput,
    input: &Arc<InputQueue>,
    input_task: &mut Option<InputTask>,
) -> Result<()> {
    // Read authoritative display metadata on its own connection. Failure does
    // not block mirroring, and no crop is inferred from a black-pixel scan.
    if let Some(service) = handshake.services.get("com.apple.coredevice.deviceinfo") {
        let query = async {
            let stream = adapter.connect(service.port).await?;
            let mut client = idevice::core_device::CoreDeviceServiceClient::new(stream).await?;
            Ok::<_, anyhow::Error>(
                client
                    .invoke_with_plist(
                        "com.apple.coredevice.feature.getdisplayinfo",
                        plist::Dictionary::new(),
                    )
                    .await?,
            )
        };
        match timeout(Duration::from_secs(3), query).await {
            Ok(Ok(value)) => {
                log::info!("Mirroring device display metadata: {value:?}");
                playback.diagnostic_reply("device-display-info", value);
            }
            result => {
                let detail = format!("Display metadata query failed: {result:?}");
                log::warn!("{detail}");
                playback.diagnostic_reply("device-display-info", plist::Value::String(detail));
            }
        }
    }
    let audio = adapter.bind_udp(0).await?;
    let video = adapter.bind_udp(0).await?;
    let receiver = adapter.host_ip().to_string();
    let sender = adapter.peer_ip().to_string();
    let call_info = CallInfoBlob {
        call_id: 0,
        client_version: 1,
        device_type: "Mac17,7".into(),
        framework_version: "2205.3.1".into(),
        os_version: "25F71".into(),
        device_name: None,
        audio_device_uid: None,
    };
    let session_id = Uuid::new_v4();
    // Device Hub establishes audio first and groups it with the video session.
    let audio_params = build_start_audio_parameters(
        &receiver,
        audio.local_port(),
        &sender,
        50000,
        build_screen_audio_offer(&Uuid::new_v4().to_string().to_uppercase(), &call_info)?,
        FEATURES,
        session_id,
    );
    let audio_answer = timeout(
        Duration::from_secs(20),
        client.start_media_stream(audio_params),
    )
    .await
    .context("Starting the media session timed out")??;
    *started = true;
    let mut audio_feedback = Feedback::from_answer(&audio_answer)?;
    let ssrc = Uuid::new_v4().as_u128() as u32;
    let video_params = build_start_video_parameters(
        &receiver,
        video.local_port(),
        &sender,
        50001,
        build_screen_video_offer(&Uuid::new_v4().to_string().to_uppercase(), &call_info, ssrc)?,
        FEATURES,
        1,
        session_id,
    );
    let answer = timeout(
        Duration::from_secs(20),
        client.start_media_stream(video_params),
    )
    .await
    .context("Starting device video timed out")??;
    // playback.diagnostic_reply("video-negotiation", answer.clone());
    let mut feedback = Feedback::from_answer(&answer)?;
    log::info!("Screen mirroring media session started");
    playback.start()?;
    if let Err(error) = audio_output.start(&audio_answer) {
        audio_output.fail(error);
    }
    *input_task = Some(InputTask::start(
        adapter.clone(),
        handshake.clone(),
        input.clone(),
    ));
    let mut depacketizer = HevcDepacketizer::new();
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut rate_tick = tokio::time::interval(Duration::from_millis(100));
    rate_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let start = Instant::now();
    let mut last_video = Instant::now();
    let mut last_keyframe = Instant::now() - Duration::from_secs(2);
    let mut received_video = false;
    loop {
        tokio::select! {
            datagram = audio.recv() => {
                let datagram = datagram.context("Device audio transport closed")?;
                if !idevice::core_device::is_rtcp(&datagram.data)
                    && let Some(packet) = RtpPacket::parse(&datagram.data) {
                    if packet.ssrc == audio_feedback.remote_ssrc {
                        audio_feedback.observe(&packet);
                        audio_output.push(&packet);
                    }
                }
            }
            datagram = video.recv() => {
                let datagram = datagram.context("Device video transport closed")?;
                if idevice::core_device::is_rtcp(&datagram.data) { continue; }
                let Some(packet) = RtpPacket::parse(&datagram.data) else { continue; };
                if packet.ssrc != feedback.remote_ssrc { continue; }
                received_video = true;
                last_video = Instant::now();
                let gap = feedback.observe(&packet);
                if gap && last_keyframe.elapsed() >= Duration::from_secs(1) {
                    feedback.request_keyframe(&video).await?;
                    last_keyframe = Instant::now();
                }
                depacketizer.push(packet.sequence_number, packet.timestamp, packet.payload);
                let bytes = depacketizer.take_output();
                if !bytes.is_empty() { playback.push(bytes)?; }
                if packet.marker {
                    feedback.send(&video, build_frame_ack(feedback.local_ssrc, packet.timestamp)).await?;
                }
            }
            _ = rate_tick.tick(), if feedback.highest_seq.is_some() => {
                feedback.send(&video, feedback.rate_control()).await?;
            }
            _ = tick.tick() => {
                playback.check_error()?;
                audio_output.check_error();
                ensure!(received_video || start.elapsed() < Duration::from_secs(20),
                    "The device started a session but did not send video. Unlock the device and retry");
                ensure!(playback.has_frame() || start.elapsed() < Duration::from_secs(25),
                    "Received device video but could not decode a frame. Check the GStreamer HEVC decoder");
                audio_feedback.keep_alive(&audio).await?;
                feedback.keep_alive(&video).await?;
                if !playback.has_frame() || last_video.elapsed() > Duration::from_secs(5) {
                    feedback.request_keyframe(&video).await?;
                }
            }
        }
    }
}

struct Feedback {
    local_ssrc: u32,
    remote_ssrc: u32,
    port: u16,
    highest_seq: Option<u32>,
    fir_seq: u8,
    packets: u32,
    frame_packets: u32,
    last_frame_packets: u32,
    timestamp: u32,
}
impl Feedback {
    fn from_answer(answer: &plist::Value) -> Result<Self> {
        let config = answer
            .as_dictionary()
            .and_then(|a| a.get("connection"))
            .and_then(plist::Value::as_dictionary)
            .and_then(|c| c.get("streamConfig"))
            .and_then(plist::Value::as_dictionary)
            .context("Device returned no media stream configuration")?;
        let number = |key| -> Result<u64> {
            config
                .get(key)
                .and_then(plist::Value::as_unsigned_integer)
                .with_context(|| format!("Media configuration is missing {key}"))
        };
        let value = Self {
            local_ssrc: number("RemoteSSRC")?.try_into()?,
            remote_ssrc: number("LocalSSRC")?.try_into()?,
            port: number("SourcePort")?.try_into()?,
            highest_seq: None,
            fir_seq: 0,
            packets: 0,
            frame_packets: 0,
            last_frame_packets: 0,
            timestamp: 0,
        };
        ensure!(
            value.port != 0,
            "Device returned an invalid media source port"
        );
        Ok(value)
    }
    fn observe(&mut self, packet: &RtpPacket<'_>) -> bool {
        self.packets = self.packets.wrapping_add(1);
        self.timestamp = packet.timestamp;
        self.frame_packets = self.frame_packets.saturating_add(1);
        if packet.marker {
            self.last_frame_packets = self.frame_packets;
            self.frame_packets = 0;
        }
        let sequence = packet.sequence_number;
        let Some(highest) = self.highest_seq else {
            self.highest_seq = Some(u32::from(sequence));
            return false;
        };
        let distance = sequence.wrapping_sub(highest as u16);
        if distance != 0 && distance < 0x8000 {
            self.highest_seq = Some(highest.wrapping_add(u32::from(distance)));
            return distance > 1;
        }
        false // Duplicate or reordered packet: do not move the report backwards.
    }
    fn rate_control(&self) -> Vec<u8> {
        // AVConference feedback layout from pymobiledevice3 screen_stream.py.
        // Echo the received 24 kHz RTP clock; an unrelated host epoch can cause
        // the device's rate controller to infer congestion and throttle video.
        let words = [
            self.local_ssrc,
            u32::from_be_bytes(*b"RCTL"),
            0x85000004,
            ((self.timestamp >> 8) & 0xffff) << 16,
            self.last_frame_packets,
            ((self.timestamp / 24) & 0xffff) << 16,
            ((self.packets & 0xffff) << 16) | 0xea61,
        ];
        let mut bytes = vec![0x80, 204, 0, 7];
        for word in words {
            bytes.extend_from_slice(&word.to_be_bytes());
        }
        bytes
    }
    fn report(&self) -> ReportBlock {
        ReportBlock {
            source_ssrc: self.remote_ssrc,
            highest_seq: self.highest_seq.unwrap_or(0),
            ..Default::default()
        }
    }
    async fn send(&self, socket: &UdpSocketHandle, bytes: Vec<u8>) -> Result<()> {
        timeout(Duration::from_secs(2), socket.send_to(self.port, bytes))
            .await
            .context("Media receiver feedback timed out")??;
        Ok(())
    }
    async fn keep_alive(&self, socket: &UdpSocketHandle) -> Result<()> {
        self.send(
            socket,
            build_liveness(self.local_ssrc, "iDescriptor", &[self.report()]),
        )
        .await
    }
    async fn request_keyframe(&mut self, socket: &UdpSocketHandle) -> Result<()> {
        self.fir_seq = self.fir_seq.wrapping_add(1);
        self.send(
            socket,
            build_keyframe_request(
                self.local_ssrc,
                "iDescriptor",
                self.remote_ssrc,
                &[self.report()],
                self.fir_seq,
            ),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn feedback_uses_host_ssrc_and_device_source_port() {
        let answer = plist::Value::from_reader_xml(
            br#"<?xml version="1.0"?><plist version="1.0"><dict>
        <key>connection</key><dict><key>streamConfig</key><dict>
        <key>RemoteSSRC</key><integer>5</integer><key>LocalSSRC</key><integer>8</integer>
        <key>SourcePort</key><integer>50001</integer></dict></dict></dict></plist>"#
                .as_slice(),
        )
        .unwrap();
        let feedback = Feedback::from_answer(&answer).unwrap();
        assert_eq!(feedback.local_ssrc, 5);
        assert_eq!(feedback.remote_ssrc, 8);
        assert_eq!(feedback.port, 50001);
        assert!(Feedback::from_answer(&plist::Value::Dictionary(Default::default())).is_err());
    }
    #[test]
    fn sequence_reports_survive_wrap_reordering_and_duplicates() {
        let mut feedback = Feedback {
            local_ssrc: 1,
            remote_ssrc: 2,
            port: 3,
            highest_seq: None,
            fir_seq: 0,
            packets: 0,
            frame_packets: 0,
            last_frame_packets: 0,
            timestamp: 0,
        };
        let packet = |seq: u16| {
            let mut bytes = vec![0x80, 100, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2];
            bytes[2..4].copy_from_slice(&seq.to_be_bytes());
            bytes
        };
        for seq in [65534, 65535, 0, 65535, 0, 1] {
            assert!(!feedback.observe(&RtpPacket::parse(&packet(seq)).unwrap()));
        }
        assert_eq!(feedback.highest_seq, Some(65537));
        assert!(feedback.observe(&RtpPacket::parse(&packet(4)).unwrap()));
        assert_eq!(feedback.highest_seq, Some(65540));
        feedback.timestamp = 96_000;
        feedback.last_frame_packets = 12;
        let rate = feedback.rate_control();
        assert_eq!(rate.len(), 32);
        assert_eq!(&rate[8..12], b"RCTL");
        assert_eq!(&rate[16..20], &((96_000_u32 >> 8) << 16).to_be_bytes());
        assert_eq!(&rate[20..24], &12_u32.to_be_bytes());
        assert_eq!(&rate[24..28], &(4_000_u32 << 16).to_be_bytes());
        assert_eq!(&rate[28..32], &((7_u32 << 16) | 0xea61).to_be_bytes());
    }
}
