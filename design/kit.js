// What every design draws with: the decks' tracks, waveforms that sound like house music
// (kicks on the beat, claps on 2 and 4, hats between, a bassline, a voice in places), knobs,
// faders, meters, pads, icons.

const C = {
  bg: '#0c0c0f', surface: '#131317', card: '#1a1a1f', well: '#0f0f13', raised: '#2b2b33', field: '#24242b',
  line: '#26262d', lineSoft: '#1c1c22', lineStrong: '#34343d', text: '#f4f4f6', text2: '#d4d4da', text3: '#8a8a95', text4: '#55555f',
  accent: '#5b8cff', amber: '#ffc53d', mint: '#3ddc97', coral: '#ff7a59', sky: '#38bdf8', violet: '#a78bfa', pink: '#f472b6', teal: '#2dd4bf', slate: '#94a3b8',
  low: '#3070f0', mid: '#f0a23c', high: '#eef0f4', red: '#ff5c5c',
};
const DECK = { A: C.accent, B: C.coral, C: C.mint, D: C.violet };
const CUES = [C.coral, C.amber, C.mint, C.sky, C.violet, C.pink, C.teal, C.slate];
const STEM = [C.pink, C.amber, C.sky, C.mint];

// The library, as the user's own: what analysis made of it.
const LIB = [
  { t: 'Why Am I Falling', a: 'Manyeya', bpm: 122.0, key: '1A', s: 318, g: 'Afro House', p: 0 },
  { t: 'Euphoria', a: 'Manyeya', bpm: 121.54, key: '6B', s: 474, g: 'Afro House', p: 1, deck: 'A' },
  { t: 'Yours', a: 'Manyeya', bpm: 122.0, key: '3A', s: 381, g: 'Afro House', p: 0 },
  { t: 'In Time', a: 'Manyeya', bpm: 112.0, key: '2A', s: 520, g: 'Deep House', p: 0 },
  { t: 'That Night', a: 'Manyeya', bpm: 120.0, key: '3A', s: 452, g: 'Afro House', p: 1, deck: 'B' },
  { t: 'From Me To You', a: 'Manyeya', bpm: 122.0, key: '3A', s: 463, g: 'Afro House', p: 0 },
  { t: 'I see through you (Afro mix)', a: 'Manyeya', bpm: 123.0, key: '7A', s: 331, g: 'Afro House', p: 2 },
  { t: 'The Good Times (Afro mix)', a: 'Manyeya', bpm: 121.0, key: '4A', s: 385, g: 'Afro House', p: 0 },
  { t: 'After The Rain', a: 'Manyeya', bpm: 117.0, key: '3B', s: 393, g: 'Deep House', p: 0 },
  { t: 'SARII WAY (A.W.A)', a: 'PLXYBOII ft. Dolly Ditebogo', bpm: 127.0, key: '7A', s: 182, g: 'Amapiano', p: 3 },
  { t: 'Judgement', a: '—', bpm: 75.0, key: '5A', s: 240, g: 'Drum & Bass', p: 0 },
  { t: 'Climax (Live cover)', a: 'Leroy', bpm: 124.5, key: '5A', s: 258, g: 'R&B', p: 0 },
  { t: 'Falling (Afro mix, vocals)', a: 'Manyeya', bpm: 122.0, key: '1A', s: 296, g: 'Afro House', p: 0, ana: true },
  { t: 'Night Drive', a: 'Pina Studio', bpm: 118.0, key: '8A', s: 214, g: 'Melodic House', p: 0, ana: true },
];

const TRACK = {
  A: { title: 'Euphoria', artist: 'Manyeya', bpm: 121.54, key: '6B', len: 474, now: 148.4, tempo: 0, seed: 7,
       phrases: [['Intro', 0, 31], ['Groove', 31, 94], ['Build', 94, 126], ['Drop', 126, 190], ['Break', 190, 238], ['Build', 238, 270], ['Drop 2', 270, 380], ['Outro', 380, 474]],
       hot: [3.1, 31.6, 94.8, 126.4, null, null, null, null], cue: 3.1, loop: null },
  B: { title: 'That Night', artist: 'Manyeya', bpm: 120.0, key: '3A', len: 452, now: 62.7, tempo: 1.28, seed: 23, sync: true,
       phrases: [['Intro', 0, 32], ['Verse', 32, 96], ['Chorus', 96, 160], ['Bridge', 160, 208], ['Chorus 2', 208, 290], ['Outro', 290, 452]],
       hot: [0.2, 32.1, 96.0, null, null, null, null, null], cue: 0.2, loop: [60.0, 62.0 + 2 * 60 / 120 * 2] },
};

