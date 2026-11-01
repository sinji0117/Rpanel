//! 작은 비밀값(2단계 인증 키 등) 암호화 — DB 백업이 새어 나가도 그대로 쓰지 못하게
//!
//! 방식: HMAC-SHA256 을 PRF 로 쓰는 CTR 암호화 + HMAC-SHA256 인증 태그 (encrypt-then-MAC).
//! 키는 패널 비밀키(JWT 비밀키)에서 용도별로 나눠 만듭니다. 외부 크레이트 없이 SHA-256 을 직접 구현합니다.
//! 저장 형식: "v1:" + hex(nonce 16바이트 ‖ 암호문 ‖ 태그 16바이트)

const PREFIX: &str = "v1:";
const NONCE: usize = 16;
const TAG: usize = 16;

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// SHA-256 (FIPS 180-4)
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for (i, word) in chunk.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for (k, wi) in K.iter().zip(w.iter()) {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(*k)
                .wrapping_add(*wi);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (x, v) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(v);
        }
    }
    let mut out = [0u8; 32];
    for (i, v) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

/// HMAC-SHA256 (RFC 2104)
pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..32].copy_from_slice(&sha256(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner: Vec<u8> = k.iter().map(|b| b ^ 0x36).collect();
    inner.extend_from_slice(msg);
    let ih = sha256(&inner);
    let mut outer: Vec<u8> = k.iter().map(|b| b ^ 0x5c).collect();
    outer.extend_from_slice(&ih);
    sha256(&outer)
}

/// 용도별 키 (암호화용, 태그용)
fn keys(master: &[u8]) -> ([u8; 32], [u8; 32]) {
    (
        hmac_sha256(master, b"rpanel secretbox v1 enc"),
        hmac_sha256(master, b"rpanel secretbox v1 mac"),
    )
}

fn keystream_xor(enc: &[u8; 32], nonce: &[u8], data: &mut [u8]) {
    for (i, block) in data.chunks_mut(32).enumerate() {
        let mut input = nonce.to_vec();
        input.extend_from_slice(&(i as u64).to_be_bytes());
        let ks = hmac_sha256(enc, &input);
        for (b, k) in block.iter_mut().zip(ks.iter()) {
            *b ^= k;
        }
    }
}

fn tag(mac: &[u8; 32], nonce: &[u8], ct: &[u8]) -> [u8; TAG] {
    let mut input = nonce.to_vec();
    input.extend_from_slice(ct);
    let full = hmac_sha256(mac, &input);
    let mut t = [0u8; TAG];
    t.copy_from_slice(&full[..TAG]);
    t
}

fn to_hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn from_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

/// 암호화된 값인지 (예전 평문과 구분)
pub fn is_sealed(s: &str) -> bool {
    s.starts_with(PREFIX)
}

/// 암호화 — nonce 는 매번 새 난수 16바이트를 넘기세요
pub fn seal(master: &[u8], nonce: [u8; NONCE], plain: &str) -> String {
    let (enc, mac) = keys(master);
    let mut ct = plain.as_bytes().to_vec();
    keystream_xor(&enc, &nonce, &mut ct);
    let t = tag(&mac, &nonce, &ct);
    let mut all = nonce.to_vec();
    all.extend_from_slice(&ct);
    all.extend_from_slice(&t);
    format!("{PREFIX}{}", to_hex(&all))
}

/// 복호화 — 키가 다르거나 값이 바뀌었으면 None
pub fn open(master: &[u8], sealed: &str) -> Option<String> {
    let raw = from_hex(sealed.strip_prefix(PREFIX)?)?;
    if raw.len() < NONCE + TAG {
        return None;
    }
    let (nonce, rest) = raw.split_at(NONCE);
    let (ct, t) = rest.split_at(rest.len() - TAG);
    let (enc, mac) = keys(master);
    let want = tag(&mac, nonce, ct);
    // 걸리는 시간이 맞은 바이트 수에 따라 달라지지 않게
    if want.iter().zip(t).fold(0u8, |acc, (a, b)| acc | (a ^ b)) != 0 {
        return None;
    }
    let mut pt = ct.to_vec();
    keystream_xor(&enc, nonce, &mut pt);
    String::from_utf8(pt).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_vectors() {
        assert_eq!(
            to_hex(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            to_hex(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            to_hex(&sha256(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        assert_eq!(
            to_hex(&sha256(&vec![b'a'; 1_000_000])),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn hmac_vectors_rfc4231() {
        assert_eq!(
            to_hex(&hmac_sha256(&[0x0b; 20], b"Hi There")),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
        assert_eq!(
            to_hex(&hmac_sha256(b"Jefe", b"what do ya want for nothing?")),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        assert_eq!(
            to_hex(&hmac_sha256(
                &[0xaa; 131],
                b"Test Using Larger Than Block-Size Key - Hash Key First"
            )),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }

    #[test]
    fn roundtrip_and_tamper() {
        let key = b"panel-secret-0123456789";
        let secret = "JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP";
        let s = seal(key, [7u8; NONCE], secret);
        assert!(is_sealed(&s));
        assert!(!s.contains(secret));
        assert_eq!(open(key, &s).as_deref(), Some(secret));
        // 같은 값도 nonce 가 다르면 다른 암호문
        assert_ne!(s, seal(key, [8u8; NONCE], secret));
        // 다른 키 · 바뀐 값 · 잘린 값은 거부
        assert_eq!(open(b"other-key", &s), None);
        let mut bad = s.clone().into_bytes();
        let i = PREFIX.len() + NONCE * 2 + 3;
        bad[i] = if bad[i] == b'0' { b'1' } else { b'0' };
        assert_eq!(open(key, &String::from_utf8(bad).unwrap()), None);
        assert_eq!(open(key, &s[..s.len() - 2]), None);
        assert_eq!(open(key, "v1:zz"), None);
        assert_eq!(open(key, secret), None);
        assert!(!is_sealed(secret));
        // 32바이트보다 긴 값 (여러 블록)
        let long = "x".repeat(100);
        assert_eq!(
            open(key, &seal(key, [1u8; NONCE], &long)).as_deref(),
            Some(long.as_str())
        );
        assert_eq!(open(key, &seal(key, [1u8; NONCE], "")).as_deref(), Some(""));
    }
}
