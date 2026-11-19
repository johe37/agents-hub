// Claude Town: every repo is a shop, every Claude agent works in one, and agents walk the streets to
// talk to each other. All sprites are original and defined in this file, as pixel arrays or in code.
'use strict';

const TOKEN = location.hash.slice(1);

// ------------------------------------------------------------------ palette

// 24 colours. Dusk and night are the same colours, shifted, so the town keeps its look.
const BASE = {
  grass: '#7fb069', grass2: '#6a9c58', road: '#d9c39a', road2: '#c4ab80',
  wallA: '#efe3c8', wallB: '#e3c49c', shade: '#b89a78', roofA: '#c4553f',
  roofB: '#4f7cac', roofC: '#5e9a6b', wood: '#a0703f', woodD: '#6e4a2a',
  win: '#3b4a63', lit: '#ffd86b', ink: '#2b2b3a', white: '#fbf7ef',
  skin: '#f0c6a0', red: '#e04848', yellow: '#ffd23f', smoke: '#c9c9cf',
  leaf: '#4f8a4a', leafD: '#3c6e3a', purple: '#8a6bb0', teal: '#3fa3a0',
};
// These glow at night, so they keep their colour.
const GLOW = new Set(['lit', 'yellow', 'red', 'teal']);

function mix(a, b, t) {
  const p = (h) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16));
  const [x, y] = [p(a), p(b)];
  return '#' + x.map((v, i) => Math.round(v + (y[i] - v) * t).toString(16).padStart(2, '0')).join('');
}

function palette(time) {
  if (time === 'day') return { ...BASE };
  const [to, t] = time === 'night' ? ['#1b2140', 0.55] : ['#6a4060', 0.22];
  const out = {};
  for (const [k, v] of Object.entries(BASE)) out[k] = GLOW.has(k) && time === 'night' ? v : mix(v, to, k === 'white' ? t * 0.6 : t);
  return out;
}

const dark = matchMedia('(prefers-color-scheme: dark)');
function timeOfDay() {
  if (dark.matches) return 'night';
  const h = new Date().getHours();
  if (h >= 21 || h < 5) return 'night';
  if (h >= 18 || h < 7) return 'dusk';
  return 'day';
}
let TIME = timeOfDay();
let P = palette(TIME);
function updateTime() {
  const t = timeOfDay();
  if (t !== TIME) { TIME = t; P = palette(t); }
}
dark.addEventListener('change', updateTime);
setInterval(updateTime, 60000);

// ------------------------------------------------------------------ sprites

// Pixel font, five rows high: each glyph is its rows joined, three pixels wide (five for M and W).
const FONT = {
  A: '.x.x.xxxxx.xx.x', B: 'xx.x.xxx.x.xxx.', C: '.xxx..x..x...xx', D: 'xx.x.xx.xx.xxx.',
  E: 'xxxx..xx.x..xxx', F: 'xxxx..xx.x..x..', G: '.xxx..x.xx.x.xx', H: 'x.xx.xxxxx.xx.x',
  I: 'xxx.x..x..x.xxx', J: '..x..x..xx.x.x.', K: 'x.xx.xxx.x.xx.x', L: 'x..x..x..x..xxx',
  M: 'x...xxx.xxx.x.xx...xx...x', N: 'x...xxx..xx.x.xx..xxx...x', O: '.x.x.xx.xx.x.x.', P: 'xx.x.xxx.x..x..',
  Q: '.x.x.xx.xxx..xx', R: 'xx.x.xxx.x.xx.x', S: '.xxx...x...xxx.', T: 'xxx.x..x..x..x.',
  U: 'x.xx.xx.xx.xxxx', V: 'x.xx.xx.xx.x.x.', W: 'x...xx...xx.x.xx.x.x.x.x.', X: 'x.xx.x.x.x.xx.x',
  Y: 'x.xx.x.x..x..x.', Z: 'xxx..x.x.x..xxx',
  0: 'xxxx.xx.xx.xxxx', 1: '.x.xx..x..x.xxx', 2: 'xx...x.x.x..xxx', 3: 'xx...x.x...xxx.',
  4: 'x.xx.xxxx..x..x', 5: 'xxxx..xx...xxx.', 6: '.xxx..xxxx.xxxx', 7: 'xxx..x.x..x..x.',
  8: 'xxxx.xxxxx.xxxx', 9: 'xxxx.xxxx..xxx.',
  '-': '......xxx......', _: '............xxx', '.': '.............x.', '/': '..x..x.x.x..x..',
  '!': '.x..x..x.....x.', '?': 'xx...x.x.....x.', ':': '....x.....x....', '+': '....x.xxx.x....',
  ' ': '...............', '#': 'x.xxxxx.xxxxx.x',
};

// People: h hair, s skin, c shirt, p trousers, k shoes.
const PERSON = {
  stand: ['..hhh..', '..sss..', '..sss..', '.ccccc.', 's.ccc.s', '..ccc..', '..p.p..', '..p.p..', '..k.k..'],
  wave: ['..hhh.s', '..sss.s', '..sss.c', '.ccccc.', 's.ccc..', '..ccc..', '..p.p..', '..p.p..', '..k.k..'],
  walkA: ['..hhh..', '..sss..', '..sss..', '.ccccc.', 's.ccc.s', '..ccc..', '.p...p.', '.p...p.', 'k.....k'],
  walkB: ['..hhh..', '..sss..', '..sss..', '.ccccc.', '.sccc.s', '..ccc..', '...pp..', '...pp..', '...kk..'],
  // Behind the counter: upper body; two frames of typing, and asleep with the head on the counter.
  deskA: ['.hhh.', '.sss.', '.sss.', 'ccccc', 's...s'],
  deskB: ['.hhh.', '.sss.', '.sss.', 'ccccc', '.s.s.'],
  sleep: ['.....', '.....', '.hhh.', 'cssss', 'ccccc'],
};