function rng(seed) { let a = seed >>> 0; return () => { a |= 0; a = (a + 0x6d2b79f5) | 0; let t = Math.imul(a ^ (a >>> 15), 1 | a); t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t; return ((t ^ (t >>> 14)) >>> 0) / 4294967296; }; }
function hash(x) { const s = Math.sin(x * 127.1 + 311.7) * 43758.5453; return s - Math.floor(s); }

// How loud each band is at `t` seconds into a track: kicks, claps, hats, bass and a voice, by section.
function bands(tr, t) {
  if (t < 0 || t > tr.len) return [0, 0, 0];
  const beat = t * tr.bpm / 60, ph = beat - Math.floor(beat), b = Math.floor(beat);
  const sec = tr.phrases.find(p => t >= p[1] && t < p[2]) || ['', 0, 0];
  const n = sec[0].toLowerCase();
  const e = n.startsWith('intro') || n.startsWith('outro') ? 0.55 : n.startsWith('break') || n.startsWith('bridge') ? 0.35 : n.startsWith('build') ? 0.6 + 0.4 * (t - sec[1]) / (sec[2] - sec[1]) : 1;
  const kickOn = !(n.startsWith('break') || n.startsWith('bridge'));
  const kick = kickOn ? Math.exp(-ph * 7) : 0;
  const bass = n.startsWith('intro') ? 0.1 : 0.32 + 0.12 * Math.sin(beat * 1.57 + tr.seed);
  const clap = (b % 2 === 1) ? Math.exp(-ph * 11) * 0.8 : 0;
  const hp = beat * 2 - Math.floor(beat * 2);
  const hat = Math.exp(-hp * 16) * 0.5 * (n.startsWith('intro') ? 0.6 : 1);
  const voice = (n.includes('verse') || n.includes('chorus') || n.includes('drop') || n.includes('groove')) ? 0.08 + 0.16 * Math.abs(Math.sin(t * 2.1 + tr.seed)) * (hash(Math.floor(t * 3) + tr.seed) > 0.3 ? 1 : 0.3) : 0.04;
  const j = 0.85 + 0.3 * hash(Math.floor(t * 60) + tr.seed * 13);
  return [Math.min(1, (kick * 0.95 + bass * 0.8) * e * j), Math.min(1, (voice + clap * 0.32 + 0.05) * e * j), Math.min(1, (hat * 0.8 + clap * 0.55) * e * j * 0.75)];
}

function canvas(el) {
  const r = el.getBoundingClientRect(), dpr = 2;
  el.width = Math.round(el.clientWidth * dpr); el.height = Math.round(el.clientHeight * dpr);
  const g = el.getContext('2d'); g.scale(dpr, dpr);
  return [g, el.clientWidth, el.clientHeight];
}

function alpha(hex, a) { const n = parseInt(hex.slice(1), 16); return `rgba(${n >> 16},${(n >> 8) & 255},${n & 255},${a})`; }

