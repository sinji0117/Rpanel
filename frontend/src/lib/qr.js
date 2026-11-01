// QR 코드 생성 (ISO/IEC 18004) — 2단계 인증 등록 화면용
// 바이트 모드 · 오류 정정 M · 버전 1~20 (영숫자 300자 정도까지). 외부 라이브러리 없이 직접 구현합니다.

// 버전별 [블록당 오류정정 코드워드, 1그룹 블록 수, 1그룹 블록당 데이터, 2그룹 블록 수, 2그룹 블록당 데이터] (레벨 M)
const BLOCKS_M = [
  null,
  [10, 1, 16, 0, 0],
  [16, 1, 28, 0, 0],
  [26, 1, 44, 0, 0],
  [18, 2, 32, 0, 0],
  [24, 2, 43, 0, 0],
  [16, 4, 27, 0, 0],
  [18, 4, 31, 0, 0],
  [22, 2, 38, 2, 39],
  [22, 3, 36, 2, 37],
  [26, 4, 43, 1, 44],
  [30, 1, 50, 4, 51],
  [22, 6, 36, 2, 37],
  [22, 8, 37, 1, 38],
  [24, 4, 40, 5, 41],
  [24, 5, 41, 5, 42],
  [28, 7, 45, 3, 46],
  [28, 10, 46, 1, 47],
  [26, 9, 43, 4, 44],
  [26, 3, 44, 11, 45],
  [26, 3, 41, 13, 42],
]

const dataCapacity = (v) => {
  const [, b1, d1, b2, d2] = BLOCKS_M[v]
  return b1 * d1 + b2 * d2
}

// ── GF(256) 와 Reed-Solomon ──
const EXP = new Array(512)
const LOG = new Array(256)
{
  let x = 1
  for (let i = 0; i < 255; i++) {
    EXP[i] = x
    LOG[x] = i
    x <<= 1
    if (x & 0x100) x ^= 0x11d
  }
  for (let i = 255; i < 512; i++) EXP[i] = EXP[i - 255]
}
const mul = (a, b) => (a === 0 || b === 0 ? 0 : EXP[LOG[a] + LOG[b]])

function rsGenerator(degree) {
  let poly = [1]
  for (let i = 0; i < degree; i++) {
    const next = new Array(poly.length + 1).fill(0)
    for (let j = 0; j < poly.length; j++) {
      next[j] ^= poly[j]
      next[j + 1] ^= mul(poly[j], EXP[i])
    }
    poly = next
  }
  return poly
}

function rsRemainder(data, degree) {
  const gen = rsGenerator(degree)
  const res = new Array(degree).fill(0)
  for (const b of data) {
    const factor = b ^ res.shift()
    res.push(0)
    for (let i = 0; i < degree; i++) res[i] ^= mul(gen[i + 1], factor)
  }
  return res
}

// ── 데이터 코드워드 ──
function encodeData(bytes, version) {
  const bits = []
  const put = (val, len) => {
    for (let i = len - 1; i >= 0; i--) bits.push((val >>> i) & 1)
  }
  put(0b0100, 4) // 바이트 모드
  put(bytes.length, version <= 9 ? 8 : 16)
  for (const b of bytes) put(b, 8)
  const cap = dataCapacity(version) * 8
  put(0, Math.min(4, cap - bits.length))
  while (bits.length % 8) bits.push(0)
  const out = []
  for (let i = 0; i < bits.length; i += 8) {
    let v = 0
    for (let j = 0; j < 8; j++) v = (v << 1) | bits[i + j]
    out.push(v)
  }
  for (let pad = 0xec; out.length < dataCapacity(version); pad ^= 0xec ^ 0x11) out.push(pad)
  return out
}

function interleave(data, version) {
  const [ecLen, b1, d1, b2, d2] = BLOCKS_M[version]
  const blocks = []
  let k = 0
  for (let i = 0; i < b1 + b2; i++) {
    const len = i < b1 ? d1 : d2
    const d = data.slice(k, k + len)
    k += len
    blocks.push({ d, ec: rsRemainder(d, ecLen) })
  }
  const out = []
  const maxD = Math.max(d1, d2)
  for (let i = 0; i < maxD; i++) for (const b of blocks) if (i < b.d.length) out.push(b.d[i])
  for (let i = 0; i < ecLen; i++) for (const b of blocks) out.push(b.ec[i])
  return out
}

// ── 기능 패턴 ──
function alignmentPositions(version) {
  if (version === 1) return []
  const n = Math.floor(version / 7) + 2
  const size = version * 4 + 17
  const step = Math.ceil((version * 4 + 4) / (n * 2 - 2)) * 2
  const res = [6]
  for (let pos = size - 7; res.length < n; pos -= step) res.splice(1, 0, pos)
  return res
}

const MASKS = [
  (x, y) => (x + y) % 2 === 0,
  (x, y) => y % 2 === 0,
  (x) => x % 3 === 0,
  (x, y) => (x + y) % 3 === 0,
  (x, y) => (Math.floor(x / 3) + Math.floor(y / 2)) % 2 === 0,
  (x, y) => ((x * y) % 2) + ((x * y) % 3) === 0,
  (x, y) => (((x * y) % 2) + ((x * y) % 3)) % 2 === 0,
  (x, y) => (((x + y) % 2) + ((x * y) % 3)) % 2 === 0,
]