// Tool icons, 7x7: x main colour, y second colour, w wood.
const ICONS = {
  edit: ['.....yy', '....xyy', '...xxx.', '..xxx..', '.xxx...', 'wxx....', 'ww.....'],
  search: ['.xxx...', 'x...x..', 'x...x..', 'x...x..', '.xxxy..', '....yy.', '.....yy'],
  bash: ['...x...', '.x.x.x.', '..xxx..', 'xxx.xxx', '..xxx..', '.x.x.x.', '...x...'],
  read: ['xxx.xxx', 'xyyxyyx', 'xyyxyyx', 'xyyxyyx', 'xyyxyyx', 'xxxxxxx', '.......'],
  web: ['..xxx..', '.xyxyx.', 'xyyxyyx', 'xxxxxxx', 'xyyxyyx', '.xyxyx.', '..xxx..'],
  agent: ['...x...', '...x...', '..xyx..', 'xxyyyxx', '..xyx..', '...x...', '...x...'],
  todo: ['xxxxxxx', 'xy.yyyx', 'x.....x', 'xy.yyyx', 'x.....x', 'xy.yyyx', 'xxxxxxx'],
  other: ['.......', '.......', '.......', 'x..x..x', '.......', '.......', '.......'],
};
const ICON_COLORS = {
  edit: ['ink', 'red'], search: ['ink', 'wood'], bash: ['ink', 'ink'], read: ['roofB', 'white'],
  web: ['roofB', 'roofC'], agent: ['purple', 'yellow'], todo: ['ink', 'roofC'], other: ['ink', 'ink'],
};

const Z = ['xxxx', '..x.', '.x..', 'xxxx'];
const CRATE = ['wwwwww', 'wdwwdw', 'wwddww', 'wwddww', 'wdwwdw', 'wwwwww'];
const PARCEL = ['bbbsbbb', 'bbbsbbb', 'sssssss', 'bbbsbbb', 'bbbsbbb'];
const TREE = ['...lll...', '.lllLll..', 'lllllLll.', 'llLllllll', 'lllllllLl', '.llLllll.', '..lllll..', '....t....', '....t....', '...ttt...'];

// ------------------------------------------------------------------ canvas and camera

const stage = document.getElementById('stage');
const canvas = document.getElementById('town');
const ctx = canvas.getContext('2d');
const cam = { x: 0, y: 0, z: 0 };
let view = { w: 320, h: 180 };

function resize() {
  const dpr = window.devicePixelRatio || 1;
  const dw = Math.max(1, Math.round(stage.clientWidth * dpr));
  const dh = Math.max(1, Math.round(stage.clientHeight * dpr));
  if (!cam.z) cam.z = Math.max(2, Math.ceil(dw / 480));
  view = { w: Math.ceil(dw / cam.z), h: Math.ceil(dh / cam.z) };
  canvas.width = view.w;
  canvas.height = view.h;
  canvas.style.width = (view.w * cam.z) / dpr + 'px';
  canvas.style.height = (view.h * cam.z) / dpr + 'px';
  ctx.imageSmoothingEnabled = false;
}
new ResizeObserver(() => { resize(); draw(); }).observe(stage);

function px(x, y, w, h, color) {
  ctx.fillStyle = color;
  ctx.fillRect(Math.floor(x - cam.x), Math.floor(y - cam.y), w, h);
}

function sprite(rows, x, y, colors) {
  for (let r = 0; r < rows.length; r++) {
    for (let c = 0; c < rows[r].length; c++) {
      const col = colors[rows[r][c]];
      if (col) px(x + c, y + r, 1, 1, col);
    }
  }
}

function glyph(ch) { return FONT[ch.toUpperCase()] || FONT['?']; }
function textWidth(s) { return [...s].reduce((w, ch) => w + glyph(ch).length / 5 + 1, 0) - 1; }
function text(s, x, y, color) {
  for (const ch of s) {
    const g = glyph(ch), w = g.length / 5;
    for (let j = 0; j < g.length; j++) if (g[j] === 'x') px(x + (j % w), y + Math.floor(j / w), 1, 1, color);
    x += w + 1;
  }
}
function fit(s, n) { return s.length > n ? s.slice(0, n - 1) + '.' : s; }

function hash(s) {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) h = Math.imul(h ^ s.charCodeAt(i), 16777619);
  return h >>> 0;
}

// ------------------------------------------------------------------ layout

// Lots in a grid; a street runs under every row and a lane left of every column.
const LOT_W = 104, LOT_H = 100, TOWN_X = 14, STREET = 10;
const streetY = (row) => row * LOT_H + LOT_H - STREET + 4; // centre line
const laneX = (col) => TOWN_X + col * LOT_W - 8; // centre line

let model = null;
let shops = new Map();
let agents = new Map();
let rows = 2;