// The scrolling waveform, horizontal: `secs` across, the playhead at `at` (0 to 1).
function drawWave(el, tr, o = {}) {
  const [g, w, h] = canvas(el);
  const secs = o.secs ?? 9, at = o.at ?? 0.5, color = o.color ?? C.accent, bg = o.bg ?? C.bg;
  g.fillStyle = bg; g.fillRect(0, 0, w, h);
  const per = secs / w * (1 + (tr.tempo || 0) / 100);
  const px = at * w, mid = h / 2, half = h / 2 - 3;
  const x = t => px + (t - tr.now) / per;
  if (tr.loop) { const [a, z] = tr.loop; g.fillStyle = alpha(C.mint, 0.13); g.fillRect(x(a), 0, x(z) - x(a), h); g.fillStyle = alpha(C.mint, 0.85); g.fillRect(x(a), 0, 1, h); g.fillRect(x(z), 0, 1, h); }
  for (let k = 0; k < w; k++) {
    const t = tr.now + (k - px) * per;
    let m = [0, 0, 0];
    for (let s = 0; s < 3; s++) { const v = bands(tr, t + s * per / 3); m = m.map((q, i) => Math.max(q, v[i])); }
    const played = o.dimPlayed && k < px;
    const A = played ? 0.55 : 1;
    const L = m[0] * half, M = m[1] * half * 0.85, H = m[2] * half * 0.7;
    g.fillStyle = alpha(C.low, A); g.fillRect(k, mid - L, 1, L * 2);
    g.fillStyle = alpha(C.mid, A); g.fillRect(k, mid - M, 1, M * 2);
    g.fillStyle = alpha(C.high, A); g.fillRect(k, mid - H, 1, H * 2);
  }
  // The grid: bars full height (a phrase brighter), beats as ticks.
  const p = 60 / tr.bpm, t0 = tr.now - px * per, t1 = tr.now + (w - px) * per;
  for (let b = Math.floor(t0 / p); b <= Math.ceil(t1 / p); b++) {
    const xx = Math.round(x(b * p)); if (xx < 0 || xx > w) continue;
    if (b % 4 === 0) { g.fillStyle = alpha('#ffffff', b % 64 === 0 ? 0.5 : 0.2); g.fillRect(xx, 0, 1, h); }
    else { g.fillStyle = alpha('#ffffff', 0.4); g.fillRect(xx, 0, 1, 5); g.fillRect(xx, h - 5, 1, 5); }
  }
  tr.hot.forEach((c, i) => { if (c == null) return; const xx = x(c); if (xx < -14 || xx > w) return; g.fillStyle = CUES[i]; g.fillRect(xx, 0, 1, h); g.fillRect(xx, 0, 14, 14); g.fillStyle = '#0c0c0f'; g.font = '600 10px GeistMono'; g.fillText(String(i + 1), xx + 4, 11); });
  g.fillStyle = C.text; g.fillRect(px - 1, 0, 2, h);
  g.fillStyle = color; g.fillRect(px - 5, 0, 10, 3); g.fillRect(px - 5, h - 3, 10, 3);
}

// The same, vertical: time runs up, the playhead across at `at` from the top.
function drawWaveV(el, tr, o = {}) {
  const [g, w, h] = canvas(el);
  const secs = o.secs ?? 9, at = o.at ?? 0.7, color = o.color ?? C.accent;
  g.fillStyle = C.bg; g.fillRect(0, 0, w, h);
  const per = secs / h * (1 + (tr.tempo || 0) / 100), py = at * h, mid = w / 2, half = w / 2 - 3;
  const y = t => py - (t - tr.now) / per;
  if (tr.loop) { const [a, z] = tr.loop; g.fillStyle = alpha(C.mint, 0.13); g.fillRect(0, y(z), w, y(a) - y(z)); }
  for (let k = 0; k < h; k++) {
    const t = tr.now + (py - k) * per; let m = [0, 0, 0];
    for (let s = 0; s < 3; s++) { const v = bands(tr, t + s * per / 3); m = m.map((q, i) => Math.max(q, v[i])); }
    const L = m[0] * half, M = m[1] * half * 0.85, H = m[2] * half * 0.7, A = k > py ? 0.55 : 1;
    g.fillStyle = alpha(C.low, A); g.fillRect(mid - L, k, L * 2, 1);
    g.fillStyle = alpha(C.mid, A); g.fillRect(mid - M, k, M * 2, 1);
    g.fillStyle = alpha(C.high, A); g.fillRect(mid - H, k, H * 2, 1);
  }
  const p = 60 / tr.bpm, t0 = tr.now - (h - py) * per, t1 = tr.now + py * per;
  for (let b = Math.floor(t0 / p); b <= Math.ceil(t1 / p); b++) {
    const yy = Math.round(y(b * p)); if (yy < 0 || yy > h) continue;
    if (b % 4 === 0) { g.fillStyle = alpha('#ffffff', 0.22); g.fillRect(0, yy, w, 1); } else { g.fillStyle = alpha('#ffffff', 0.4); g.fillRect(0, yy, 5, 1); g.fillRect(w - 5, yy, 5, 1); }
  }
  tr.hot.forEach((c, i) => { if (c == null) return; const yy = y(c); if (yy < 0 || yy > h) return; g.fillStyle = CUES[i]; g.fillRect(0, yy, w, 1); g.fillRect(0, yy - 13, 14, 13); g.fillStyle = '#0c0c0f'; g.font = '600 10px GeistMono'; g.fillText(String(i + 1), 4, yy - 3); });
  g.fillStyle = C.text; g.fillRect(0, py - 1, w, 2); g.fillStyle = color; g.fillRect(0, py - 5, 3, 10); g.fillRect(w - 3, py - 5, 3, 10);
}

