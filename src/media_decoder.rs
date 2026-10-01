// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

use anyhow::{Context, Result, anyhow, bail};
use cpp::cpp;
use ffmpeg::{
    codec, format,
    frame::Video,
    media::Type,
    software::scaling::{context::Context as ScalingContext, flag::Flags},
    util::{format::Pixel, frame::side_data::Type as FrameSideDataType},
};
use ffmpeg_next as ffmpeg;
use libheif_rs::{ColorSpace, HeifContext, LibHeif, RgbChroma};
use qttypes::QImage;
use std::io::{Read, Seek};
use tokio_util::sync::CancellationToken;

const FFMPEG_IO_BUFFER_SIZE: usize = 1024 * 1024;

pub fn generate_thumbnail<R>(
    reader: R,
    requested_width: u32,
    requested_height: u32,
    cancellation: CancellationToken,
) -> Result<QImage>
where
    R: Read + Seek + Send + 'static,
{
    if requested_width == 0 || requested_height == 0 {
        bail!("Thumbnail dimensions must be greater than zero");
    }

    ffmpeg::init().context("Failed to initialize FFmpeg")?;
    let stream_io =
        format::context::StreamIo::from_read_seek_with_capacity(reader, FFMPEG_IO_BUFFER_SIZE)
            .context("Failed to create FFmpeg AFC stream")?;

    let cancellation_for_ffmpeg = cancellation.clone();
    let mut input = format::input_from_stream_with_interrupt(stream_io, None, None, move || {
        cancellation_for_ffmpeg.is_cancelled()
    })
    .context("FFmpeg could not open the video stream")?;

    let (stream_index, parameters, stream_rotation) = {
        let stream = input
            .streams()
            .best(Type::Video)
            .ok_or_else(|| anyhow!("The file does not contain a video stream"))?;
        let rotation = stream
            .side_data()
            .find(|data| data.kind() == ffmpeg::codec::packet::side_data::Type::DisplayMatrix)
            .map(|data| display_rotation(data.data()))
            .unwrap_or_default();
        (stream.index(), stream.parameters(), rotation)
    };

    let decoder_context = codec::context::Context::from_parameters(parameters)
        .context("Failed to create the video decoder")?;
    let mut decoder = decoder_context
        .decoder()
        .video()
        .context("Failed to open the video decoder")?;

    let mut decoded = Video::empty();
    for (stream, packet) in input.packets() {
        if cancellation.is_cancelled() {
            bail!("Thumbnail generation was cancelled");
        }
        if stream.index() != stream_index {
            continue;
        }

        decoder
            .send_packet(&packet)
            .context("Failed to send a video packet to the decoder")?;
        if decoder.receive_frame(&mut decoded).is_ok() {
            return frame_to_qimage(&decoded, stream_rotation, requested_width, requested_height);
        }
    }

    decoder
        .send_eof()
        .context("Failed to flush the video decoder")?;
    if decoder.receive_frame(&mut decoded).is_ok() {
        return frame_to_qimage(&decoded, stream_rotation, requested_width, requested_height);
    }

    bail!("FFmpeg did not decode a video frame")
}

fn frame_to_qimage(
    frame: &Video,
    stream_rotation: f64,
    requested_width: u32,
    requested_height: u32,
) -> Result<QImage> {
    let mut scaler = ScalingContext::get(
        frame.format(),
        frame.width(),
        frame.height(),
        Pixel::RGB24,
        frame.width(),
        frame.height(),
        Flags::BILINEAR,
    )
    .context("Failed to create FFmpeg's RGB scaler")?;

    let mut rgb_frame = Video::empty();
    scaler
        .run(frame, &mut rgb_frame)
        .context("Failed to convert the video frame to RGB")?;

    let frame_rotation = frame
        .side_data(FrameSideDataType::DisplayMatrix)
        .map(|data| display_rotation(data.data()))
        .unwrap_or(stream_rotation);
    pixels_to_qimage(
        rgb_frame.data(0),
        rgb_frame.width(),
        rgb_frame.height(),
        rgb_frame.stride(0),
        false,
        frame_rotation,
        requested_width,
        requested_height,
    )
}