function layout() {
  shops = new Map(model.shops.map((s) => [s.id, s]));
  agents = new Map(model.agents.map((a) => [a.key, a]));
  const cols = model.columns || 4;
  let maxLot = 0;
  for (const s of model.shops) {
    s.col = s.lot % cols;
    s.row = Math.floor(s.lot / cols);
    maxLot = Math.max(maxLot, s.lot);
    s.staff = model.agents.filter((a) => a.shop === s.id);
    // One counter space per agent, so the shop grows with them.
    s.slots = Math.min(6, Math.max(1, s.staff.length));
    s.wallW = Math.min(88, 46 + 12 * (s.slots - 1));
    s.wallH = 38;
    s.x0 = TOWN_X + s.col * LOT_W;
    s.y0 = s.row * LOT_H;
    s.cx = s.x0 + Math.floor(LOT_W / 2) - 4;
    s.baseY = s.y0 + 64;
    s.left = s.cx - Math.floor(s.wallW / 2);
    s.right = s.left + s.wallW;
    s.top = s.baseY - s.wallH;
    s.doorX = s.right - 12;
    s.style = hash(s.id);
    const busy = s.terminals.some((t) => t.kind !== 'shell');
    s.lit = s.staff.some((a) => a.state !== 'asleep') || busy;
  }
  rows = Math.max(2, Math.floor(maxLot / cols) + 1);
}

function townBounds() {
  const cols = model ? model.columns || 4 : 4;
  return { x: 0, y: -10, w: TOWN_X + cols * LOT_W, h: rows * LOT_H + 10 };
}

let centered = false;
function centerOnce() {
  if (centered || !model) return;
  centered = true;
  const b = townBounds();
  const dpr = window.devicePixelRatio || 1;
  const dw = stage.clientWidth * dpr, dh = stage.clientHeight * dpr;
  cam.z = Math.max(2, Math.min(Math.floor(dw / (b.w + 16)), Math.floor(dh / (b.h + 16))));
  resize();
  cam.x = Math.round(b.x + b.w / 2 - view.w / 2);
  cam.y = Math.round(b.y + b.h / 2 - view.h / 2);
}

// ------------------------------------------------------------------ walks between shops

const WALK_SPEED = 16; // world pixels per second: calm
const TALK_MS = 4500;
const played = new Set();
const queues = new Map(); // agent key -> messages it still has to deliver
const walking = new Map(); // agent key -> its walk now
let firstModel = true;

function door(shop) { return { x: shop.doorX + 4, y: shop.baseY + 4 }; }

// Along the streets: down to my street, along it (through a lane to another row if needed), up to their door.
function route(from, to) {
  const a = door(from), b = door(to);
  const pts = [a, { x: a.x, y: streetY(from.row) }];
  if (from.row !== to.row) {
    let best = 0, cost = Infinity;
    for (let c = 0; c <= (model.columns || 4); c++) {
      const d = Math.abs(a.x - laneX(c)) + Math.abs(b.x - laneX(c));
      if (d < cost) { cost = d; best = c; }
    }
    pts.push({ x: laneX(best), y: streetY(from.row) }, { x: laneX(best), y: streetY(to.row) });
  }
  // Stop beside the door, left of it: the right side has the parcels and whoever waits for permission.
  pts.push({ x: b.x - 13, y: streetY(to.row) }, { x: b.x - 13, y: b.y });
  return pts;
}

function lengthOf(pts) {
  let d = 0;
  for (let i = 1; i < pts.length; i++) d += Math.abs(pts[i].x - pts[i - 1].x) + Math.abs(pts[i].y - pts[i - 1].y);
  return d;
}

function pointAt(pts, dist) {
  for (let i = 1; i < pts.length; i++) {
    const seg = Math.abs(pts[i].x - pts[i - 1].x) + Math.abs(pts[i].y - pts[i - 1].y);
    if (dist <= seg) {
      const t = seg ? dist / seg : 1;
      return { x: Math.round(pts[i - 1].x + (pts[i].x - pts[i - 1].x) * t), y: Math.round(pts[i - 1].y + (pts[i].y - pts[i - 1].y) * t) };
    }
    dist -= seg;
  }
  return pts[pts.length - 1];
}

function takeMessages() {
  const now = Date.now() / 1000;
  for (const m of model.messages || []) {
    if (played.has(m.id)) continue;
    played.add(m.id);
    // On load, older messages are history, not news.
    if (firstModel && now - m.time > 20) continue;
    if (!queues.has(m.from)) queues.set(m.from, []);
    queues.get(m.from).push(m);
  }
  firstModel = false;
}

function stepWalks(t) {
  for (const [key, q] of queues) {
    if (walking.has(key) || !q.length) continue;
    const m = q.shift();
    const a = agents.get(m.from), b = agents.get(m.to);
    const from = a && shops.get(a.shop), to = b && shops.get(b.shop);
    if (!from || !to) continue;
    const out = route(from, to);
    walking.set(key, { m, out, back: [...out].reverse(), len: lengthOf(out), start: t });
  }
  for (const [key, w] of walking) {
    const outMs = (w.len / WALK_SPEED) * 1000;
    const el = t - w.start;
    if (el < outMs) { w.phase = 'out'; w.pos = pointAt(w.out, (el / 1000) * WALK_SPEED); }
    else if (el < outMs + TALK_MS) { w.phase = 'talk'; w.pos = w.out[w.out.length - 1]; }
    else if (el < outMs * 2 + TALK_MS) { w.phase = 'back'; w.pos = pointAt(w.back, ((el - outMs - TALK_MS) / 1000) * WALK_SPEED); }
    else walking.delete(key);
  }
}

// ------------------------------------------------------------------ drawing

let hits = [];
const agentPos = new Map();
let hover = null;
let selected = null;
let lastFrame = 0;