// The whole track small: played dim, phrases along the bottom, cues, the playhead.
function drawOverview(el, tr, o = {}) {
  const [g, w, h] = canvas(el);
  const color = o.color ?? C.accent, strip = o.phrases === false ? 0 : 5;
  g.fillStyle = o.bg ?? C.well; g.fillRect(0, 0, w, h);
  const mid = (h - strip) / 2, half = (h - strip) / 2 - 2, x = t => t / tr.len * w;
  for (let k = 0; k < w; k++) {
    const t0 = k / w * tr.len, t1 = (k + 1) / w * tr.len; let m = [0, 0, 0];
    for (let s = 0; s < 6; s++) { const v = bands(tr, t0 + (t1 - t0) * s / 6); m = m.map((q, i) => Math.max(q, v[i])); }
    const A = x(tr.now) > k ? 0.38 : 0.95;
    g.fillStyle = alpha(C.low, A); g.fillRect(k, mid - m[0] * half, 1, m[0] * half * 2);
    g.fillStyle = alpha(C.mid, A); g.fillRect(k, mid - m[1] * half * 0.85, 1, m[1] * half * 1.7);
    g.fillStyle = alpha(C.high, A); g.fillRect(k, mid - m[2] * half * 0.7, 1, m[2] * half * 1.4);
  }
  if (strip) for (const [n, a, z] of tr.phrases) { g.fillStyle = phraseColor(n); g.fillRect(x(a), h - 3, x(z) - x(a) - 1.5, 3); }
  tr.hot.forEach((c, i) => { if (c == null) return; g.fillStyle = CUES[i]; g.fillRect(x(c), 0, 1, h - strip); g.fillRect(x(c), 0, 9, 9); });
  g.fillStyle = color; g.fillRect(x(tr.now) - 1, 0, 2, h);
}

function phraseColor(n) { n = n.toLowerCase(); return n.startsWith('intro') || n.startsWith('outro') ? C.slate : n.startsWith('drop') || n.startsWith('chorus') ? C.coral : n.startsWith('build') ? C.amber : n.startsWith('break') || n.startsWith('bridge') ? C.violet : C.sky; }

// A knob: an arc lit to its value round a disc, the disc's pointer; bipolar ones light from the top.
function knob(v, size = 30, color = C.text2, bipolar = false, o = {}) {
  const r = size / 2 - 2, c = size / 2, t = size >= 34 ? 3 : 2.5;
  const at = (deg, rr) => [c + rr * Math.cos(deg * Math.PI / 180), c + rr * Math.sin(deg * Math.PI / 180)];
  const arc = (a0, a1) => { const [x0, y0] = at(a0, r), [x1, y1] = at(a1, r); const large = Math.abs(a1 - a0) > 180 ? 1 : 0; return `M${x0} ${y0} A${r} ${r} 0 ${large} 1 ${x1} ${y1}`; };
  const [from, to] = bipolar ? [270, 270 + v * 135] : [135, 135 + v * 270];
  const lit = Math.abs(to - from) > 0.5 ? `<path d="${arc(Math.min(from, to), Math.max(from, to))}" stroke="${color}" stroke-width="${t}" fill="none" stroke-linecap="round"/>` : '';
  const [px0, py0] = at(to, r * 0.2), [px1, py1] = at(to, r * 0.62);
  const body = o.body ?? C.raised, ring = o.ring ?? C.raised;
  return `<svg width="${size}" height="${size}" style="flex:none;display:block">
    <path d="${arc(135, 405 - 0.01)}" stroke="${ring}" stroke-width="${t}" fill="none" stroke-linecap="round"/>${lit}
    <circle cx="${c}" cy="${c}" r="${r * 0.7}" fill="${body}"/>${o.cap ? `<circle cx="${c}" cy="${c}" r="${r * 0.7}" fill="url(#cap)"/>` : ''}
    <line x1="${px0}" y1="${py0}" x2="${px1}" y2="${py1}" stroke="${C.text}" stroke-width="${size >= 34 ? 2.2 : 1.8}" stroke-linecap="round"/></svg>`;
}

// A stereo LED meter, `segs` tall: green to amber at -12, red from -3.
function meter(l, r, h, o = {}) {
  const segs = o.segs ?? 20, w = o.w ?? 3, gap = 1, sh = (h - gap * (segs - 1)) / segs;
  let s = `<svg width="${w * 2 + 2}" height="${h}" style="flex:none">`;
  [l, r].forEach((v, side) => {
    for (let k = 0; k < segs; k++) {
      const db = -48 + (k + 1) * 51 / segs, on = k < v * segs;
      const col = db > -3 ? C.red : db > -12 ? C.amber : C.mint;
      s += `<rect x="${side * (w + 2)}" y="${h - (k + 1) * (sh + gap) + gap}" width="${w}" height="${sh}" rx="0.5" fill="${on ? col : alpha('#2b2b33', 0.9)}"/>`;
    }
  });
  return s + '</svg>';
}