function build(version, codewords, mask) {
  const size = version * 4 + 17
  const m = Array.from({ length: size }, () => new Array(size).fill(false))
  const fn = Array.from({ length: size }, () => new Array(size).fill(false))
  const set = (x, y, dark) => {
    m[y][x] = dark
    fn[y][x] = true
  }

  for (let i = 0; i < size; i++) {
    set(6, i, i % 2 === 0)
    set(i, 6, i % 2 === 0)
  }
  for (const [cx, cy] of [
    [3, 3],
    [size - 4, 3],
    [3, size - 4],
  ]) {
    for (let dy = -4; dy <= 4; dy++)
      for (let dx = -4; dx <= 4; dx++) {
        const x = cx + dx
        const y = cy + dy
        if (x < 0 || y < 0 || x >= size || y >= size) continue
        const d = Math.max(Math.abs(dx), Math.abs(dy))
        set(x, y, d !== 2 && d !== 4)
      }
  }
  const al = alignmentPositions(version)
  const last = al.length - 1
  for (let i = 0; i < al.length; i++)
    for (let j = 0; j < al.length; j++) {
      if ((i === 0 && j === 0) || (i === 0 && j === last) || (i === last && j === 0)) continue
      for (let dy = -2; dy <= 2; dy++) for (let dx = -2; dx <= 2; dx++) set(al[i] + dx, al[j] + dy, Math.max(Math.abs(dx), Math.abs(dy)) !== 1)
    }

  // 형식 정보 (레벨 M = 00) — 자리만 먼저 잡고 아래에서 값을 씀
  const formatBits = (() => {
    const data = (0b00 << 3) | mask
    let rem = data
    for (let i = 0; i < 10; i++) rem = (rem << 1) ^ ((rem >>> 9) * 0x537)
    return ((data << 10) | rem) ^ 0x5412
  })()
  const fb = (i) => ((formatBits >>> i) & 1) === 1
  for (let i = 0; i <= 5; i++) set(8, i, fb(i))
  set(8, 7, fb(6))
  set(8, 8, fb(7))
  set(7, 8, fb(8))
  for (let i = 9; i < 15; i++) set(14 - i, 8, fb(i))
  for (let i = 0; i < 8; i++) set(size - 1 - i, 8, fb(i))
  for (let i = 8; i < 15; i++) set(8, size - 15 + i, fb(i))
  set(8, size - 8, true)

  if (version >= 7) {
    let rem = version
    for (let i = 0; i < 12; i++) rem = (rem << 1) ^ ((rem >>> 11) * 0x1f25)
    const bits = (version << 12) | rem
    for (let i = 0; i < 18; i++) {
      const bit = ((bits >>> i) & 1) === 1
      const a = size - 11 + (i % 3)
      const b = Math.floor(i / 3)
      set(a, b, bit)
      set(b, a, bit)
    }
  }

  // 데이터 (오른쪽 아래부터 두 칸씩 지그재그)
  let i = 0
  const total = codewords.length * 8
  for (let right = size - 1; right >= 1; right -= 2) {
    if (right === 6) right = 5
    for (let vert = 0; vert < size; vert++) {
      for (let j = 0; j < 2; j++) {
        const x = right - j
        const upward = ((right + 1) & 2) === 0
        const y = upward ? size - 1 - vert : vert
        if (fn[y][x]) continue
        let dark = false
        if (i < total) {
          dark = ((codewords[i >>> 3] >>> (7 - (i & 7))) & 1) === 1
          i++
        }
        m[y][x] = dark !== MASKS[mask](x, y)
      }
    }
  }
  return m
}

function penalty(m) {
  const size = m.length
  let score = 0
  const line = (get) => {
    for (let a = 0; a < size; a++) {
      let run = 1
      for (let b = 1; b <= size; b++) {
        if (b < size && get(a, b) === get(a, b - 1)) run++
        else {
          if (run >= 5) score += 3 + (run - 5)
          run = 1
        }
      }
      // 1:1:3:1:1 (찾기 패턴 닮은꼴) + 앞뒤 밝은 4칸
      for (let b = 0; b + 11 <= size; b++) {
        const s = []
        for (let k = 0; k < 11; k++) s.push(get(a, b + k) ? 1 : 0)
        const t = s.join('')
        if (t === '10111010000' || t === '00001011101') score += 40
      }
    }
  }
  line((a, b) => m[a][b])
  line((a, b) => m[b][a])
  for (let y = 0; y < size - 1; y++)
    for (let x = 0; x < size - 1; x++) {
      const c = m[y][x]
      if (c === m[y][x + 1] && c === m[y + 1][x] && c === m[y + 1][x + 1]) score += 3
    }
  let dark = 0
  for (const row of m) for (const c of row) if (c) dark++
  const total = size * size
  score += (Math.ceil(Math.abs(dark * 20 - total * 10) / total) - 1) * 10
  return score
}

/** 글자 → QR 모듈 (true = 검은 칸). 너무 길면 오류 */
export function qrMatrix(text) {
  const bytes = Array.from(new TextEncoder().encode(text))
  let version = 1
  while (version <= 20 && 4 + (version <= 9 ? 8 : 16) + bytes.length * 8 > dataCapacity(version) * 8) version++
  if (version > 20) throw new Error('QR: text too long')
  const codewords = interleave(encodeData(bytes, version), version)
  let best = null
  let bestScore = Infinity
  for (let mask = 0; mask < 8; mask++) {
    const m = build(version, codewords, mask)
    const s = penalty(m)
    if (s < bestScore) {
      best = m
      bestScore = s
    }
  }
  return best
}

/** SVG path (1칸 = 1, 여백 4칸 포함한 한 변 길이도 돌려줌) */
export function qrPath(text, margin = 4) {
  const m = qrMatrix(text)
  let d = ''
  for (let y = 0; y < m.length; y++)
    for (let x = 0; x < m.length; x++) if (m[y][x]) d += `M${x + margin} ${y + margin}h1v1h-1z`
  return { d, size: m.length + margin * 2 }
}