function drawGround() {
  const T = 8;
  const x0 = Math.floor(cam.x / T) * T, y0 = Math.floor(cam.y / T) * T;
  px(cam.x, cam.y, view.w + 1, view.h + 1, P.grass);
  for (let y = y0; y < cam.y + view.h; y += T) {
    for (let x = x0; x < cam.x + view.w; x += T) {
      const h = hash(x + ',' + y);
      if (h % 7 === 0) px(x + (h >>> 3) % 6, y + (h >>> 6) % 6, 1, 2, P.grass2);
      else if (h % 53 === 0) px(x + (h >>> 4) % 7, y + (h >>> 8) % 7, 1, 1, h % 2 ? P.white : P.yellow);
    }
  }
  const b = townBounds(), cols = model.columns || 4;
  for (let r = 0; r < rows; r++) {
    const y = streetY(r) - 4;
    px(laneX(0) - 4, y, b.w - laneX(0) + 4, STREET - 2, P.road);
    px(laneX(0) - 4, y + STREET - 3, b.w - laneX(0) + 4, 1, P.road2);
  }
  for (let c = 0; c <= cols; c++) {
    px(laneX(c) - 4, -10, 8, rows * LOT_H + 4, P.road);
    px(laneX(c) + 3, -10, 1, rows * LOT_H + 4, P.road2);
  }
}

function drawTree(x, y) { sprite(TREE, x, y, { l: P.leaf, L: P.leafD, t: P.woodD }); }

function drawEmptyLots() {
  const cols = model.columns || 4;
  const used = new Set(model.shops.map((s) => s.lot));
  for (let lot = 0; lot < rows * cols; lot++) {
    if (used.has(lot)) continue;
    const x0 = TOWN_X + (lot % cols) * LOT_W, y0 = Math.floor(lot / cols) * LOT_H;
    const h = hash('lot' + lot);
    for (let i = 0; i < 1 + (h % 2); i++) drawTree(x0 + 8 + ((h >>> (i * 5)) % 70), y0 + 14 + ((h >>> (i * 3 + 2)) % 50));
  }
}

function shirt(key) { return P[['roofA', 'roofB', 'roofC', 'purple', 'teal', 'yellow'][hash(key) % 6]]; }
function hair(key) { return [P.woodD, P.ink, P.wood, P.yellow][(hash(key) >>> 4) % 4]; }
function personColors(a) { return { h: hair(a.key), s: P.skin, c: shirt(a.key), p: P.win, k: P.ink }; }

function hit(kind, id, x, y, w, h) { hits.push({ kind, id, x, y, w, h }); }

function bubble(x, y, label, bg, fg) {
  // A speech bubble whose tail points down at (x, y).
  const bx = x - 3, by = y - 11;
  px(bx, by, 7, 9, P.ink);
  px(bx + 1, by + 1, 5, 7, bg);
  px(x - 1, by + 9, 3, 1, P.ink);
  px(x, by + 10, 1, 1, P.ink);
  text(label, bx + 2, by + 2, fg);
}

function talkBubble(x, y) {
  // A wider bubble with three dots: someone is talking.
  const bx = x - 6, by = y - 11;
  px(bx, by, 13, 9, P.ink);
  px(bx + 1, by + 1, 11, 7, P.white);
  for (let i = 0; i < 3; i++) px(bx + 3 + i * 3, by + 4, 1, 1, P.ink);
  px(x - 1, by + 9, 3, 1, P.ink);
  px(x, by + 10, 1, 1, P.ink);
}

function alarm(x, y, blink) {
  // Twice the size of the other bubbles, with a thick '!'. The only thing in town that blinks.
  const bx = x - 5, by = y - 15;
  px(bx, by, 11, 13, P.ink);
  px(bx + 1, by + 1, 9, 11, blink ? P.yellow : P.red);
  px(x - 1, by + 13, 3, 1, P.ink);
  px(x, by + 14, 1, 1, P.ink);
  const c = blink ? P.red : P.white;
  px(x - 1, by + 3, 3, 5, c);
  px(x - 1, by + 9, 3, 2, c);
}

// z z z rising from (x, y) by rise pixels, so they stay inside the shop window and off the sign.
function zzz(x, y, t, seed, rise) {
  for (let i = 0; i < 3; i++) {
    const phase = ((t / 2600 + i / 3 + (seed % 7) / 7) % 1);
    if (phase < 0.9) sprite(Z, Math.round(x + phase * 4), Math.round(y - phase * rise), { x: P.white });
  }
}

function toolIcon(kind, x, y) {
  px(x - 1, y - 1, 9, 9, P.ink);
  px(x, y, 7, 7, P.white);
  const [m, s] = ICON_COLORS[kind] || ICON_COLORS.other;
  sprite(ICONS[kind] || ICONS.other, x, y, { x: P[m], y: P[s], w: P.wood });
}

function marker(x, y, w, h) {
  for (const [cx, cy, dx, dy] of [[x, y, 1, 1], [x + w, y, -1, 1], [x, y + h, 1, -1], [x + w, y + h, -1, -1]]) {
    px(dx > 0 ? cx : cx - 3, cy, 4, 1, P.yellow);
    px(cx - (dx > 0 ? 0 : 1), dy > 0 ? cy : cy - 3, 1, 4, P.yellow);
  }
}

