// Claude Town: the hub's workspaces as houses and its agents as people, drawn in pixels.
// Every sprite here is original and defined in this file, either as a small pixel array or in code.
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
const GLOW = new Set(['lit', 'yellow', 'red', 'white']);

function mix(a, b, t) {
  const p = (h) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16));
  const [x, y] = [p(a), p(b)];
  return '#' + x.map((v, i) => Math.round(v + (y[i] - v) * t).toString(16).padStart(2, '0')).join('');
}

function palette(time) {
  if (time === 'day') return { ...BASE };
  const [to, t] = time === 'night' ? ['#1b2140', 0.55] : ['#6a4060', 0.22];
  const out = {};
  for (const [k, v] of Object.entries(BASE)) out[k] = GLOW.has(k) && time === 'night' && k !== 'white' ? v : mix(v, to, k === 'white' ? t * 0.6 : t);
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
let P = palette(timeOfDay());
let TIME = timeOfDay();
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
  M: 'x...xxx.xxx.x.xx...xx...x', N: 'xx.x.xx.xx.xx.x', O: '.x.x.xx.xx.x.x.', P: 'xx.x.xxx.x..x..',
  Q: '.x.x.xx.xxx..xx', R: 'xx.x.xxx.x.xx.x', S: '.xxx...x...xxx.', T: 'xxx.x..x..x..x.',
  U: 'x.xx.xx.xx.xxxx', V: 'x.xx.xx.xx.x.x.', W: 'x...xx...xx.x.xx.x.x.x.x.', X: 'x.xx.x.x.x.xx.x',
  Y: 'x.xx.x.x..x..x.', Z: 'xxx..x.x.x..xxx',
  0: 'xxxx.xx.xx.xxxx', 1: '.x.xx..x..x.xxx', 2: 'xx...x.x.x..xxx', 3: 'xx...x.x...xxx.',
  4: 'x.xx.xxxx..x..x', 5: 'xxxx..xx...xxx.', 6: '.xxx..xxxx.xxxx', 7: 'xxx..x.x..x..x.',
  8: 'xxxx.xxxxx.xxxx', 9: 'xxxx.xxxx..xxx.',
  '-': '......xxx......', _: '............xxx', '.': '.............x.', '/': '..x..x.x.x..x..',
  '!': '.x..x..x.....x.', '?': 'xx...x.x.....x.', ':': '....x.....x....', '+': '....x.xxx.x....',
  ' ': '...............', '#': 'x.xxxxx.xxxxx.x', '$': '.xxxx.xxx.xxxx.',
};

