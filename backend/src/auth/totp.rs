//! TOTP (RFC 6238) — 관리자 2단계 인증
//!
//! Google Authenticator · 1Password · Authy 등과 같은 방식 (SHA-1, 6자리, 30초).
//! 외부 크레이트 없이 SHA-1 · HMAC · Base32 를 직접 구현합니다 (RFC 테스트 값으로 검증).

/// 코드가 바뀌는 간격 (초)
pub const STEP: u64 = 30;
/// 코드 자릿수
pub const DIGITS: u32 = 6;

/// SHA-1 (RFC 3174)
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks(64) {
        let mut w = [0u32; 80];
        for (i, word) in chunk.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let t = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }
    let mut out = [0u8; 20];
    for (i, v) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

/// HMAC-SHA1 (RFC 2104)
pub fn hmac_sha1(key: &[u8], msg: &[u8]) -> [u8; 20] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..20].copy_from_slice(&sha1(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner = Vec::with_capacity(64 + msg.len());
    inner.extend(k.iter().map(|b| b ^ 0x36));
    inner.extend_from_slice(msg);
    let inner_hash = sha1(&inner);
    let mut outer = Vec::with_capacity(64 + 20);
    outer.extend(k.iter().map(|b| b ^ 0x5c));
    outer.extend_from_slice(&inner_hash);
    sha1(&outer)
}

/// HOTP (RFC 4226)
pub fn hotp(key: &[u8], counter: u64, digits: u32) -> u32 {
    let mac = hmac_sha1(key, &counter.to_be_bytes());
    let off = (mac[19] & 0x0f) as usize;
    let bin = (u32::from(mac[off] & 0x7f) << 24)
        | (u32::from(mac[off + 1]) << 16)
        | (u32::from(mac[off + 2]) << 8)
        | u32::from(mac[off + 3]);
    bin % 10u32.pow(digits)
}

const B32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// Base32 (RFC 4648, 패딩 없음) — 인증 앱에 넣는 비밀키 형식
pub fn base32_encode(data: &[u8]) -> String {
    let mut out = String::new();
    let (mut buf, mut bits) = (0u32, 0u32);
    for &b in data {
        buf = (buf << 8) | u32::from(b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(B32[((buf >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(B32[((buf << (5 - bits)) & 31) as usize] as char);
    }
    out
}

/// Base32 해석 — 공백·하이픈·패딩(=)은 무시, 대소문자 구분 없음
pub fn base32_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (mut buf, mut bits) = (0u32, 0u32);
    for c in s.chars() {
        if c == ' ' || c == '-' || c == '=' {
            continue;
        }
        let v = B32
            .iter()
            .position(|&x| x as char == c.to_ascii_uppercase())? as u32;
        buf = (buf << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

/// 6자리 코드를 확인합니다. 맞으면 맞은 시간 단계(step)를, 틀리면 None.
/// 휴대폰 시계가 조금 어긋나도 되도록 앞뒤 한 단계(±30초)까지 허용합니다.
pub fn verify(secret_b32: &str, code: &str, unix_secs: u64) -> Option<u64> {
    let code = code.trim().replace(' ', "");
    if code.len() != DIGITS as usize || !code.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let want: u32 = code.parse().ok()?;
    let key = base32_decode(secret_b32)?;
    if key.is_empty() {
        return None;
    }
    let now = unix_secs / STEP;
    let mut hit = None;
    // 세 단계를 모두 계산 (맞은 위치에 따라 걸리는 시간이 달라지지 않게)
    for step in [now.saturating_sub(1), now, now + 1] {
        if hotp(&key, step, DIGITS) == want && hit.is_none() {
            hit = Some(step);
        }
    }
    hit
}

fn pct(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// 인증 앱 등록용 주소 (QR 코드로 보여 줌)
pub fn otpauth_uri(issuer: &str, account: &str, secret_b32: &str) -> String {
    format!(
        "otpauth://totp/{}:{}?secret={}&issuer={}&algorithm=SHA1&digits={}&period={}",
        pct(issuer),
        pct(account),
        secret_b32,
        pct(issuer),
        DIGITS,
        STEP
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn sha1_vectors() {
        assert_eq!(
            hex(&sha1(b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(hex(&sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(
            hex(&sha1(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
        let million_a = vec![b'a'; 1_000_000];
        assert_eq!(
            hex(&sha1(&million_a)),
            "34aa973cd4c4daa4f61eeb2bdbad27316534016f"
        );
    }

    #[test]
    fn hmac_vectors_rfc2202() {
        assert_eq!(
            hex(&hmac_sha1(&[0x0b; 20], b"Hi There")),
            "b617318655057264e28bc0b6fb378c8ef146be00"
        );
        assert_eq!(
            hex(&hmac_sha1(b"Jefe", b"what do ya want for nothing?")),
            "effcdf6ae5eb2fa2d27416d5f184df9c259a7c79"
        );
        // 블록보다 긴 키
        assert_eq!(
            hex(&hmac_sha1(
                &[0xaa; 80],
                b"Test Using Larger Than Block-Size Key - Hash Key First"
            )),
            "aa4ae5e15272d00e95705637ce8a3b55ed402112"
        );
    }

    #[test]
    fn hotp_vectors_rfc4226() {
        let key = b"12345678901234567890";
        let want = [
            755224, 287082, 359152, 969429, 338314, 254676, 287922, 162583, 399871, 520489,
        ];
        for (i, w) in want.iter().enumerate() {
            assert_eq!(hotp(key, i as u64, 6), *w, "counter {i}");
        }
    }

    #[test]
    fn totp_vectors_rfc6238() {
        let key = b"12345678901234567890";
        for (t, w) in [
            (59u64, 94287082u32),
            (1111111109, 7081804),
            (1111111111, 14050471),
            (1234567890, 89005924),
            (2000000000, 69279037),
            (20000000000, 65353130),
        ] {
            assert_eq!(hotp(key, t / STEP, 8), w, "t={t}");
        }
    }

    #[test]
    fn base32_roundtrip() {
        assert_eq!(base32_encode(b"foobar"), "MZXW6YTBOI");
        assert_eq!(base32_encode(b"f"), "MY");
        assert_eq!(base32_decode("mzxw 6ytb-oi==").unwrap(), b"foobar");
        assert!(base32_decode("MZ1!").is_none());
        let k: Vec<u8> = (0u8..20).collect();
        assert_eq!(base32_decode(&base32_encode(&k)).unwrap(), k);
    }

    #[test]
    fn verify_window() {
        let secret = base32_encode(b"12345678901234567890");
        let t = 1_111_111_111u64;
        let code = format!("{:06}", hotp(b"12345678901234567890", t / STEP, 6));
        assert_eq!(verify(&secret, &code, t), Some(t / STEP));
        // 30초 앞뒤까지 허용, 그보다 멀면 거부
        assert_eq!(verify(&secret, &code, t + STEP), Some(t / STEP));
        assert_eq!(verify(&secret, &code, t - STEP), Some(t / STEP));
        assert_eq!(verify(&secret, &code, t + 3 * STEP), None);
        assert_eq!(verify(&secret, "12345", t), None);
        assert_eq!(verify(&secret, "abcdef", t), None);
        assert_eq!(verify(&secret, &format!("{} ", code), t), Some(t / STEP));
    }

    #[test]
    fn uri_is_encoded() {
        let u = otpauth_uri("Rpanel", "a+b@example.com", "ABC");
        assert_eq!(
            u,
            "otpauth://totp/Rpanel:a%2Bb%40example.com?secret=ABC&issuer=Rpanel&algorithm=SHA1&digits=6&period=30"
        );
    }
}