function drawShop(s, t) {
  const git = s.git;
  const wall = [P.wallA, P.wallB][s.style % 2];
  const awning = git ? [P.roofA, P.roofB, P.roofC, P.purple][(s.style >>> 3) % 4] : P.wood;
  const winL = s.left + 3, winR = s.doorX - 3, winTop = s.top + 18, winBot = s.baseY - 3;

  // Path from the door to the street.
  px(s.doorX, s.baseY, 9, streetY(s.row) - 4 - s.baseY, P.road);

  // Building: wall, shaded right edge, parapet.
  px(s.left, s.top, s.wallW, s.wallH, wall);
  px(s.right - 3, s.top, 3, s.wallH, P.shade);
  px(s.left - 1, s.top - 2, s.wallW + 2, 3, P.shade);

  // Chimneys, one per terminal that is not an editor; smoke when something runs in it.
  s.terminals.filter((x) => x.kind !== 'editor').slice(0, 3).forEach((term, i) => {
    const x = s.left + 5 + i * 7;
    px(x, s.top - 8, 4, 6, P.woodD);
    px(x - 1, s.top - 9, 6, 2, P.shade);
    if (term.kind === 'busy') {
      for (let j = 0; j < 3; j++) {
        const ph = ((t / 3000 + j / 3 + i * 0.17) % 1);
        const size = ph < 0.5 ? 2 : 1;
        px(x + 1 + Math.round(ph * 5 + Math.sin(ph * 5)), s.top - 11 - Math.round(ph * 14), size, size, P.smoke);
      }
    }
  });

  // Sign board with the name, above the awning.
  const name = fit(s.name, Math.floor((s.wallW - 6) / 4));
  px(s.left + 2, s.top + 2, s.wallW - 4, 9, P.woodD);
  px(s.left + 3, s.top + 3, s.wallW - 6, 7, P.wallA);
  text(name, s.cx - Math.floor(textWidth(name) / 2), s.top + 4, P.ink);

  // Striped awning with a scalloped edge.
  for (let x = s.left - 1; x < s.right + 1; x++) {
    const c = Math.floor((x - s.left) / 3) % 2 ? P.white : awning;
    px(x, s.top + 12, 1, 4, c);
    if ((x - s.left) % 3 !== 2) px(x, s.top + 16, 1, 1, c);
  }

  // Shop window: the counter, one workbench (a small screen) per agent, and the staff behind it.
  px(winL - 1, winTop - 1, winR - winL + 2, winBot - winTop + 2, P.woodD);
  px(winL, winTop, winR - winL, winBot - winTop, s.lit ? P.lit : P.win);
  const counterY = winBot - 5;
  const inside = s.staff.filter((a) => a.state !== 'permission' && !walking.has(a.key));
  const span = winR - winL;
  for (let i = 0; i < s.slots; i++) {
    const x = winL + Math.round((span * (i + 0.5)) / s.slots) - 4;
    const a = inside[i];
    if (a) {
      const frame = a.state === 'asleep' ? PERSON.sleep : a.state === 'working' && Math.floor(t / 380 + (hash(a.key) % 5)) % 2 ? PERSON.deskA : PERSON.deskB;
      sprite(frame, x + 1, counterY - 5, personColors(a));
      agentPos.set(a.key, { x: x + 3, y: counterY - 3 });
      hit('agent', a.key, x - 1, winTop, 9, winBot - winTop);
      if (a.state === 'asleep') zzz(x + 4, counterY - 9, t, hash(a.key), counterY - 9 - winTop - 1);
      if (a.state === 'waiting') bubble(x + 3, counterY - 6, '?', P.white, P.ink);
      if (selected && selected.id === a.key) marker(x - 2, winTop - 2, 11, winBot - winTop + 4);
    }
    // Workbench screen, lit while its agent works.
    px(x + 5, counterY - 3, 3, 2, a && a.state === 'working' ? P.teal : P.ink);
    px(x + 6, counterY - 1, 1, 1, P.ink);
  }
  px(winL, counterY, winR - winL, 2, P.wood);
  px(winL, counterY + 2, winR - winL, winBot - counterY - 2, P.woodD);

  // Door with a glass pane.
  px(s.doorX, s.baseY - 15, 9, 15, P.woodD);
  px(s.doorX + 1, s.baseY - 14, 7, 14, P.wood);
  px(s.doorX + 2, s.baseY - 12, 5, 5, s.lit ? P.lit : P.win);
  px(s.doorX + 6, s.baseY - 6, 1, 1, P.yellow);

  // Tool icons above the sign, one per working agent; an alarm over the roof when someone needs permission.
  const working = inside.filter((a) => a.state === 'working');
  working.forEach((a, i) => toolIcon(a.tool_kind || 'other', Math.round(s.cx - (working.length * 11) / 2 + i * 11 + 2), s.top - 13 - (Math.floor(t / 900 + i) % 2)));
  const blink = Math.floor(t / 450) % 2 === 0;
  const atDoor = s.staff.filter((a) => a.state === 'permission' && !walking.has(a.key));
  if (atDoor.length) alarm(s.right - 4, s.top - (working.length ? 15 : 3), blink);

  // Git on the shop front: crates for uncommitted changes, parcels going out (ahead) and coming in (behind).
  if (git) {
    for (let i = 0; i < Math.min(git.dirty, 6); i++) {
      const x = s.left - 8 + (i % 3) * 7 - (i >= 3 ? 3 : 0), y = s.baseY - 6 - (i >= 3 ? 6 : 0);
      sprite(CRATE, x, y, { w: P.wood, d: P.woodD });
    }
    const parcels = (n, x, tag) => {
      for (let i = 0; i < Math.min(n, 3); i++) sprite(PARCEL, x, s.baseY + 1 - i * 5, { b: P.wallB, s: P.woodD });
      if (n > 0) px(x + 6, s.baseY - Math.min(n, 3) * 5 + 3, 2, 2, tag);
    };
    parcels(git.ahead, s.right + 2, P.red); // outgoing, waiting for the post: push
    parcels(git.behind, s.doorX - 10, P.roofC); // incoming, delivered but not unpacked: pull
  }

  // Lamp posts, one per open editor.
  s.terminals.filter((x) => x.kind === 'editor').slice(0, 2).forEach((_, i) => {
    const x = i ? s.x0 + LOT_W - 14 : s.x0 + 2, y = s.baseY - 14;
    if (TIME !== 'day') for (let dy = -3; dy <= 3; dy++) for (let dx = -5; dx <= 5; dx++) if ((dx + dy) % 2 === 0 && dx * dx + dy * dy * 3 < 26) px(x + dx, y + 18 + dy, 1, 1, P.lit);
    px(x, y + 2, 1, 16, P.ink);
    px(x - 1, y, 3, 3, P.ink);
    px(x, y + 1, 1, 1, TIME === 'day' ? P.white : P.lit);
  });

  // Agents that need permission come out of the door, wave and call.
  atDoor.forEach((a, i) => {
    const x = s.doorX + 1 + i * 9, y = s.baseY + 2;
    sprite(blink ? PERSON.wave : PERSON.stand, x, y, personColors(a));
    alarm(x + 3, y - 1, blink);
    agentPos.set(a.key, { x: x + 3, y });
    hit('agent', a.key, x - 2, y - 16, 11, 25);
    if (selected && selected.id === a.key) marker(x - 4, y - 18, 15, 29);
  });

  if (hover && hover.kind === 'shop' && hover.id === s.id && git && git.branch) {
    const label = fit(git.branch, 22);
    const w = textWidth(label) + 8, x = Math.round(s.cx - w / 2), y = s.top - (working.length ? 26 : 14) - (atDoor.length ? 14 : 0);
    px(x, y, w, 9, P.purple);
    px(x + 1, y + 1, w - 2, 7, mix(P.purple, P.white, 0.15));
    text(label, x + 4, y + 2, P.white);
  }
  if (selected && selected.id === s.id) marker(s.left - 4, s.top - 4, s.wallW + 8, s.wallH + 8);
  hit('shop', s.id, s.left - 2, s.top - 2, s.wallW + 4, s.wallH + 4);
}