// People: h hair, s skin, c shirt, p trousers, k shoes.
const PERSON = {
  stand: ['..hhh..', '..sss..', '..sss..', '.ccccc.', 's.ccc.s', '..ccc..', '..p.p..', '..p.p..', '..k.k..'],
  wave: ['..hhh.s', '..sss.s', '..sss.c', '.ccccc.', 's.ccc..', '..ccc..', '..p.p..', '..p.p..', '..k.k..'],
  sit: ['..hhh..', '..sss..', '..sss..', '.ccccc.', 's.ccc.s', '.ppppp.', '.k...k.'],
  // Upper body behind a desk, seen through a window; two frames of typing.
  deskA: ['.hhh.', '.sss.', '.sss.', 'ccccc', 's...s'],
  deskB: ['.hhh.', '.sss.', '.sss.', 'ccccc', '.s.s.'],
  // Asleep with the head on the desk.
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
    const row = rows[r];
    for (let c = 0; c < row.length; c++) {
      const col = colors[row[c]];
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

const LOT_W = 78, LOT_H = 82, CAMP_W = 76, TOWN_X = CAMP_W + 12;
let model = null;
let houses = new Map();
let agents = new Map();
let rows = 2;
const INSIDE = new Set(['working', 'asleep']);
const ACTIVE = new Set(['working', 'permission', 'waiting', 'idle']);

function layout() {
  houses = new Map(model.houses.map((h) => [h.id, h]));
  agents = new Map(model.agents.map((a) => [a.key, a]));
  const cols = model.columns || 5;
  let maxLot = 0;
  for (const h of model.houses) {
    h.col = h.lot % cols;
    h.row = Math.floor(h.lot / cols);
    maxLot = Math.max(maxLot, h.lot);
    const mine = model.agents.filter((a) => a.house === h.id && !a.camp);
    h.inside = mine.filter((a) => INSIDE.has(a.state)).sort((a, b) => (a.state === 'asleep') - (b.state === 'asleep'));
    h.door = mine.filter((a) => a.state === 'permission');
    h.bench = mine.filter((a) => a.state === 'waiting' || a.state === 'idle');
    h.campers = model.agents.filter((a) => a.house === h.id && a.camp);
    const busy = h.terminals.some((t) => t.kind !== 'shell');
    // Agents of this repo that camp outside the hub still light it up: someone works on it.
    h.lit = mine.concat(h.campers).some((a) => ACTIVE.has(a.state)) || busy;
    const n = mine.length + h.terminals.length;
    h.windows = Math.min(4, Math.max(2, h.inside.length));
    h.wallW = Math.min(66, Math.max(36, h.windows * 14 + 12) + 4 * Math.min(3, Math.max(0, n - 2)));
    h.wallH = n >= 4 ? 24 : 20;
    h.x0 = TOWN_X + h.col * LOT_W;
    h.y0 = h.row * LOT_H;
    h.cx = h.x0 + LOT_W / 2;
    h.baseY = h.y0 + 48;
    h.left = h.cx - Math.floor(h.wallW / 2);
    h.right = h.left + h.wallW;
    h.top = h.baseY - h.wallH;
    h.roofH = Math.ceil((h.wallW + 4) / 4);
    h.style = hash(h.id);
  }
  rows = Math.max(2, Math.floor(maxLot / cols) + 1);
}

function townBounds() {
  const cols = model ? model.columns || 5 : 5;
  return { x: 0, y: -16, w: TOWN_X + cols * LOT_W, h: rows * LOT_H + 16 };
}

let centered = false;
function centerOnce() {
  if (centered || !model) return;
  centered = true;
  const b = townBounds();
  // The largest whole zoom that shows the whole town, but never so far out that pixels get tiny.
  const dpr = window.devicePixelRatio || 1;
  const dw = stage.clientWidth * dpr, dh = stage.clientHeight * dpr;
  cam.z = Math.max(2, Math.min(Math.floor(dw / (b.w + 16)), Math.floor(dh / (b.h + 16))));
  resize();
  cam.x = Math.round(b.x + b.w / 2 - view.w / 2);
  cam.y = Math.round(b.y + b.h / 2 - view.h / 2);
}

// ------------------------------------------------------------------ drawing

let hits = [];
const agentPos = new Map();
let hover = null;
let selected = null;
let lastFrame = 0;

function now() { return performance.now(); }

function drawGround() {
  const T = 8;
  const x0 = Math.floor(cam.x / T) * T, y0 = Math.floor(cam.y / T) * T;
  px(cam.x, cam.y, view.w + 1, view.h + 1, P.grass);
  for (let y = y0; y < cam.y + view.h; y += T) {
    for (let x = x0; x < cam.x + view.w; x += T) {
      const h = hash(x + ',' + y);
      if (h % 7 === 0) { px(x + (h >>> 3) % 6, y + (h >>> 6) % 6, 1, 2, P.grass2); px(x + 1 + (h >>> 3) % 6, y + 1 + (h >>> 6) % 6, 1, 1, P.grass2); }
      else if (h % 53 === 0) px(x + (h >>> 4) % 7, y + (h >>> 8) % 7, 1, 1, h % 2 ? P.white : P.yellow);
    }
  }
  const b = townBounds();
  // A road under every row of lots, and one along the camp.
  for (let r = 0; r < rows; r++) {
    const y = r * LOT_H + LOT_H - 10;
    px(CAMP_W, y, b.w - CAMP_W + 8, 8, P.road);
    px(CAMP_W, y + 7, b.w - CAMP_W + 8, 1, P.road2);
  }
  px(CAMP_W, -16, 8, rows * LOT_H + 16, P.road);
  px(CAMP_W + 7, -16, 1, rows * LOT_H + 16, P.road2);
}

function drawTree(x, y) { sprite(TREE, x, y, { l: P.leaf, L: P.leafD, t: P.woodD }); }

function drawEmptyLots() {
  const cols = model.columns || 5;
  const used = new Set(model.houses.map((h) => h.lot));
  for (let lot = 0; lot < rows * cols; lot++) {
    if (used.has(lot)) continue;
    const x0 = TOWN_X + (lot % cols) * LOT_W, y0 = Math.floor(lot / cols) * LOT_H;
    const h = hash('lot' + lot);
    for (let i = 0; i < 1 + (h % 2); i++) drawTree(x0 + 6 + ((h >>> (i * 5)) % 62), y0 + 10 + ((h >>> (i * 3 + 2)) % 46));
  }
}

function sign(label, cx, y, board, ink) {
  const w = textWidth(label) + 6;
  const x = Math.round(cx - w / 2);
  px(x + 2, y + 7, 1, 3, P.woodD);
  px(x + w - 3, y + 7, 1, 3, P.woodD);
  px(x, y, w, 8, P.woodD);
  px(x + 1, y + 1, w - 2, 6, board);
  text(label, x + 3, y + 2, ink);
}

function alarm(x, y, blink) {
  // A bubble twice the size of the others, with a thick '!'.
  const bx = x - 5, by = y - 15;
  px(bx, by, 11, 13, P.ink);
  px(bx + 1, by + 1, 9, 11, blink ? P.yellow : P.red);
  px(x - 1, by + 13, 3, 1, P.ink);
  px(x, by + 14, 1, 1, P.ink);
  const c = blink ? P.red : P.white;
  px(x - 1, by + 3, 3, 5, c);
  px(x - 1, by + 9, 3, 2, c);
}

function bubble(x, y, label, bg, fg) {
  // A speech bubble whose tail points down at (x, y).
  const w = 7, h = 9;
  const bx = x - 3, by = y - h - 2;
  px(bx, by, w, h, P.ink);
  px(bx + 1, by + 1, w - 2, h - 2, bg);
  px(x - 1, by + h, 3, 1, P.ink);
  px(x, by + h + 1, 1, 1, P.ink);
  text(label, bx + 2, by + 2, fg);
}

function shirt(key) {
  const opts = ['roofA', 'roofB', 'roofC', 'purple', 'teal', 'yellow'];
  return P[opts[hash(key) % opts.length]];
}
function hair(key) { return [P.woodD, P.ink, P.wood, P.yellow][(hash(key) >>> 4) % 4]; }

function personColors(a) { return { h: hair(a.key), s: P.skin, c: shirt(a.key), p: P.win, k: P.ink }; }

function hit(kind, id, x, y, w, h) { hits.push({ kind, id, x, y, w, h }); }

function zzz(x, y, t, seed) {
  for (let i = 0; i < 3; i++) {
    const phase = ((t / 2600 + i / 3 + (seed % 7) / 7) % 1);
    const zx = Math.round(x + phase * 6 + Math.sin(phase * 6) * 1.5);
    const zy = Math.round(y - phase * 14);
    if (phase < 0.92) sprite(Z, zx, zy, { x: P.white });
  }
}

function toolIcon(kind, x, y) {
  px(x - 1, y - 1, 9, 9, P.ink);
  px(x, y, 7, 7, P.white);
  const [m, s] = ICON_COLORS[kind] || ICON_COLORS.other;
  sprite(ICONS[kind] || ICONS.other, x, y, { x: P[m], y: P[s], w: P.wood });
}

function selectionMarker(x, y, w, h) {
  const c = P.yellow;
  for (const [cx, cy, dx, dy] of [[x, y, 1, 1], [x + w, y, -1, 1], [x, y + h, 1, -1], [x + w, y + h, -1, -1]]) {
    px(dx > 0 ? cx : cx - 3, cy, 4, 1, c);
    px(cx - (dx > 0 ? 0 : 1), dy > 0 ? cy : cy - 3, 1, 4, c);
  }
}

function roofRows(h) {
  // From the eaves up: [x, y, width] of each row of the stepped roof.
  const out = [];
  for (let r = 0; ; r++) {
    const w = h.wallW + 4 - r * 4;
    if (w <= 2) break;
    out.push([h.left - 2 + r * 2, h.top - 1 - r, w]);
  }
  return out;
}

function drawHouse(h, t) {
  const git = h.git;
  const wall = git ? (h.style % 3 === 0 ? P.wallB : P.wallA) : P.wallB;
  const roof = git ? [P.roofA, P.roofB, P.roofC][(h.style >>> 3) % 3] : P.wood;
  const roofs = roofRows(h);
  const apex = h.top - roofs.length;

  // Path from the door to the road.
  px(h.cx - 3, h.baseY, 7, h.y0 + LOT_H - 10 - h.baseY, P.road);

  // Walls with a shaded right side and a sill line.
  px(h.left, h.top, h.wallW, h.wallH, wall);
  px(h.right - 3, h.top, 3, h.wallH, P.shade);
  px(h.left, h.baseY - 1, h.wallW, 1, P.shade);

  // Chimneys, one per terminal that is not an editor; smoke when something runs in it.
  const chimneys = h.terminals.filter((x) => x.kind !== 'editor').slice(0, 3);
  chimneys.forEach((term, i) => {
    const x = h.right - 9 - i * 7;
    const r = Math.floor((h.right + 2 - x) / 2);
    const surface = h.top - 1 - r;
    px(x, surface - 5, 4, 7, P.woodD);
    px(x - 1, surface - 6, 6, 2, P.shade);
    if (term.kind === 'busy') {
      for (let j = 0; j < 3; j++) {
        const ph = ((t / 3000 + j / 3 + i * 0.17) % 1);
        const size = ph < 0.5 ? 2 : 1;
        px(x + 1 + Math.round(ph * 5 + Math.sin(ph * 5)), surface - 8 - Math.round(ph * 14), size, size, P.smoke);
      }
    }
  });

  // Stepped roof, darker eaves and every third row.
  roofs.forEach(([x, y, w], r) => px(x, y, w, 1, r === 0 ? P.woodD : r % 3 === 2 ? mix(roof, P.ink, 0.18) : roof));

  // Door.
  px(h.cx - 3, h.baseY - 11, 7, 11, P.woodD);
  px(h.cx - 2, h.baseY - 10, 5, 10, P.wood);
  px(h.cx + 1, h.baseY - 5, 1, 1, P.yellow);

  // Windows: inside agents work at desks or sleep on them.
  const slots = windowSlots(h);
  slots.forEach(([x, y], i) => {
    const a = h.inside[i];
    const lit = h.lit && (!a || a.state !== 'asleep');
    px(x - 1, y - 1, 12, 10, P.woodD);
    px(x, y, 10, 8, lit ? P.lit : P.win);
    if (a) {
      const frame = a.state === 'working' ? (Math.floor(t / 380 + (hash(a.key) % 5)) % 2 ? PERSON.deskA : PERSON.deskB) : PERSON.sleep;
      sprite(frame, x + 2, y + 2, personColors(a));
      px(x, y + 7, 10, 1, P.wood);
      agentPos.set(a.key, { x: x + 5, y: y + 4 });
      hit('agent', a.key, x - 1, y - 1, 12, 10);
      if (a.state === 'asleep') zzz(x + 6, y - 3, t, hash(a.key));
      if (selected && selected.id === a.key) selectionMarker(x - 3, y - 3, 15, 13);
    } else {
      px(x + 5, y, 1, 8, P.woodD);
      px(x, y + 4, 10, 1, P.woodD);
    }
  });

  // Tool icons above the roof, one per working agent.
  const working = h.inside.filter((a) => a.state === 'working');
  working.forEach((a, i) => {
    const x = Math.round(h.cx - (working.length * 11) / 2 + i * 11 + 2);
    const bob = Math.floor(t / 900 + i) % 2;
    toolIcon(a.tool_kind || 'other', x, apex - 11 - bob);
  });

  // Uncommitted changes: a few bricks, or scaffolding when there are many.
  if (git && git.dirty > 0 && git.dirty <= 3) {
    for (let i = 0; i < git.dirty; i++) {
      const bx = h.left - 7 + (i % 2) * 4, by = h.baseY - 3 - Math.floor(i / 2) * 2;
      px(bx, by, 4, 2, P.roofA);
      px(bx, by + 2, 4, 1, P.woodD);
    }
  } else if (git && git.dirty > 3) {
    for (const x of [h.left - 2, h.left + Math.floor(h.wallW / 2) - 6, h.right + 1]) px(x, h.top - 2, 1, h.wallH + 2, P.wood);
    for (const y of [h.top + 5, h.baseY - 4]) {
      px(h.left - 3, y, h.wallW + 5, 1, P.wood);
      px(h.left - 3, y + 1, h.wallW + 5, 1, P.woodD);
    }
  }

  // Mailbox: flag up for outgoing commits, a letter in it for incoming ones.
  if (git) {
    const mx = h.right + 4, my = h.baseY - 9;
    px(mx + 2, my + 4, 1, 9, P.woodD);
    px(mx, my, 6, 5, P.roofB);
    px(mx, my + 4, 6, 1, P.ink);
    if (git.ahead > 0) {
      px(mx + 6, my - 5, 1, 7, P.ink);
      px(mx + 7, my - 5, 3, 2, P.red);
    } else {
      px(mx + 6, my + 1, 3, 1, P.red);
    }
    if (git.behind > 0) {
      px(mx + 1, my - 2, 4, 3, P.white);
      px(mx + 2, my - 1, 2, 1, P.red);
    }
  }

  // Lamp posts, one per open editor.
  const lamps = h.terminals.filter((x) => x.kind === 'editor').slice(0, 4);
  lamps.forEach((_, i) => {
    const x = h.x0 + [4, LOT_W - 6, 12, LOT_W - 14][i];
    const y = h.baseY - 10;
    if (TIME !== 'day') {
      for (let dy = -3; dy <= 3; dy++) for (let dx = -5; dx <= 5; dx++) if ((dx + dy) % 2 === 0 && dx * dx + dy * dy * 3 < 26) px(x + dx, y + 18 + dy, 1, 1, P.lit);
    }
    px(x, y + 2, 1, 16, P.ink);
    px(x - 1, y, 3, 3, P.ink);
    px(x, y + 1, 1, 1, TIME === 'day' ? P.white : P.lit);
  });

  // Bench by the wall for agents that wait for your reply or are done.
  const bx = h.left + 2, by = h.baseY + 4;
  px(bx, by, 16, 2, P.wood);
  px(bx + 1, by + 2, 1, 3, P.woodD);
  px(bx + 14, by + 2, 1, 3, P.woodD);
  h.bench.slice(0, 2).forEach((a, i) => {
    const x = bx + 1 + i * 7, y = by - 5;
    sprite(PERSON.sit, x, y, personColors(a));
    if (a.state === 'waiting') bubble(x + 3, y - 1, '?', P.white, P.ink);
    else bubble(x + 3, y - 1, '.', P.white, P.ink);
    agentPos.set(a.key, { x: x + 3, y });
    hit('agent', a.key, x, y - 11, 7, 18);
    if (selected && selected.id === a.key) selectionMarker(x - 2, y - 13, 11, 22);
  });

  // Agents that need permission step outside, wave and call. The only thing in town that blinks.
  const blink = Math.floor(t / 450) % 2 === 0;
  h.door.forEach((a, i) => {
    const x = h.cx - 3 + (i % 2 ? 9 : 0) + Math.floor(i / 2) * 18, y = h.baseY + 1;
    sprite(blink ? PERSON.wave : PERSON.stand, x, y, personColors(a));
    alarm(x + 3, y - 1, blink);
    agentPos.set(a.key, { x: x + 3, y });
    hit('agent', a.key, x - 2, y - 16, 11, 25);
    if (selected && selected.id === a.key) selectionMarker(x - 4, y - 18, 15, 29);
  });
  if (h.door.length) alarm(h.cx, apex - (working.length ? 13 : 1), blink);

  sign(fit(h.name, 18), h.cx, h.baseY + 12, P.wallA, P.ink);

  if (hover && hover.kind === 'house' && hover.id === h.id && git && git.branch) {
    const label = fit(git.branch, 20);
    // Above the tool icons and the alarm, whichever are there.
    const stack = apex - (working.length ? 12 : 0) - (h.door.length ? 16 : 0);
    const w = textWidth(label) + 8, x = Math.round(h.cx - w / 2), y = stack - 11;
    px(x, y, w, 9, P.purple);
    px(x + 1, y + 1, w - 2, 7, mix(P.purple, P.white, 0.15));
    px(x - 1, y + 2, 1, 5, P.purple);
    px(x + w, y + 2, 1, 5, P.purple);
    text(label, x + 4, y + 2, P.white);
  }
  if (selected && selected.id === h.id) selectionMarker(h.left - 6, apex - 3, h.wallW + 12, h.baseY - apex + 26);
  hit('house', h.id, h.left - 4, apex, h.wallW + 12, h.baseY - apex + 22);
}

function windowSlots(h) {
  const n = h.windows;
  const left = Math.ceil(n / 2), right = n - left;
  const y = h.top + Math.floor((h.wallH - 8) / 2) - 1;
  const slots = [];
  const place = (count, from, to) => {
    const span = to - from;
    for (let i = 0; i < count; i++) slots.push([Math.round(from + (span * (i + 0.5)) / count - 5), y]);
  };
  place(left, h.left + 2, h.cx - 4);
  place(right, h.cx + 5, h.right - 3);
  return slots;
}

function campAgents() { return model.agents.filter((a) => a.camp); }

function drawCamp(t) {
  const list = campAgents();
  if (!list.length) return;
  const fx = Math.round(CAMP_W / 2), fy = 10;
  // Trampled ground, a fire and a sign.
  for (let dy = -10; dy <= 12; dy++) for (let dx = -14; dx <= 14; dx++) if (dx * dx + dy * dy * 1.8 < 190 && hash(dx + ':' + dy) % 3) px(fx + dx, fy + dy, 1, 1, P.road);
  px(fx - 4, fy + 2, 9, 2, P.woodD);
  const flick = Math.floor(t / 300) % 2;
  px(fx - 2, fy - 2, 5, 4, P.red);
  px(fx - 1, fy - 4 - flick, 3, 3, P.yellow);
  sign('CAMP', fx, fy + 14, P.wallA, P.ink);
  list.forEach((a, i) => {
    const x = 4 + (i % 2) * 36, y = 56 + Math.floor(i / 2) * 40;
    drawTent(a, x, y, t);
  });
}

function drawTent(a, x, y, t) {
  const color = [P.roofA, P.teal, P.purple, P.roofC][hash(a.key) % 4];
  for (let r = 0; r < 12; r++) {
    px(x + 11 - r, y + r, r * 2 + 2, 1, r % 4 === 3 ? mix(color, P.ink, 0.2) : color);
  }
  const inside = a.state === 'working' ? P.lit : P.ink;
  for (let r = 4; r < 12; r++) px(x + 12 - Math.floor((r - 4) / 2), y + r, (r - 4) + 1, 1, inside);
  px(x + 11, y - 3, 1, 4, P.woodD);
  const label = fit(a.workspace || a.path.split('/').pop() || 'agent', 7);
  text(label, x + 12 - Math.floor(textWidth(label) / 2), y + 15, P.ink);
  const fx = x + 25, fy = y + 6;
  let ay = y + 4;
  if (a.state === 'working') {
    toolIcon(a.tool_kind || 'other', x + 8, y - 14 - (Math.floor(t / 900) % 2));
    ay = y + 6;
  } else if (a.state === 'asleep') {
    zzz(x + 14, y - 2, t, hash(a.key));
  } else if (a.state === 'permission') {
    const blink = Math.floor(t / 450) % 2 === 0;
    sprite(blink ? PERSON.wave : PERSON.stand, fx, fy - 2, personColors(a));
    alarm(fx + 3, fy - 3, blink);
  } else {
    px(fx - 1, fy + 6, 9, 2, P.wood);
    sprite(PERSON.sit, fx, fy, personColors(a));
    bubble(fx + 3, fy - 1, a.state === 'waiting' ? '?' : '.', P.white, P.ink);
  }
  agentPos.set(a.key, { x: x + 14, y: ay });
  hit('agent', a.key, x, y - 4, 34, 22);
  if (selected && selected.id === a.key) selectionMarker(x - 2, y - 6, 38, 28);
}

function draw() {
  if (!model) return;
  const t = now();
  lastFrame = t;
  hits = [];
  ctx.clearRect(0, 0, view.w, view.h);
  drawGround();
  drawEmptyLots();
  drawCamp(t);
  [...houses.values()].sort((a, b) => a.y0 - b.y0 || a.x0 - b.x0).forEach((h) => drawHouse(h, t));
}

// Calm frame rate: the town only needs a few frames a second.
function loop() {
  if (now() - lastFrame > 110) draw();
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

function connect() {
  const es = new EventSource('/events?t=' + encodeURIComponent(TOKEN));
  es.onmessage = (e) => {
    conn.hidden = true;
    model = JSON.parse(e.data);
    document.getElementById('demo').hidden = !model.demo;
    layout();
    centerOnce();
    updateCounter();
    if (selected && !(selected.kind === 'agent' ? agents.has(selected.id) : houses.has(selected.id))) select(null);
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
  // Agents first: they stand in front of houses.
  const inside = (h) => w.x >= h.x && w.x < h.x + h.w && w.y >= h.y && w.y < h.y + h.h;
  return hits.find((h) => h.kind === 'agent' && inside(h)) || hits.find((h) => h.kind === 'house' && inside(h)) || null;
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
  const key = h && h.kind + h.id;
  if (key !== (hover && hover.kind + hover.id)) { hover = h; draw(); }
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

function line(parent, textContent, cls) {
  const el = document.createElement(cls === 'b' ? 'b' : 'div');
  if (cls && cls !== 'b') el.className = cls;
  el.textContent = textContent;
  parent.appendChild(el);
  return el;
}

function gitSummary(g) {
  const out = [];
  out.push(g.dirty ? `${g.dirty} changed file${g.dirty === 1 ? '' : 's'}, +${g.added} −${g.removed}` : 'No uncommitted changes');
  if (g.ahead || g.behind) out.push(`${g.ahead} ahead, ${g.behind} behind`);
  return out;
}

function showTip(h, e) {
  if (!h || drag) { tip.hidden = true; return; }
  tip.replaceChildren();
  if (h.kind === 'house') {
    const house = houses.get(h.id);
    line(tip, house.name, 'b');
    line(tip, house.path, 'dim');
    if (house.git) {
      line(tip, '⎇ ' + (house.git.branch || '-'));
      gitSummary(house.git).forEach((s) => line(tip, s));
    } else line(tip, 'Not a git repo', 'dim');
  } else {
    const a = agents.get(h.id);
    line(tip, a.title, 'b');
    line(tip, a.headline + (a.detail ? ' · ' + a.detail : ''), 'dim');
  }
  tip.hidden = false;
  const r = stage.getBoundingClientRect();
  const x = Math.min(e.clientX - r.left + 14, r.width - tip.offsetWidth - 8);
  const y = Math.min(e.clientY - r.top + 14, r.height - tip.offsetHeight - 8);
  tip.style.left = Math.max(8, x) + 'px';
  tip.style.top = Math.max(8, y) + 'px';
}

// ------------------------------------------------------------------ side panel

const panel = document.getElementById('panel');
const body = document.getElementById('panel-body');
const title = document.getElementById('panel-title');
const replyForm = document.getElementById('reply');
const replyText = document.getElementById('reply-text');
const note = document.getElementById('note');
let shown = '';

document.getElementById('close').addEventListener('click', () => select(null));
document.addEventListener('keydown', (e) => { if (e.key === 'Escape' && document.activeElement !== replyText) select(null); });

function select(h) {
  const changed = (h && h.kind + h.id) !== (selected && selected.kind + selected.id);
  selected = h ? { kind: h.kind, id: h.id } : null;
  if (changed) { replyText.value = ''; setNote(''); }
  renderPanel(changed);
  draw();
}

function setNote(msg, kind) { note.textContent = msg; note.className = kind || ''; }

function section(heading) {
  const s = document.createElement('section');
  if (heading) { const h3 = document.createElement('h3'); h3.textContent = heading; s.appendChild(h3); }
  body.appendChild(s);
  return s;
}

function ago(ts) {
  const s = Math.max(0, Math.floor(Date.now() / 1000) - ts);
  if (s >= 86400) return Math.floor(s / 86400) + 'd';
  if (s >= 3600) return Math.floor(s / 3600) + 'h';
  if (s >= 60) return Math.floor(s / 60) + 'm';
  return s + 's';
}

function filesSection(g) {
  if (!g || !g.files.length) return;
  const s = section('Changed files');
  const ul = document.createElement('ul');
  ul.className = 'files';
  for (const [st, path] of g.files) {
    const li = document.createElement('li');
    const span = document.createElement('span');
    span.className = 'st';
    span.textContent = st;
    li.append(span, path);
    ul.appendChild(li);
  }
  s.appendChild(ul);
  if (g.more_files) line(s, `and ${g.more_files} more`, 'meta');
}

const STATE_LABEL = { permission: 'Needs permission', waiting: 'Waiting for you', working: 'Working', idle: 'Done', asleep: 'Asleep' };

// The panel is rebuilt on every update; skip that when nothing it shows changed, so text selection survives.
function renderPanel(force) {
  if (!selected || !model) {
    panel.hidden = true;
    shown = '';
    return;
  }
  const item = selected.kind === 'agent' ? agents.get(selected.id) : houses.get(selected.id);
  if (!item) { panel.hidden = true; return; }
  const sig = JSON.stringify([selected, item, selected.kind === 'agent' && item.house ? houses.get(item.house)?.git : null, model.demo]);
  if (!force && sig === shown && !panel.hidden) return;
  shown = sig;
  panel.hidden = false;
  body.replaceChildren();
  if (selected.kind === 'agent') agentPanel(item); else housePanel(item);
}

function housePanel(h) {
  title.replaceChildren(h.name);
  line(title, h.path, 'sub');
  replyForm.hidden = true;
  const g = h.git;
  const s = section('Repository');
  if (g) {
    line(s, '⎇ ' + (g.branch || '-'));
    gitSummary(g).forEach((x) => line(s, x, 'meta'));
  } else line(s, 'Not a git repo', 'meta');
  if (!h.hub) line(s, 'No Claude Hub workspace; its agents camp at the edge of town.', 'meta');
  const people = model.agents.filter((a) => a.house === h.id);
  if (people.length) {
    const sa = section('Agents');
    const ul = document.createElement('ul');
    ul.className = 'links';
    for (const a of people) {
      const li = document.createElement('li');
      const b = document.createElement('button');
      b.type = 'button';
      b.textContent = `${a.title} — ${STATE_LABEL[a.state]}${a.camp ? ' (camp)' : ''}`;
      b.addEventListener('click', () => select({ kind: 'agent', id: a.key }));
      li.appendChild(b);
      ul.appendChild(li);
    }
    sa.appendChild(ul);
  }
  if (h.terminals.length) {
    const st = section('Terminals');
    for (const t of h.terminals) line(st, `${t.name} · ${t.command}`, 'meta');
  }
  filesSection(g);
}

function agentPanel(a) {
  title.replaceChildren(a.title);
  line(title, (a.workspace ? a.workspace + ' · ' : '') + a.path, 'sub');
  const head = section();
  const chip = document.createElement('span');
  chip.className = 'chip ' + a.state;
  chip.textContent = STATE_LABEL[a.state];
  head.appendChild(chip);
  if (a.ts && (a.state === 'permission' || a.state === 'waiting')) head.append(' for ' + ago(a.ts));
  line(head, a.headline, 'b').style.display = 'block';
  if (a.detail && a.state !== 'permission') line(head, a.detail, 'meta');
  if (a.camp) line(head, a.background ? 'Runs in Claude Code\'s background daemon.' : 'Runs outside Claude Hub.', 'meta');

  if (a.state === 'permission') {
    const s = section('It wants to run');
    const box = document.createElement('div');
    box.className = 'call';
    if (a.call) {
      line(box, a.call.name, 'name');
      const pre = document.createElement('pre');
      pre.textContent = a.call.input;
      box.appendChild(pre);
    } else line(box, a.message || 'The tool call is not in the transcript yet.', 'meta');
    s.appendChild(box);
    const row = document.createElement('div');
    row.className = 'row';
    row.style.marginTop = '10px';
    const ok = button('Approve', 'approve', () => act('approve', a));
    const no = button('Deny', 'deny', () => act('deny', a));
    ok.disabled = no.disabled = !a.can_reply || model.demo;
    row.append(ok, no);
    s.appendChild(row);
    if (a.message) line(s, a.message, 'meta');
  } else if (a.message) {
    line(section('It says'), a.message, 'text');
  }

  if (a.prompt) line(section('Your last request'), a.prompt, 'text');
  if (a.reply) line(section('Its last reply'), a.reply, 'text');
  filesSection(a.house ? houses.get(a.house)?.git : null);

  const meta = [a.model, a.turns ? `${a.turns} turn${a.turns === 1 ? '' : 's'}` : null, a.cost ? '$' + a.cost.toFixed(2) : null, a.mode ? a.mode + ' mode' : null, a.session].filter(Boolean);
  if (meta.length) line(section(), meta.join(' · '), 'meta');

  replyForm.hidden = !a.can_reply && !model.demo;
  replyForm.querySelector('button').disabled = !a.can_reply || model.demo;
  replyText.disabled = !a.can_reply || model.demo;
  if (model.demo) replyText.placeholder = 'Replies are off in the demo town';
  else replyText.placeholder = a.state === 'permission' ? 'Reply instead of approving (y / n also work)…' : 'Reply to the agent…';
}

function button(label, cls, fn) {
  const b = document.createElement('button');
  b.type = 'button';
  b.className = 'act ' + cls;
  b.textContent = label;
  b.addEventListener('click', fn);
  return b;
}

async function act(op, a, textValue) {
  setNote(op === 'reply' ? 'Sending…' : op === 'approve' ? 'Approving…' : 'Denying…');
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
  const value = replyText.value.trim();
  if (!a || !value) return;
  if (await act('reply', a, value)) replyText.value = '';
});
replyText.addEventListener('keydown', (e) => {
  if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); replyForm.requestSubmit(); }
});

// ------------------------------------------------------------------ start

resize();
connect();
requestAnimationFrame(loop);