// A fader `h` tall: its slot, a scale, its cap where it's set (0 bottom, 1 top).
function fader(v, h, o = {}) {
  const w = o.w ?? 30, cap = o.cap ?? 18, top = (1 - v) * (h - cap), scale = o.scale ?? true;
  let ticks = '';
  if (scale) for (let k = 0; k <= 10; k++) { const y = cap / 2 + k * (h - cap) / 10; ticks += `<div style="position:absolute;left:${k % 5 === 0 ? 2 : 5}px;top:${y}px;width:${k % 5 === 0 ? 7 : 4}px;height:1px;background:${C.lineStrong}"></div><div style="position:absolute;right:${k % 5 === 0 ? 2 : 5}px;top:${y}px;width:${k % 5 === 0 ? 7 : 4}px;height:1px;background:${C.lineStrong}"></div>`; }
  return `<div style="position:relative;width:${w}px;height:${h}px;flex:none">${ticks}
    <div style="position:absolute;left:${w / 2 - 2}px;top:${cap / 2}px;width:4px;height:${h - cap}px;border-radius:2px;background:#060608;border:1px solid ${C.line}"></div>
    ${o.center ? `<div style="position:absolute;left:0;right:0;top:${h / 2}px;height:1px;background:${C.text3}"></div>` : ''}
    <div style="position:absolute;left:${(w - (o.capW ?? w - 6)) / 2}px;top:${top}px;width:${o.capW ?? w - 6}px;height:${cap}px;border-radius:3px;background:linear-gradient(#3a3a44,#24242b);border:1px solid #4a4a55;box-shadow:0 3px 8px #0008">
      <div style="position:absolute;left:3px;right:3px;top:${cap / 2 - 0.5}px;height:1px;background:${o.capLine ?? C.text}"></div></div></div>`;
}

// A crossfader `w` wide at `v` (-1 to 1).
function xfader(v, w, o = {}) {
  const cap = o.cap ?? 22, left = (v + 1) / 2 * (w - cap);
  let ticks = ''; for (let k = 0; k <= 8; k++) ticks += `<div style="position:absolute;top:2px;left:${cap / 2 + k * (w - cap) / 8}px;width:1px;height:${k % 4 === 0 ? 7 : 4}px;background:${C.lineStrong}"></div>`;
  return `<div style="position:relative;width:${w}px;height:30px;flex:none">${ticks}
    <div style="position:absolute;top:13px;left:${cap / 2}px;width:${w - cap}px;height:4px;border-radius:2px;background:#060608;border:1px solid ${C.line}"></div>
    <div style="position:absolute;top:4px;left:${left}px;width:${cap}px;height:22px;border-radius:3px;background:linear-gradient(90deg,#3a3a44,#24242b);border:1px solid #4a4a55;box-shadow:0 3px 8px #0008"><div style="position:absolute;top:3px;bottom:3px;left:${cap / 2 - 0.5}px;width:1px;background:${C.text}"></div></div></div>`;
}