function drawWalkers(t) {
  for (const [key, w] of walking) {
    const a = agents.get(key);
    if (!a || !w.pos) continue;
    const frame = w.phase === 'talk' ? PERSON.stand : Math.floor(t / 220) % 2 ? PERSON.walkA : PERSON.walkB;
    const x = w.pos.x - 3, y = w.pos.y - 9;
    sprite(frame, x, y, personColors(a));
    if (w.phase === 'talk') talkBubble(w.pos.x, y - 1);
    else if (w.phase === 'out') px(x + 6, y + 4, 3, 2, P.white); // a letter under the arm
    agentPos.set(key, { x: w.pos.x, y: w.pos.y - 4 });
    hit('agent', key, x - 1, y - 2, 9, 12);
    if (selected && selected.id === key) marker(x - 2, y - 3, 11, 14);
  }
}

function draw() {
  if (!model) return;
  const t = performance.now();
  lastFrame = t;
  hits = [];
  stepWalks(t);
  ctx.clearRect(0, 0, view.w, view.h);
  drawGround();
  drawEmptyLots();
  [...shops.values()].sort((a, b) => a.y0 - b.y0 || a.x0 - b.x0).forEach((s) => drawShop(s, t));
  drawWalkers(t);
  // The panel shows the message of a walking agent; refresh it when a walk starts or ends.
  const w = selected && selected.kind === 'agent' ? walking.get(selected.id) : null;
  if ((w ? w.m.id : null) !== panelWalk) renderPanel(true);
}

// Calm frame rate: the town needs only a few frames a second.
function loop() {
  if (performance.now() - lastFrame > 110) draw();
  requestAnimationFrame(loop);
}

// ------------------------------------------------------------------ data

const counter = document.getElementById('counter');
const conn = document.getElementById('conn');

function waitingAgents() {
  const order = { permission: 0, waiting: 1 };
  return model.agents.filter((a) => a.state in order).sort((a, b) => order[a.state] - order[b.state] || a.ts - b.ts);
}

function updateCounter() {
  const n = waitingAgents().length;
  counter.textContent = n === 0 ? 'No agents waiting for you' : `${n} agent${n === 1 ? '' : 's'} waiting for you`;
  counter.classList.toggle('hot', n > 0);
  document.title = n ? `(${n}) Claude Town` : 'Claude Town';
}

let waitIndex = 0;
counter.addEventListener('click', () => {
  const list = model ? waitingAgents() : [];
  if (!list.length) return;
  const a = list[waitIndex++ % list.length];
  draw();
  const p = agentPos.get(a.key);
  if (p) { cam.x = Math.round(p.x - view.w / 2); cam.y = Math.round(p.y - view.h / 2); }
  select({ kind: 'agent', id: a.key });
});

function applyTheme(t) {
  for (const [k, v] of Object.entries(t || {})) if (v && v !== 'default') document.documentElement.style.setProperty('--' + k, v);
}