fn display_rotation(data: &[u8]) -> f64 {
    if data.len() < 9 * std::mem::size_of::<i32>() {
        return 0.0;
    }

    let rotation = unsafe { -ffmpeg::ffi::av_display_rotation_get(data.as_ptr().cast::<i32>()) };
    if rotation.is_finite() { rotation } else { 0.0 }
}

pub fn decode_heic_image(data: &[u8]) -> Result<QImage> {
    if data.is_empty() {
        bail!("Cannot decode an empty HEIC image");
    }

    let context = HeifContext::read_from_bytes(data).context("Failed to parse the HEIC image")?;
    let handle = context
        .primary_image_handle()
        .context("Failed to find the primary HEIC image")?;
    let image = LibHeif::new()
        .decode(&handle, ColorSpace::Rgb(RgbChroma::Rgba), None)
        .context("Failed to decode the HEIC image")?;
    let plane = image
        .planes()
        .interleaved
        .ok_or_else(|| anyhow!("The decoded HEIC image has no interleaved RGB plane"))?;

    pixels_to_qimage(
        plane.data,
        plane.width,
        plane.height,
        plane.stride,
        true,
        0.0,
        0,
        0,
    )
}

cpp! {{
    #include <QImage>
    #include <QTransform>
}}

fn pixels_to_qimage(
    data: &[u8],
    width: u32,
    height: u32,
    stride: usize,
    rgba: bool,
    rotation: f64,
    requested_width: u32,
    requested_height: u32,
) -> Result<QImage> {
    let row_bytes = (width as usize)
        .checked_mul(if rgba { 4 } else { 3 })
        .ok_or_else(|| anyhow!("Image row overflow"))?;
    let required = stride
        .checked_mul(height.saturating_sub(1) as usize)
        .and_then(|n| n.checked_add(row_bytes))
        .ok_or_else(|| anyhow!("Image size overflow"))?;
    if width == 0 || height == 0 || stride < row_bytes || data.len() < required {
        bail!("Invalid decoded image layout");
    }
    let width = i32::try_from(width)?;
    let height = i32::try_from(height)?;
    let stride = isize::try_from(stride)?;
    let requested_width = i32::try_from(requested_width)?;
    let requested_height = i32::try_from(requested_height)?;
    let data = data.as_ptr();
    // Copy before returning: the decoder owns the source pixel storage.
    let image = cpp!(unsafe [
        data as "const unsigned char *", width as "int", height as "int",
        stride as "qsizetype", rgba as "bool", rotation as "double",
        requested_width as "int", requested_height as "int"
    ] -> QImage as "QImage" {
        QImage result = QImage(data, width, height, stride,
            rgba ? QImage::Format_RGBA8888 : QImage::Format_RGB888).copy();
        if (rotation != 0.0) {
            QTransform transform;
            transform.rotate(rotation);
            result = result.transformed(transform);
        }
        if (requested_width > 0 && requested_height > 0) {
            result = result.scaled(requested_width, requested_height,
                Qt::KeepAspectRatio, Qt::SmoothTransformation);
        }
        return result;
    });
    if image == QImage::default() {
        bail!("Failed to create QImage");
    }
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn qimage_owns_pixels_and_preserves_rgba_and_stride() {
        let image = {
            let pixels = vec![255, 0, 0, 128, 9, 9, 9, 9, 0, 255, 0, 255];
            pixels_to_qimage(&pixels, 1, 2, 8, true, 0.0, 0, 0).unwrap()
        };
        let valid = cpp!(unsafe [image as "QImage"] -> bool as "bool" {
            return image.width() == 1 && image.height() == 2
                && image.pixelColor(0, 0) == QColor(255, 0, 0, 128)
                && image.pixelColor(0, 1) == QColor(0, 255, 0, 255);
        });
        assert!(valid);
    }

    #[test]
    fn shared_afc_reader_opens_once_seeks_and_closes_once() {
        check_shared_afc_cleanup(false);
    }

    #[test]
    fn shared_afc_reader_explicitly_closes_after_read_error() {
        check_shared_afc_cleanup(true);
    }

    fn check_shared_afc_cleanup(fail_read: bool) {
        crate::RUNTIME.block_on(async {
            use idevice::{
                Idevice,
                afc::{AfcClient, opcode::AfcFopenMode, shared_file::SharedFileDescriptor},
            };
            use std::io::SeekFrom;
            use std::sync::Arc;
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            use tokio::sync::Mutex;
            use tokio_util::io::SyncIoBridge;

            let (client, mut server) = tokio::io::duplex(4096);
            let client = Arc::new(Mutex::new(AfcClient::new(Idevice::new(
                Box::new(client),
                "test",
            ))));
            let device = crate::RUNTIME.spawn(async move {
                // Exact wire operations: no reopen between reads, one close.
                let operations = if fail_read {
                    vec![
                        (13u64, 14u64, 1u64.to_le_bytes().to_vec(), vec![]),
                        (15, 1, 4u64.to_le_bytes().to_vec(), vec![]),
                        (20, 1, 0u64.to_le_bytes().to_vec(), vec![]),
                    ]
                } else {
                    vec![
                        (13u64, 14u64, 1u64.to_le_bytes().to_vec(), vec![]),
                        (15, 2, vec![], b"abc".to_vec()),
                        (17, 1, 0u64.to_le_bytes().to_vec(), vec![]),
                        (18, 19, 1u64.to_le_bytes().to_vec(), vec![]),
                        (15, 2, vec![], b"bc".to_vec()),
                        (20, 1, 0u64.to_le_bytes().to_vec(), vec![]),
                    ]
                };
                for (opcode, response_opcode, header_data, payload) in operations {
                    let mut header = [0u8; 40];
                    server.read_exact(&mut header).await.unwrap();
                    let number =
                        |offset| u64::from_le_bytes(header[offset..offset + 8].try_into().unwrap());
                    assert_eq!(number(32), opcode);
                    let mut body = vec![0; number(8) as usize - 40];
                    server.read_exact(&mut body).await.unwrap();
                    if opcode == 17 {
                        assert_eq!(u64::from_le_bytes(body[8..16].try_into().unwrap()), 0);
                        assert_eq!(u64::from_le_bytes(body[16..24].try_into().unwrap()), 1);
                    }
                    let mut response = Vec::new();
                    for word in [
                        number(0),
                        (40 + header_data.len() + payload.len()) as u64,
                        (40 + header_data.len()) as u64,
                        number(24),
                        response_opcode,
                    ] {
                        response.extend(word.to_le_bytes());
                    }
                    response.extend(header_data);
                    response.extend(payload);
                    server.write_all(&response).await.unwrap();
                }
            });
            let file = SharedFileDescriptor::open(client, "video.mov", AfcFopenMode::RdOnly)
                .await
                .unwrap();
            let (reader, closer) = file.split();
            let bridge = SyncIoBridge::new(reader);
            tokio::task::spawn_blocking(move || {
                let mut bridge = bridge;
                let mut bytes = [0; 3];
                if fail_read {
                    assert!(std::io::Read::read(&mut bridge, &mut bytes).is_err());
                    return;
                }
                std::io::Read::read_exact(&mut bridge, &mut bytes).unwrap();
                assert_eq!(&bytes, b"abc");
                assert_eq!(
                    std::io::Seek::seek(&mut bridge, SeekFrom::Start(1)).unwrap(),
                    1
                );
                let mut bytes = [0; 2];
                std::io::Read::read_exact(&mut bridge, &mut bytes).unwrap();
                assert_eq!(&bytes, b"bc");
            })
            .await
            .unwrap();
            closer.close().await.unwrap();
            device.await.unwrap();
        });
    }

    #[test]
    fn generates_a_thumbnail_from_seekable_rust_io() {
        let video = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/resources/unlock.mp4"))
            .expect("test video should be present");
        let image = generate_thumbnail(Cursor::new(video), 320, 180, CancellationToken::new())
            .expect("FFmpeg should decode the first video frame");

        let size = image.size();
        assert!(size.width > 0 && size.width <= 320);
        assert!(size.height > 0 && size.height <= 180);
    }
}
