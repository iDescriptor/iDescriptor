// SPDX-FileCopyrightText: 2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Read picture geometry from HEVC SPS (H.265 7.3.2.2), not pixel colors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub left: u32,
    pub top: u32,
    pub visible_width: u32,
    pub visible_height: u32,
    pub chroma_width: u32,
    pub chroma_height: u32,
}
impl Picture {
    pub fn crop(self, width: u32, height: u32) -> Option<[i32; 4]> {
        if (width, height) == (self.visible_width, self.visible_height) {
            return Some([0; 4]);
        }
        // Accept coding-block padding, not a scaled or unrelated decoder output.
        if width < self.width
            || height < self.height
            || width - self.width > 63
            || height - self.height > 63
        {
            return None;
        }
        Some([
            self.left as i32,
            (width - self.left - self.visible_width) as i32,
            self.top as i32,
            (height - self.top - self.visible_height) as i32,
        ])
    }
}
/// Use the mode of the exact display requested by startMediaStream.
pub fn display_size(value: &plist::Value, display_id: u64) -> Option<(u32, u32)> {
    let displays = value.as_dictionary()?.get("displays")?.as_array()?;
    let display = displays
        .iter()
        .filter_map(plist::Value::as_dictionary)
        .find(|display| {
            display
                .get("displayId")
                .and_then(plist::Value::as_unsigned_integer)
                == Some(display_id)
        })?;
    let size = display
        .get("currentMode")?
        .as_dictionary()?
        .get("size")?
        .as_array()?;
    fn dimension(value: &plist::Value) -> Option<u32> {
        let n = value
            .as_real()
            .or_else(|| value.as_unsigned_integer().map(|n| n as f64))?;
        if n.is_finite() && n >= 1.0 && n <= 16384.0 && n.fract() == 0.0 {
            Some(n as u32)
        } else {
            None
        }
    }
    Some((dimension(size.first()?)?, dimension(size.get(1)?)?))
}
pub fn visible_crop(
    picture: Picture,
    display: Option<(u32, u32)>,
    width: u32,
    height: u32,
) -> Option<[i32; 4]> {
    // An explicit conformance window is stronger than display-mode inference.
    if picture.visible_width != picture.width || picture.visible_height != picture.height {
        return picture.crop(width, height);
    }
    if let Some((mut dw, mut dh)) = display {
        if (width > height) != (dw > dh) {
            std::mem::swap(&mut dw, &mut dh);
        }
        dw -= dw % picture.chroma_width;
        dh -= dh % picture.chroma_height;
        if dw > 0 && dh > 0 && width >= dw && height >= dh && width - dw <= 63 && height - dh <= 63
        {
            return Some([0, (width - dw) as i32, 0, (height - dh) as i32]);
        }
    }
    picture.crop(width, height)
}
struct Bits<'a> {
    bytes: &'a [u8],
    bit: usize,
}
impl Bits<'_> {
    fn read(&mut self, n: usize) -> Option<u32> {
        if n > 32 || self.bit.checked_add(n)? > self.bytes.len() * 8 {
            return None;
        }
        let mut value = 0;
        for _ in 0..n {
            value = (value << 1) | u32::from((self.bytes[self.bit / 8] >> (7 - self.bit % 8)) & 1);
            self.bit += 1;
        }
        Some(value)
    }
    fn skip(&mut self, n: usize) -> Option<()> {
        if self.bit.checked_add(n)? > self.bytes.len() * 8 {
            return None;
        }
        self.bit += n;
        Some(())
    }
    fn ue(&mut self) -> Option<u32> {
        let mut zeros = 0;
        while self.read(1)? == 0 {
            zeros += 1;
            if zeros > 30 {
                return None;
            }
        }
        Some(((1_u32 << zeros) - 1) + self.read(zeros)?)
    }
}
pub fn sps(nal: &[u8]) -> Option<Picture> {
    if nal.len() < 3 || (nal[0] >> 1) & 63 != 33 {
        return None;
    }
    let mut rbsp = Vec::with_capacity(nal.len());
    let mut zeros = 0;
    for &byte in &nal[2..] {
        if zeros >= 2 && byte == 3 {
            zeros = 0;
            continue;
        }
        rbsp.push(byte);
        zeros = if byte == 0 { zeros + 1 } else { 0 };
    }
    let mut b = Bits {
        bytes: &rbsp,
        bit: 0,
    };
    b.skip(4)?;
    let layers = b.read(3)? as usize;
    if layers > 6 {
        return None;
    }
    b.skip(1)?;
    b.skip(96)?; // general_profile_tier_level and general_level_idc
    let mut flags = Vec::new();
    for _ in 0..layers {
        flags.push((b.read(1)? != 0, b.read(1)? != 0));
    }
    if layers > 0 {
        b.skip((8 - layers) * 2)?;
    }
    for (profile, level) in flags {
        if profile {
            b.skip(88)?;
        }
        if level {
            b.skip(8)?;
        }
    }
    b.ue()?;
    let chroma = b.ue()?;
    if chroma > 3 {
        return None;
    }
    let separate = chroma == 3 && b.read(1)? != 0;
    let width = b.ue()?;
    let height = b.ue()?;
    if width == 0 || height == 0 || width > 16384 || height > 16384 {
        return None;
    }
    let (sx, sy) = match if separate { 0 } else { chroma } {
        1 => (2, 2),
        2 => (2, 1),
        _ => (1, 1),
    };
    let (mut left, mut right, mut top, mut bottom) = (0, 0, 0, 0);
    if b.read(1)? != 0 {
        left = b.ue()?.checked_mul(sx)?;
        right = b.ue()?.checked_mul(sx)?;
        top = b.ue()?.checked_mul(sy)?;
        bottom = b.ue()?.checked_mul(sy)?;
    }
    let visible_width = width.checked_sub(left.checked_add(right)?)?;
    let visible_height = height.checked_sub(top.checked_add(bottom)?)?;
    if visible_width == 0 || visible_height == 0 {
        return None;
    }
    Some(Picture {
        width,
        height,
        left,
        top,
        visible_width,
        visible_height,
        chroma_width: sx,
        chroma_height: sy,
    })
}
pub fn parameter_sets(bytes: &[u8]) -> Vec<&[u8]> {
    fn start(bytes: &[u8], from: usize) -> Option<(usize, usize)> {
        (from..bytes.len().saturating_sub(2)).find_map(|i| {
            if bytes[i..].starts_with(&[0, 0, 0, 1]) {
                Some((i, i + 4))
            } else if bytes[i..].starts_with(&[0, 0, 1]) {
                Some((i, i + 3))
            } else {
                None
            }
        })
    }
    let mut next = start(bytes, 0);
    let mut found = Vec::new();
    while let Some((_, body)) = next {
        next = start(bytes, body);
        let end = next.map(|(offset, _)| offset).unwrap_or(bytes.len());
        let nal = &bytes[body..end];
        if nal.len() >= 2 && matches!((nal[0] >> 1) & 63, 32..=34) && nal.len() <= 65536 {
            found.push(nal);
        }
    }
    found
}
pub fn picture(bytes: &[u8]) -> Option<Picture> {
    parameter_sets(bytes).into_iter().filter_map(sps).last()
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(width: u32, height: u32, window: Option<[u32; 4]>) -> Vec<u8> {
        let mut bits = vec![false; 8 + 96]; // no sublayers, zero profile fields
        fn ue(bits: &mut Vec<bool>, value: u32) {
            let n = value + 1;
            let len = (32 - n.leading_zeros()) as usize;
            bits.extend(std::iter::repeat_n(false, len - 1));
            for shift in (0..len).rev() {
                bits.push(n & (1 << shift) != 0);
            }
        }
        ue(&mut bits, 0);
        ue(&mut bits, 1);
        ue(&mut bits, width);
        ue(&mut bits, height);
        bits.push(window.is_some());
        if let Some(offsets) = window {
            for n in offsets {
                ue(&mut bits, n);
            }
        }
        bits.push(true);
        while bits.len() % 8 != 0 {
            bits.push(false);
        }
        let mut nal = vec![0x42, 1];
        let mut zeros = 0;
        for chunk in bits.chunks(8) {
            let byte = chunk.iter().fold(0_u8, |n, &bit| (n << 1) | u8::from(bit));
            if zeros >= 2 && byte <= 3 {
                nal.push(3);
                zeros = 0;
            }
            nal.push(byte);
            zeros = if byte == 0 { zeros + 1 } else { 0 };
        }
        nal
    }
    #[test]
    fn sps_geometry_removes_alignment_padding_without_examining_pixels() {
        let p = sps(&fixture(1124, 2436, None)).unwrap();
        assert_eq!(p.crop(1136, 2464), Some([0, 12, 0, 28]));
        assert_eq!(p.crop(1124, 2436), Some([0; 4]));
        assert_eq!(p.crop(720, 1280), None);
        assert_eq!(p.crop(1280, 2560), None);
        let mut bytes = vec![0, 0, 0, 1];
        bytes.extend(fixture(1124, 2436, None));
        assert_eq!(picture(&bytes), Some(p));
    }
    #[test]
    fn conformance_window_uses_chroma_units_and_does_not_crop_twice() {
        let p = sps(&fixture(1140, 2460, Some([2, 4, 3, 5]))).unwrap();
        assert_eq!((p.visible_width, p.visible_height), (1128, 2444));
        assert_eq!(p.crop(1140, 2460), Some([4, 8, 6, 10]));
        assert_eq!(p.crop(1128, 2444), Some([0; 4]));
        assert!(sps(&fixture(10, 10, Some([5, 5, 5, 5]))).is_none());
        for n in 0..8 {
            assert!(sps(&fixture(1124, 2436, None)[..n]).is_none());
        }
    }
    #[test]
    fn device_display_mode_trims_padding_and_ignores_wireless_displays() {
        let value:plist::Value=plist::from_bytes(br#"<?xml version="1.0"?><plist version="1.0"><dict><key>displays</key><array>
        <dict><key>displayId</key><integer>2</integer><key>currentMode</key><dict><key>size</key><array><real>1136</real><real>2448</real></array></dict></dict>
        <dict><key>displayId</key><integer>1</integer><key>currentMode</key><dict><key>size</key><array><real>1125</real><real>2436</real></array></dict></dict>
        </array></dict></plist>"#).unwrap();
        let mode = display_size(&value, 1);
        assert_eq!(mode, Some((1125, 2436)));
        let p = sps(&fixture(1136, 2464, None)).unwrap();
        assert_eq!(visible_crop(p, mode, 1136, 2464), Some([0, 12, 0, 28]));
        assert_eq!(visible_crop(p, mode, 2464, 1136), Some([0, 28, 0, 12]));
        assert_eq!(visible_crop(p, mode, 1124, 2436), Some([0; 4]));
        assert_eq!(visible_crop(p, mode, 720, 1280), None);
        assert_eq!(display_size(&value, 7), None);
    }
}