function connect() {
  const es = new EventSource('/events?t=' + encodeURIComponent(TOKEN));
  es.onmessage = (e) => {
    conn.hidden = true;
    model = JSON.parse(e.data);
    document.getElementById('demo').hidden = !model.demo;
    applyTheme(model.theme);
    layout();
    takeMessages();
    centerOnce();
    updateCounter();
    if (selected && !(selected.kind === 'agent' ? agents.has(selected.id) : shops.has(selected.id))) select(null);
    else renderPanel(false);
    draw();
  };
  es.onerror = () => { conn.hidden = false; };
}

// ------------------------------------------------------------------ mouse

const tip = document.getElementById('tip');
let drag = null;

function worldAt(e) {
  const r = canvas.getBoundingClientRect();
  return { x: cam.x + ((e.clientX - r.left) / r.width) * view.w, y: cam.y + ((e.clientY - r.top) / r.height) * view.h };
}

function hitAt(e) {
  const w = worldAt(e);
  const inside = (h) => w.x >= h.x && w.x < h.x + h.w && w.y >= h.y && w.y < h.y + h.h;
  // Agents first, newest drawn first: walkers are drawn last and stand in front of shops.
  return [...hits].reverse().find((h) => h.kind === 'agent' && inside(h)) || hits.find((h) => h.kind === 'shop' && inside(h)) || null;
}

stage.addEventListener('pointerdown', (e) => {
  if (e.target !== canvas) return;
  drag = { x: e.clientX, y: e.clientY, cx: cam.x, cy: cam.y, moved: false };
  stage.setPointerCapture(e.pointerId);
});

stage.addEventListener('pointermove', (e) => {
  if (drag) {
    const dx = e.clientX - drag.x, dy = e.clientY - drag.y;
    if (Math.abs(dx) + Math.abs(dy) > 3) { drag.moved = true; stage.classList.add('dragging'); }
    if (drag.moved) {
      const scale = (window.devicePixelRatio || 1) / cam.z;
      cam.x = Math.round(drag.cx - dx * scale);
      cam.y = Math.round(drag.cy - dy * scale);
      tip.hidden = true;
      draw();
      return;
    }
  }
  const h = hitAt(e);
  if ((h && h.kind + h.id) !== (hover && hover.kind + hover.id)) { hover = h; draw(); }
  stage.classList.toggle('pointing', !!h);
  showTip(h, e);
});

stage.addEventListener('pointerleave', () => { hover = null; tip.hidden = true; });

stage.addEventListener('pointerup', (e) => {
  if (!drag) return;
  const moved = drag.moved;
  drag = null;
  stage.classList.remove('dragging');
  if (!moved) select(hitAt(e));
});

stage.addEventListener('wheel', (e) => {
  e.preventDefault();
  const before = worldAt(e);
  const z = Math.min(10, Math.max(1, cam.z + (e.deltaY < 0 ? 1 : -1)));
  if (z === cam.z) return;
  cam.z = z;
  resize();
  const after = worldAt(e);
  cam.x = Math.round(cam.x + before.x - after.x);
  cam.y = Math.round(cam.y + before.y - after.y);
  draw();
}, { passive: false });

function el(tag, cls, textContent) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (textContent !== undefined) e.textContent = textContent;
  return e;
}

function showTip(h, e) {
  if (!h || drag) { tip.hidden = true; return; }
  tip.replaceChildren();
  if (h.kind === 'shop') {
    const s = shops.get(h.id);
    tip.append(el('b', '', s.name), el('div', 'dim', s.path));
    if (s.git) {
      const g = s.git;
      tip.append(el('div', '', '⎇ ' + (g.branch || '-')));
      tip.append(el('div', '', g.dirty ? `${g.dirty} changed, +${g.added} −${g.removed}` : 'No uncommitted changes'));
      if (g.ahead || g.behind) tip.append(el('div', '', `${g.ahead} to push, ${g.behind} to pull`));
    } else tip.append(el('div', 'dim', 'Not a git repository'));
  } else {
    const a = agents.get(h.id);
    tip.append(el('b', '', a.title));
    const w = walking.get(h.id);
    if (w) tip.append(el('div', 'dim', `To ${agents.get(w.m.to)?.title || 'another agent'}: “${w.m.preview}”`));
  }
  tip.hidden = false;
  const r = stage.getBoundingClientRect();
  tip.style.left = Math.max(8, Math.min(e.clientX - r.left + 14, r.width - tip.offsetWidth - 8)) + 'px';
  tip.style.top = Math.max(8, Math.min(e.clientY - r.top + 14, r.height - tip.offsetHeight - 8)) + 'px';
}

// ------------------------------------------------------------------ the hub's detail panel

// The content (title, banner, buttons, tabs, body, meta) is built by the hub's own panel code and arrives as
// styled lines of [text, fg, bg, flags] spans; flags: 1 bold, 2 italic, 4 underline, 8 dim.
const panel = document.getElementById('panel');
const $ = (id) => document.getElementById(id);
const replyForm = $('p-reply'), input = $('p-input'), note = $('p-note');
let tab = 0, shown = '', panelWalk = null;

$('p-close').addEventListener('click', () => select(null));

function spans(line, parent) {
  for (const [t, fg, bg, flags] of line) {
    const s = el('span', '', t);
    if (fg) s.style.color = fg;
    if (bg) s.style.background = bg;
    if (flags & 1) s.style.fontWeight = '700';
    if (flags & 2) s.style.fontStyle = 'italic';
    if (flags & 4) s.style.textDecoration = 'underline';
    if (flags & 8) s.style.opacity = '0.6';
    parent.appendChild(s);
  }
  return parent;
}