// Lucide-style icons, as the app draws them.
const I = {
  play: '<path fill="currentColor" stroke="none" d="M8 5.6v12.8a1 1 0 0 0 1.53.85l10.2-6.4a1 1 0 0 0 0-1.7L9.53 4.75A1 1 0 0 0 8 5.6z"/>',
  pause: '<rect fill="currentColor" stroke="none" x="6.5" y="5" width="4" height="14" rx="1.2"/><rect fill="currentColor" stroke="none" x="13.5" y="5" width="4" height="14" rx="1.2"/>',
  cue: '<path d="M6 5v14M18 6.5l-8.5 5.5 8.5 5.5z"/>',
  sync: '<path d="M4 12a8 8 0 0 1 14-5.3M20 12a8 8 0 0 1-14 5.3M18 3v4h-4M6 21v-4h4"/>',
  lock: '<rect x="5" y="10.5" width="14" height="9.5" rx="2"/><path d="M8.5 10.5V8a3.5 3.5 0 0 1 7 0v2.5"/>',
  loop: '<path d="M4 12a5 5 0 0 1 5-5h9M15 4l3 3-3 3M20 12a5 5 0 0 1-5 5H6M9 20l-3-3 3-3"/>',
  phones: '<path d="M3 14h3a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-7a9 9 0 0 1 18 0v7a2 2 0 0 1-2 2h-1a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2h3"/>',
  search: '<circle cx="10.5" cy="10.5" r="6"/><path d="M15 15l5 5"/>',
  rec: '<circle fill="currentColor" stroke="none" cx="12" cy="12" r="5.5"/>',
  midi: '<circle cx="12" cy="12" r="8"/><circle fill="currentColor" stroke="none" cx="8.5" cy="11" r="1"/><circle fill="currentColor" stroke="none" cx="15.5" cy="11" r="1"/><circle fill="currentColor" stroke="none" cx="12" cy="8.5" r="1"/>',
  speaker: '<path d="M11 5L6.5 9H3.5v6h3l4.5 4z"/><path d="M15.5 8.5a5 5 0 0 1 0 7"/>',
  settings: '<path d="M21 4h-7M10 4H3M21 12h-9M8 12H3M21 20h-5M12 20H3M14 2v4M8 10v4M16 18v4"/>',
  magnet: '<path d="M6 4v7a6 6 0 0 0 12 0V4M6 8h4M14 8h4"/>',
  list: '<path d="M9 6.5h11M9 12h11M9 17.5h11"/><circle fill="currentColor" stroke="none" cx="4.75" cy="6.5" r="1.25"/><circle fill="currentColor" stroke="none" cx="4.75" cy="12" r="1.25"/><circle fill="currentColor" stroke="none" cx="4.75" cy="17.5" r="1.25"/>',
  plus: '<path d="M6 12h12M12 6v12"/>',
  history: '<path d="M3 12a9 9 0 1 0 3-6.7L3 8M3 3v5h5M12 7v5l3 2"/>',
  folder: '<path d="M3.5 7.5a2 2 0 0 1 2-2h3.8l2 2.2h7.2a2 2 0 0 1 2 2v7.8a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2z"/>',
  stack: '<path d="M12 3.5l8.5 4.5-8.5 4.5-8.5-4.5z"/><path d="M3.5 12l8.5 4.5 8.5-4.5"/><path d="M3.5 16l8.5 4.5 8.5-4.5"/>',
  chevron: '<path d="M6 9l6 6 6-6"/>',
  power: '<path d="M12 3v8M6.3 7.3a8 8 0 1 0 11.4 0"/>',
  slip: '<path d="M4 17h16M4 7h10l3 3-3 3"/>',
  rev: '<path d="M20 12H5M10 7l-5 5 5 5"/>',
  wave: '<path d="M3 12h2M7 8v8M10 5v14M13 9v6M16 7v10M19 11v2"/>',
  match: '<path d="M4 12h4l3-7 4 14 3-7h2"/>',
  grid: '<path d="M4 4v16M10 4v16M16 4v16M22 4v16" />',
  hand: '<path d="M18 11V6a2 2 0 0 0-4 0v1M14 10V4a2 2 0 0 0-4 0v2M10 10.5V6a2 2 0 0 0-4 0v8M18 8a2 2 0 1 1 4 0v6a8 8 0 0 1-8 8h-2c-2.8 0-4.5-.86-5.99-2.34l-3.6-3.6a2 2 0 0 1 2.83-2.82L7 15"/>',
  key: '<path d="M9 18V6l10-2v12"/><circle cx="6.5" cy="18" r="2.5"/><circle cx="16.5" cy="16" r="2.5"/>',
  stems: '<path d="M4 6h16M4 10h12M4 14h16M4 18h9"/>',
  pointer: '<path fill="#f4f4f6" stroke="#0c0c0f" stroke-width="1.2" d="M5 3l14 8-6.5 1.6L9.8 19z"/>',
};
const icon = (n, s = 15, style = '') => `<svg class="i" viewBox="0 0 24 24" style="width:${s}px;height:${s}px;${style}">${I[n]}</svg>`;

const clock = (s, tenths) => { const neg = s < 0; s = Math.abs(s); const m = Math.floor(s / 60), r = s - m * 60; return (neg ? '−' : '') + m + ':' + (tenths ? r.toFixed(1).padStart(4, '0') : String(Math.floor(r)).padStart(2, '0')); };
const beatText = b => b < 1 ? '1/' + Math.round(1 / b) : String(b);
const fits = (k, lead) => { const p = s => [parseInt(s), s.slice(-1)]; const [n, l] = p(k), [m, q] = p(lead); const d = ((n - m) % 12 + 12) % 12; return (l === q && (d === 0 || d === 1 || d === 11)) || n === m; };