function lines(list, parent) {
  parent.replaceChildren(...list.map((l) => spans(l, el('div', 'tl'))));
}

function select(h) {
  const changed = (h && h.kind + h.id) !== (selected && selected.kind + selected.id);
  selected = h ? { kind: h.kind, id: h.id } : null;
  if (changed) { input.value = ''; setNote(''); tab = 0; }
  renderPanel(true);
  draw();
}

function setNote(msg, kind) { note.textContent = msg; note.className = 'note' + (kind ? ' ' + kind : ''); }

function current() {
  if (!selected || !model) return null;
  return selected.kind === 'agent' ? agents.get(selected.id) : shops.get(selected.id);
}

// Rebuilt on every update; skipped when nothing it shows changed, so text selection survives.
function renderPanel(force) {
  const item = current();
  if (!item) { panel.hidden = true; shown = ''; panelWalk = null; return; }
  const w = selected.kind === 'agent' ? walking.get(selected.id) : null;
  panelWalk = w ? w.m.id : null;
  const p = item.panel;
  const sig = JSON.stringify([selected, tab, p, panelWalk, model.demo]);
  if (!force && sig === shown && !panel.hidden) return;
  shown = sig;
  panel.hidden = false;

  $('p-title').textContent = ' ' + p.title + ' ';
  $('p-meta').replaceChildren(...spans(p.meta, el('span')).childNodes);

  const walk = $('p-walk');
  walk.hidden = !w;
  if (w) walk.replaceChildren(el('b', '', `Walking to ${agents.get(w.m.to)?.title || 'another agent'}`), el('div', '', `“${w.m.preview}”`));

  const banner = $('p-banner');
  banner.style.borderLeftColor = p.color;
  banner.style.background = p.banner_bg;
  lines(p.banner, banner);
  if (p.buttons.length) {
    const row = el('div', 'buttons');
    for (const b of p.buttons) {
      const btn = el('button', 'btn');
      btn.type = 'button';
      const key = el('span', 'key', ` ${b.key} `);
      key.style.background = b.bg;
      key.style.color = b.fg;
      btn.append(key, b.label);
      const can = !model.demo && ['approve', 'deny', 'reply', 'done'].includes(b.action);
      btn.disabled = !can;
      btn.title = can ? '' : model.demo ? 'Off in the demo town' : 'Do this in the hub';
      btn.addEventListener('click', () => perform(b.action));
      row.appendChild(btn);
    }
    banner.appendChild(row);
  }

  $('p-tabs').replaceChildren(...p.tabs.map(([label, n], i) => {
    const b = el('button', 'tab' + (i === tab ? ' on' : ''), label);
    b.type = 'button';
    if (n > 0) b.append(' ', el('span', 'n', String(n)));
    b.addEventListener('click', () => { tab = i; renderPanel(true); });
    return b;
  }));
  lines(p.bodies[tab] || [], $('p-body'));

  replyForm.hidden = !p.reply;
  input.placeholder = model.demo ? 'Replies are off in the demo town' : p.reply || '';
  input.disabled = model.demo;
}

function perform(action) {
  const a = selected && selected.kind === 'agent' ? agents.get(selected.id) : null;
  if (!a || model.demo || !a.panel.buttons.some((b) => b.action === action)) return;
  if (action === 'reply') input.focus();
  else if (['approve', 'deny', 'done'].includes(action)) act(action, a);
}

input.addEventListener('focus', () => replyForm.classList.add('on'));
input.addEventListener('blur', () => replyForm.classList.remove('on'));

// The hub's keys work in the panel too: y approve, d deny, r reply, m done, 1-4 / h l tabs, Esc closes.
document.addEventListener('keydown', (e) => {
  if (!selected || e.ctrlKey || e.metaKey || e.altKey) return;
  if (document.activeElement === input) {
    if (e.key === 'Escape') input.blur();
    return;
  }
  const keys = { y: 'approve', d: 'deny', r: 'reply', m: 'done' };
  if (keys[e.key]) { e.preventDefault(); perform(keys[e.key]); }
  else if (/^[1-4]$/.test(e.key)) { tab = +e.key - 1; renderPanel(true); }
  else if (e.key === 'l' || e.key === 'ArrowRight' || e.key === 'Tab') { e.preventDefault(); tab = (tab + 1) % 4; renderPanel(true); }
  else if (e.key === 'h' || e.key === 'ArrowLeft') { tab = (tab + 3) % 4; renderPanel(true); }
  else if (e.key === 'Escape') select(null);
});

async function act(op, a, textValue) {
  setNote({ reply: 'Sending…', approve: 'Approving…', deny: 'Denying…', done: 'Marking done…' }[op]);
  try {
    const res = await fetch('/api/act', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', 'X-Town-Token': TOKEN },
      body: JSON.stringify({ op, key: a.key, ts: a.ts, text: textValue }),
    });
    const data = await res.json();
    if (!res.ok) { setNote(data.error || 'Failed.', 'err'); return false; }
    setNote(data.ok, 'ok');
    return true;
  } catch (e) {
    setNote('The town server does not answer.', 'err');
    return false;
  }
}

replyForm.addEventListener('submit', async (e) => {
  e.preventDefault();
  const a = selected && agents.get(selected.id);
  const value = input.value.trim();
  if (!a || !value) return;
  if (await act('reply', a, value)) { input.value = ''; input.blur(); }
});

// ------------------------------------------------------------------ start

resize();
connect();
requestAnimationFrame(loop);
