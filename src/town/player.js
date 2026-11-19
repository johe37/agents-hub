// You in Claude Town: the XP bar, level-ups and loot, your character (a paper doll you dress in
// the loot), the quest log, and you walking the streets to the agents that wait for you.
// Loaded before town.js and shares its globals (model, shops, px, mix, hash, el, route…), which it
// only touches inside functions. All sprites are original and drawn in code.
'use strict';

// ------------------------------------------------------------------ palettes and names

const RARITY = {
  poor: { color: '#9d9d9d', label: 'Poor' },
  common: { color: '#f0f0f0', label: 'Common' },
  uncommon: { color: '#3ccf4e', label: 'Uncommon' },
  rare: { color: '#3d8bff', label: 'Rare' },
  epic: { color: '#b155ff', label: 'Epic' },
  legendary: { color: '#ff9a1f', label: 'Legendary' },
};
const SKINS = ['#f6d5b8', '#eec09a', '#d9a07a', '#b97d55', '#8d5a3b', '#5e3b26'];
const HAIR_COLORS = ['#2b2b3a', '#5a3a22', '#a0703f', '#e2c06a', '#c4553f', '#d8d8e0', '#4f7cac', '#8a6bb0'];
const OUTFITS = ['#4f7cac', '#5e9a6b', '#c4553f', '#8a6bb0', '#3fa3a0', '#d9a441', '#6b6f80', '#2f3a4f'];
const HAIR_STYLES = ['Short', 'Spiky', 'Long', 'Bun', 'Buzz'];
const DEFAULT_LOOK = { name: 'You', skin: 1, hair_style: 0, hair_color: 1, outfit: 0 };
const SLOT_LABEL = { head: 'Head', chest: 'Chest', hands: 'Hands', feet: 'Feet', main_hand: 'Main hand', off_hand: 'Off-hand', back: 'Back', trinket: 'Trinket' };
const KIND_LABEL = { weapon: 'Weapon', helmet: 'Helmet', headphones: 'Headphones', chest: 'Chest', gloves: 'Gloves', boots: 'Boots', cape: 'Cape', shield: 'Shield', mug: 'Mug', keyboard: 'Keyboard', trinket: 'Trinket' };
const STATS = ['focus', 'speed', 'insight', 'stamina'];
const STAT_LABEL = { focus: 'Focus', speed: 'Speed', insight: 'Insight', stamina: 'Stamina' };
// Particle colours of the legendary auras: main, second, highlight.
const AURA = {
  ember: ['#ff7a1a', '#ffd23f', '#e04848'], frost: ['#bfefff', '#7fd4ff', '#ffffff'], sparkle: ['#fff3a0', '#ffd23f', '#ffffff'],
  storm: ['#c9b6ff', '#ffffff', '#7f6bff'], leaf: ['#7fd46b', '#4f8a4a', '#c8f08a'], shadow: ['#8a6bb0', '#3b2b5a', '#c8a8f0'],
};
const INK = '#1b1b28';

// The base material of each kind, before the rarity tints it.
const MATERIAL = {
  weapon: '#b3bcc6', helmet: '#a9b2bc', headphones: '#3a4150', chest: '#8b6b4a', gloves: '#8a5a3a', boots: '#6e4a2a',
  cape: '#7a4f8f', shield: '#8f6a3f', mug: '#ece6da', keyboard: '#3a3f4b', trinket: '#d9b24a',
};
const TINT = { poor: 0, common: 0.1, uncommon: 0.25, rare: 0.35, epic: 0.42, legendary: 0.45 };

/// Colours of an item: Poor and Common plain, Uncommon and Rare with trim, Epic with a detail colour,
/// Legendary in the colours of its aura.
function material(it) {
  const r = RARITY[it.rarity].color;
  let base = MATERIAL[it.kind] || '#999999';
  if (it.rarity === 'poor') base = mix(base, '#8a8478', 0.6);
  const cloth = it.kind === 'chest' || it.kind === 'cape';
  base = mix(base, r, Math.min(0.8, TINT[it.rarity] * (cloth ? 1.5 : 1)));
  const aura = AURA[it.aura] || null;
  if (aura) base = mix(base, aura[0], 0.3);
  return {
    base, dark: mix(base, INK, 0.38), light: mix(base, '#ffffff', 0.4),
    trim: ['uncommon', 'rare', 'epic', 'legendary'].includes(it.rarity) ? r : null,
    detail: it.rarity === 'epic' ? '#ffd23f' : aura ? aura[1] : null,
    aura,
  };
}

// ------------------------------------------------------------------ drawing helpers

function R(g, x, y, w, h, c) {
  if (!c || w <= 0 || h <= 0) return;
  g.fillStyle = c;
  g.fillRect(x, y, w, h);
}

function rgbOf(hex) { return [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16)); }

/// A one-pixel dark outline around everything drawn so far: what makes pixel art read at a glance.
function outline(g, w, h) {
  const img = g.getImageData(0, 0, w, h), d = img.data, src = new Uint8ClampedArray(d);
  const [r, gg, b] = rgbOf(INK);
  const solid = (x, y) => x >= 0 && y >= 0 && x < w && y < h && src[(y * w + x) * 4 + 3] > 0;
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const i = (y * w + x) * 4;
      if (src[i + 3] > 0) continue;
      if (solid(x - 1, y) || solid(x + 1, y) || solid(x, y - 1) || solid(x, y + 1)) {
        d[i] = r; d[i + 1] = gg; d[i + 2] = b; d[i + 3] = 255;
      }
    }
  }
  g.putImageData(img, 0, 0);
}

/// The town's pixel font on any canvas.
function textOn(g, s, x, y, color, scale = 1) {
  for (const ch of s) {
    const gl = glyph(ch), w = gl.length / 5;
    for (let j = 0; j < gl.length; j++) if (gl[j] === 'x') R(g, x + (j % w) * scale, y + Math.floor(j / w) * scale, scale, scale, color);
    x += (w + 1) * scale;
  }
}

function newCanvas(w, h) {
  const c = document.createElement('canvas');
  c.width = w; c.height = h;
  const g = c.getContext('2d');
  g.imageSmoothingEnabled = false;
  return [c, g];
}

// ------------------------------------------------------------------ the body: poses and anchors

// The body is 32x48; the canvas leaves room around it for weapons, hats and jumps.
const DOLL_W = 48, DOLL_H = 66, OX = 8, OY = 14;

/// The body in one frame. Every worn sprite is drawn from the anchors of the pose, so it follows
/// the body through every frame of every animation.
function pose(anim, ms) {
  const p = { by: 0, bob: 0, hl: 0, hr: 0, fl: 0, fr: 0, blink: false, sway: 0 };
  if (anim === 'walk') {
    const f = Math.floor(ms / 150) % 4;
    if (f === 0) { p.fl = 2; p.hl = 1; p.hr = -1; p.sway = 1; }
    else if (f === 2) { p.fr = 2; p.hl = -1; p.hr = 1; p.sway = -1; }
    else p.bob = 1;
  } else if (anim === 'jump') {
    if (ms < 140 || (ms >= 620 && ms < 780)) { p.bob = 2; p.hl = p.hr = 1; }
    else if (ms < 620) {
      p.by = -Math.round(Math.sin(((ms - 140) / 480) * Math.PI) * 9);
      p.hl = p.hr = -15; p.fl = p.fr = 1; p.sway = 2;
    }
  } else {
    p.bob = Math.floor(ms / 900) % 2; // breathing
    p.blink = ms % 3700 < 140;
  }
  return p;
}

function anchors(p) {
  const y = p.by, b = p.bob;
  return {
    head: { x: 9, y: 5 + y + b }, // 14x13
    torso: { x: 10, y: 19 + y + b }, // 12x12
    sh: 19 + y + b,
    hip: 31 + y + b,
    handL: { x: 7, y: 29 + y + b + p.hl }, // 3x3; the main hand, on the viewer's left
    handR: { x: 22, y: 29 + y + b + p.hr },
    footL: { x: 10, y: 41 + y - p.fl }, // 5x3
    footR: { x: 17, y: 41 + y - p.fr },
    sway: p.sway,
  };
}

function lookColors(look) {
  const skin = SKINS[look.skin] || SKINS[1];
  const shirt = OUTFITS[look.outfit] || OUTFITS[0];
  return { skin, skinD: mix(skin, '#7a4a3a', 0.3), shirt, shirtD: mix(shirt, INK, 0.3), pants: mix(shirt, '#2b2f45', 0.7), shoe: '#3a2a22' };
}

function drawBody(g, A, look, p) {
  const c = lookColors(look);
  [[11, A.footL], [17, A.footR]].forEach(([x, f]) => {
    R(g, x, A.hip, 4, f.y - A.hip, c.pants);
    R(g, f.x, f.y, 5, 3, c.shoe);
  });
  const T = A.torso;
  R(g, T.x, T.y, 12, 12, c.shirt);
  R(g, T.x + 10, T.y, 2, 12, c.shirtD);
  R(g, T.x, T.y + 11, 12, 1, c.shirtD);
  [[7, A.handL], [22, A.handR]].forEach(([x, h]) => {
    const top = Math.min(A.sh, h.y), bot = Math.max(A.sh, h.y);
    R(g, x, top, 3, bot - top, c.skin);
    R(g, x, A.sh, 3, 3, c.shirt);
    R(g, x, h.y, 3, 3, c.skin);
  });
  const H = A.head;
  R(g, 14, H.y + 13, 4, 1, c.skinD); // neck
  R(g, H.x + 1, H.y, 12, 13, c.skin);
  R(g, H.x, H.y + 1, 14, 11, c.skin);
  R(g, H.x - 1, H.y + 6, 1, 2, c.skinD);
  R(g, H.x + 14, H.y + 6, 1, 2, c.skinD);
  if (p.blink) {
    R(g, H.x + 3, H.y + 7, 2, 1, INK);
    R(g, H.x + 9, H.y + 7, 2, 1, INK);
  } else {
    R(g, H.x + 3, H.y + 6, 2, 2, INK);
    R(g, H.x + 9, H.y + 6, 2, 2, INK);
    R(g, H.x + 3, H.y + 6, 1, 1, '#ffffff');
    R(g, H.x + 9, H.y + 6, 1, 1, '#ffffff');
  }
  const blush = mix(c.skin, '#f07a7a', 0.45);
  R(g, H.x + 1, H.y + 9, 2, 1, blush);
  R(g, H.x + 11, H.y + 9, 2, 1, blush);
  R(g, H.x + 6, H.y + 10, 2, 1, c.skinD);
}

function drawHair(g, A, look) {
  const c = HAIR_COLORS[look.hair_color] || HAIR_COLORS[0], d = mix(c, INK, 0.35), l = mix(c, '#ffffff', 0.3), H = A.head;
  const style = look.hair_style || 0;
  if (style === 4) {
    R(g, H.x + 1, H.y - 1, 12, 3, d);
    R(g, H.x + 3, H.y - 1, 5, 1, c);
    return;
  }
  if (style === 2) { // long: falls behind the shoulders, drawn first so the top covers it
    R(g, H.x - 1, H.y + 5, 3, 12, c);
    R(g, H.x + 12, H.y + 5, 3, 12, c);
    R(g, H.x - 1, H.y + 16, 3, 1, d);
    R(g, H.x + 12, H.y + 16, 3, 1, d);
  }
  if (style === 3) { R(g, H.x + 4, H.y - 7, 6, 5, c); R(g, H.x + 5, H.y - 6, 2, 1, l); }
  R(g, H.x, H.y - 2, 14, 5, c);
  R(g, H.x + 2, H.y - 3, 10, 1, c);
  R(g, H.x - 1, H.y, 2, 6, c);
  R(g, H.x + 13, H.y, 2, 6, c);
  R(g, H.x + 2, H.y + 3, 3, 1, c);
  R(g, H.x + 8, H.y + 3, 2, 1, c);
  R(g, H.x + 3, H.y - 2, 4, 1, l);
  if (style === 1) for (let i = 0; i < 5; i++) R(g, H.x + 1 + i * 3, H.y - 5 + (i % 2), 2, 3, c);
}

// ------------------------------------------------------------------ worn sprites
// One base shape per item kind (and per base name, it.base), decorated by rarity. Legendaries add a
// unique shape of their own on top. Each takes the pose's anchors.

const WOOD = '#8a5a32', WOOD_D = '#5e3b22';

function legendShape(it) { return hash(it.name) % 3; }

function wornCape(g, A, it, m, t) {
  const T = A.torso, len = [20, 25, 11][it.base] || 20, L = it.rarity === 'legendary';
  for (let i = 0; i < 18; i++) {
    let h = len + (i % 4 === 0 ? 0 : 1);
    if (L) h += [0, 2, 1, 3][(i + Math.floor(t / 220)) % 4]; // a ragged edge that flickers
    const off = Math.round(A.sway * (i / 17));
    R(g, T.x - 3 + i, T.y, 1, h - 2, i < 2 || i > 15 ? m.dark : m.base);
    R(g, T.x - 3 + i + off, T.y + h - 2, 1, 2, m.trim || m.dark);
  }
  if (it.base === 1) R(g, A.head.x + 1, A.head.y + 2, 12, 6, m.dark); // the hood, hanging behind the head
  if (it.base === 2) R(g, T.x - 3, T.y - 1, 18, 3, m.light); // the mantle's collar
}

function wornChest(g, A, it, m, t, c) {
  const T = A.torso, L = it.rarity === 'legendary';
  if (it.base === 3) { // vest: the shirt shows in the middle
    R(g, T.x, T.y, 4, 12, m.base);
    R(g, T.x + 8, T.y, 4, 12, m.base);
    R(g, T.x + 10, T.y, 2, 12, m.dark);
    R(g, T.x + 3, T.y + 4, 1, 1, m.trim || m.light);
    R(g, T.x + 3, T.y + 8, 1, 1, m.trim || m.light);
  } else {
    R(g, T.x, T.y, 12, 12, m.base);
    R(g, T.x + 10, T.y, 2, 12, m.dark);
    const sleeve = [5, 9, 9][it.base] || 5;
    [[7, A.handL], [22, A.handR]].forEach(([x, h]) => {
      if (h.y >= A.sh) R(g, x, A.sh, 3, Math.min(sleeve, h.y - A.sh), m.base);
      else R(g, x, Math.max(h.y + 3, A.sh - sleeve + 3), 3, A.sh + 3 - Math.max(h.y + 3, A.sh - sleeve + 3), m.base);
    });
    if (it.base === 0) R(g, T.x, T.y + 8, 12, 1, m.dark); // belt
    if (it.base === 1) for (let y = T.y + 1; y < T.y + 11; y++) for (let x = T.x; x < T.x + 10; x++) if ((x + y) % 2 === 0) R(g, x, y, 1, 1, m.light); // chain links
    if (it.base === 2) { // hoodie: pocket and strings
      R(g, T.x + 3, T.y + 7, 6, 3, m.dark);
      R(g, T.x + 4, T.y + 1, 1, 3, m.light);
      R(g, T.x + 7, T.y + 1, 1, 3, m.light);
    }
  }
  if (m.trim) { R(g, T.x, T.y, 12, 1, m.trim); R(g, T.x, T.y + 11, 12, 1, m.trim); }
  if (m.detail && !L) R(g, T.x + 5, T.y + 4, 2, 2, m.detail);
  if (L) { // pauldrons and a glowing rune
    R(g, T.x - 4, T.y - 2, 6, 4, m.light);
    R(g, T.x + 10, T.y - 2, 6, 4, m.light);
    R(g, T.x - 4, T.y + 1, 6, 1, m.aura[0]);
    R(g, T.x + 10, T.y + 1, 6, 1, m.aura[0]);
    R(g, T.x + 5, T.y + 4, 2, 3, Math.floor(t / 400) % 2 ? m.aura[2] : m.aura[1]);
  }
}

function wornTrinket(g, A, it, m, t) {
  const T = A.torso, gem = m.detail || m.trim || '#e04848', big = it.rarity === 'legendary' ? 1 : 0;
  if (it.base === 0) { // charm on the belt
    R(g, T.x + 1, T.y + 8, 1, 2, WOOD_D);
    R(g, T.x, T.y + 10, 3 + big, 3 + big, m.base);
    R(g, T.x + 1, T.y + 11, 1, 1, gem);
  } else if (it.base === 1) { // amulet
    for (let i = 0; i < 3; i++) { R(g, T.x + 3 + i, T.y + i, 1, 1, m.light); R(g, T.x + 8 - i, T.y + i, 1, 1, m.light); }
    R(g, T.x + 5 - big, T.y + 3, 2 + big * 2, 2 + big, gem);
  } else if (it.base === 2) { // ring
    R(g, A.handR.x, A.handR.y + 1, 3, 1, m.base);
    R(g, A.handR.x + 1, A.handR.y + 1, 1, 1, gem);
  } else { // a rubber duck on the shoulder
    const x = T.x + 11, y = T.y - 3;
    R(g, x, y, 5, 3, '#ffd23f');
    R(g, x + 3, y - 3, 3, 3, '#ffd23f');
    R(g, x + 6, y - 2, 1, 1, '#ff8a1f');
    R(g, x + 4, y - 2, 1, 1, INK);
    if (m.trim) R(g, x + 2, y - 1, 3, 1, m.trim); // a ribbon
  }
}

function wornGloves(g, A, it, m, t) {
  for (const h of [A.handL, A.handR]) {
    if (it.base === 1) { R(g, h.x - 1, h.y - 2, 5, 5, m.base); R(g, h.x - 1, h.y - 2, 5, 1, m.light); }
    else if (it.base === 2) { R(g, h.x, h.y - 1, 3, 4, m.base); R(g, h.x - 1, h.y, 5, 2, m.base); }
    else R(g, h.x, h.y - 1, 3, 4, m.base);
    if (m.trim) R(g, h.x - (it.base === 1 ? 1 : 0), h.y - (it.base === 1 ? 2 : 1), it.base === 1 ? 5 : 3, 1, m.trim);
    if (m.detail) R(g, h.x + 1, h.y + 1, 1, 1, m.detail);
    if (it.rarity === 'legendary') { R(g, h.x - 1, h.y + 1, 1, 1, m.aura[1]); R(g, h.x + 3, h.y + 1, 1, 1, m.aura[1]); }
  }
}

function wornBoots(g, A, it, m, t) {
  [[11, A.footL, -1], [17, A.footR, 1]].forEach(([legX, f, side]) => {
    if (it.base === 2) { // sandals: straps over the foot
      R(g, f.x, f.y + 2, 5, 1, m.dark);
      R(g, f.x, f.y, 5, 1, m.base);
      R(g, legX + 1, f.y - 2, 2, 1, m.base);
    } else {
      const shaft = it.base === 1 ? 5 : 3;
      R(g, legX, f.y - shaft, 4, shaft, m.base);
      R(g, f.x, f.y, 5, 3, m.base);
      R(g, f.x, f.y + 2, 5, 1, m.dark);
      if (m.trim) R(g, legX, f.y - shaft, 4, 1, m.trim);
      if (it.base === 1) R(g, legX, f.y - 2, 4, 1, m.dark);
    }
    if (m.detail) R(g, legX + 1, f.y - 2, 2, 1, m.detail);
    if (it.rarity === 'legendary') { // little wings at the ankles
      const wx = side < 0 ? f.x - 3 : f.x + 5;
      R(g, wx, f.y - 2, 3, 1, '#fbf7ef');
      R(g, side < 0 ? wx : wx + 2, f.y - 3, 1, 1, '#fbf7ef');
    }
  });
}

function wornHead(g, A, it, m, t) {
  const H = A.head, L = it.rarity === 'legendary';
  if (it.kind === 'headphones') {
    if (it.base === 2) { // earmuffs
      R(g, H.x + 1, H.y - 3, 12, 1, m.base);
      R(g, H.x - 3, H.y + 2, 4, 7, m.light);
      R(g, H.x + 13, H.y + 2, 4, 7, m.light);
    } else {
      R(g, H.x, H.y - 3, 14, 1, m.dark);
      R(g, H.x - 1, H.y - 2, 1, 5, m.dark);
      R(g, H.x + 14, H.y - 2, 1, 5, m.dark);
      R(g, H.x - 3, H.y + 3, 3, 6, m.base);
      R(g, H.x + 14, H.y + 3, 3, 6, m.base);
      R(g, H.x - 2, H.y + 4, 1, 2, m.light);
      R(g, H.x + 15, H.y + 4, 1, 2, m.light);
      if (it.base === 1) { // headset: a microphone boom
        R(g, H.x - 1, H.y + 8, 1, 2, m.dark);
        R(g, H.x, H.y + 10, 3, 1, m.dark);
        R(g, H.x + 3, H.y + 10, 2, 2, m.light);
      }
    }
    if (m.trim) { R(g, H.x - 3, H.y + 8, 3, 1, m.trim); R(g, H.x + 14, H.y + 8, 3, 1, m.trim); }
    if (m.detail) { R(g, H.x - 2, H.y + 6, 1, 1, m.detail); R(g, H.x + 15, H.y + 6, 1, 1, m.detail); }
    if (L) { R(g, H.x + 6, H.y - 7, 1, 4, m.dark); R(g, H.x + 5, H.y - 9, 3, 2, Math.floor(t / 300) % 2 ? m.aura[0] : m.aura[2]); }
    return;
  }
  if (it.base === 0) { // helm
    R(g, H.x - 1, H.y - 3, 16, 8, m.base);
    R(g, H.x, H.y - 4, 14, 1, m.base);
    R(g, H.x - 1, H.y + 5, 3, 6, m.base);
    R(g, H.x + 12, H.y + 5, 3, 6, m.base);
    R(g, H.x + 6, H.y + 5, 2, 4, m.dark);
    R(g, H.x + 1, H.y - 3, 5, 1, m.light);
    if (m.trim) R(g, H.x - 1, H.y + 4, 16, 1, m.trim);
  } else if (it.base === 1) { // cap
    R(g, H.x, H.y - 3, 14, 6, m.base);
    R(g, H.x + 1, H.y - 4, 12, 1, m.base);
    R(g, H.x + 7, H.y + 3, 9, 1, m.dark);
    R(g, H.x + 6, H.y - 5, 2, 1, m.light);
    if (m.trim) R(g, H.x, H.y + 2, 14, 1, m.trim);
  } else if (it.base === 2) { // hood
    R(g, H.x - 2, H.y - 3, 18, 5, m.base);
    R(g, H.x - 2, H.y + 2, 3, 13, m.base);
    R(g, H.x + 13, H.y + 2, 3, 13, m.base);
    R(g, H.x + 1, H.y + 2, 12, 1, m.dark);
    if (m.trim) R(g, H.x - 2, H.y - 3, 18, 1, m.trim);
  } else { // circlet: over the hair
    R(g, H.x, H.y + 2, 14, 1, mix(m.base, '#d9b24a', 0.5));
    R(g, H.x + 6, H.y + 1, 2, 2, m.detail || m.trim || '#e04848');
  }
  if (m.detail && it.base !== 3 && !L) R(g, H.x + 6, H.y - 2, 2, 2, m.detail);
  if (L) {
    const shape = legendShape(it);
    if (shape === 0) { // horns
      R(g, H.x - 2, H.y - 6, 2, 4, m.light); R(g, H.x - 3, H.y - 8, 1, 2, m.light);
      R(g, H.x + 14, H.y - 6, 2, 4, m.light); R(g, H.x + 16, H.y - 8, 1, 2, m.light);
    } else if (shape === 1) { // wings
      R(g, H.x - 5, H.y - 1, 4, 1, '#fbf7ef'); R(g, H.x - 6, H.y - 3, 4, 2, '#fbf7ef');
      R(g, H.x + 15, H.y - 1, 4, 1, '#fbf7ef'); R(g, H.x + 16, H.y - 3, 4, 2, '#fbf7ef');
    } else { // a crown of points in the aura's colour
      for (let i = 0; i < 4; i++) R(g, H.x + 1 + i * 4, H.y - 6, 2, 2, m.aura[1]);
    }
  }
}

function wornWeapon(g, A, it, m, t) {
  const h = A.handL, cx = h.x + 1, L = it.rarity === 'legendary';
  switch (it.base) {
    case 1: // axe
      R(g, cx, h.y - 12, 1, 17, WOOD);
      R(g, cx - 4, h.y - 12, 4, 5, m.base);
      R(g, cx - 5, h.y - 11, 1, 3, m.light);
      R(g, cx + 1, h.y - 11, 2, 2, m.dark);
      if (m.trim) R(g, cx - 1, h.y - 6, 3, 1, m.trim);
      if (L) { R(g, cx - 6, h.y - 13, 2, 7, m.aura[0]); R(g, cx - 7, h.y - 11, 1, 3, m.aura[1]); }
      break;
    case 2: // mace
      R(g, cx, h.y - 9, 1, 13, WOOD);
      R(g, cx - 2, h.y - 14, 5, 5, m.base);
      R(g, cx, h.y - 16, 1, 2, m.light);
      R(g, cx - 3, h.y - 12, 1, 1, m.light);
      R(g, cx + 3, h.y - 12, 1, 1, m.light);
      if (m.trim) R(g, cx - 1, h.y - 9, 3, 1, m.trim);
      if (L) R(g, cx - 1, h.y - 13, 3, 3, m.aura[Math.floor(t / 250) % 3]);
      break;
    case 3: // spear
      R(g, cx, h.y - 20, 1, 26, WOOD);
      R(g, cx - 1, h.y - 23, 3, 3, m.light);
      R(g, cx, h.y - 25, 1, 2, m.light);
      if (m.trim) R(g, cx - 1, h.y - 20, 3, 1, m.trim);
      if (L) { R(g, cx - 2, h.y - 22, 1, 2, m.aura[0]); R(g, cx + 2, h.y - 22, 1, 2, m.aura[0]); }
      break;
    case 4: // staff
      R(g, cx, h.y - 17, 1, 24, WOOD);
      R(g, cx - 1, h.y - 21, 3, 4, m.detail || m.trim || m.light);
      R(g, cx - 1, h.y - 21, 1, 1, '#ffffff');
      if (L) { R(g, cx - 2, h.y - 22, 1, 6, m.aura[0]); R(g, cx + 2, h.y - 22, 1, 6, m.aura[0]); }
      break;
    default: // sword
      R(g, cx, h.y + 3, 1, 1, m.dark);
      R(g, cx - 2, h.y - 1, 5, 1, m.trim || m.dark);
      if (m.detail) R(g, cx, h.y - 1, 1, 1, m.detail);
      if (L) { // a wide blade with a core of the aura's colour
        R(g, cx - 1, h.y - 15, 4, 14, m.light);
        R(g, cx, h.y - 15, 2, 14, m.aura[Math.floor(t / 200) % 2]);
        R(g, cx, h.y - 17, 2, 2, m.light);
      } else {
        R(g, cx, h.y - 13, 2, 12, m.light);
        R(g, cx, h.y - 13, 1, 12, m.base);
        R(g, cx, h.y - 14, 1, 1, m.light);
      }
  }
}

function wornOffHand(g, A, it, m, t, c) {
  const h = A.handR, L = it.rarity === 'legendary';
  if (it.kind === 'shield') {
    if (it.base === 3) { // mousepad, held like a small shield, with a mouse on it
      R(g, h.x - 2, h.y - 6, 9, 9, m.base);
      R(g, h.x - 2, h.y + 2, 9, 1, m.dark);
      if (m.trim) { R(g, h.x - 2, h.y - 6, 9, 1, m.trim); R(g, h.x - 2, h.y - 6, 1, 9, m.trim); }
      R(g, h.x + 2, h.y - 4, 3, 4, '#d8d8e0');
      R(g, h.x + 3, h.y - 4, 1, 1, '#9d9d9d');
      R(g, h.x + 3, h.y - 6, 1, 2, '#6e7681');
      if (m.detail) R(g, h.x - 1, h.y, 2, 1, m.detail);
      if (L) R(g, h.x - 1, h.y - 5, 2, 2, m.aura[Math.floor(t / 300) % 3]);
      return;
    }
    if (it.base === 1) { // buckler
      R(g, h.x, h.y - 4, 5, 7, m.base);
      R(g, h.x - 1, h.y - 3, 7, 5, m.base);
      R(g, h.x + 1, h.y - 2, 3, 3, m.light);
      if (m.trim) R(g, h.x, h.y - 4, 5, 1, m.trim);
    } else {
      const top = it.base === 2 ? h.y - 8 : h.y - 6, body = it.base === 2 ? 8 : 9;
      R(g, h.x - 1, top, 8, body, m.base);
      for (let i = 0; i < 3; i++) R(g, h.x + i, top + body + i, 6 - i * 2, 1, m.base);
      R(g, h.x - 1, top, 8, 1, m.trim || m.dark);
      R(g, h.x + 2, top + 3, 2, 2, m.light);
      if (m.detail) { R(g, h.x + 2, top + 1, 2, body, m.detail); R(g, h.x, top + 3, 6, 2, m.detail); }
    }
    if (L) { R(g, h.x + 1, h.y - 4, 4, 4, m.aura[0]); R(g, h.x + 2, h.y - 3, 2, 2, m.aura[2]); }
    return;
  }
  if (it.kind === 'mug') {
    if (it.base === 1) { // tankard
      R(g, h.x, h.y - 5, 6, 7, m.base);
      R(g, h.x - 1, h.y - 6, 8, 1, m.dark);
      R(g, h.x + 6, h.y - 4, 2, 1, m.dark);
      R(g, h.x + 7, h.y - 4, 1, 4, m.dark);
      R(g, h.x, h.y - 3, 6, 1, m.trim || m.dark);
    } else if (it.base === 2) { // thermos
      R(g, h.x + 1, h.y - 8, 4, 10, m.base);
      R(g, h.x + 1, h.y - 9, 4, 2, m.dark);
      if (m.trim) R(g, h.x + 1, h.y - 4, 4, 1, m.trim);
    } else { // mug with coffee
      R(g, h.x + 1, h.y - 3, 5, 5, m.base);
      R(g, h.x + 1, h.y - 3, 5, 1, '#5a3a22');
      R(g, h.x + 6, h.y - 2, 1, 3, m.dark);
      R(g, h.x + 1, h.y + 1, 5, 1, m.dark);
      if (m.trim) R(g, h.x + 1, h.y - 1, 5, 1, m.trim);
    }
    if (m.detail) R(g, h.x + 3, h.y - 1, 1, 1, m.detail);
    // Steam, always: it is a mug.
    const ph = (t / 900) % 1;
    if (ph < 0.8) R(g, h.x + 3 + Math.round(Math.sin(t / 300)), h.y - (it.base === 2 ? 11 : 5) - Math.floor(ph * 4), 1, 1, L ? m.aura[0] : '#e8e8f0');
    R(g, h.x, h.y - 1, 2, 2, c.skin); // the thumb holding it
    return;
  }
  // keyboard, under the arm
  if (it.base === 1) { // keypad
    R(g, h.x - 1, h.y - 2, 6, 5, m.base);
    for (let i = 0; i < 3; i++) for (let j = 0; j < 2; j++) R(g, h.x + i * 2, h.y - 1 + j * 2, 1, 1, L ? m.aura[(i + j) % 3] : m.light);
  } else if (it.base === 2) { // split
    R(g, h.x - 6, h.y - 1, 5, 4, m.base);
    R(g, h.x + 1, h.y - 1, 5, 4, m.base);
    for (let i = 0; i < 2; i++) { R(g, h.x - 5 + i * 2, h.y, 1, 1, m.light); R(g, h.x + 2 + i * 2, h.y, 1, 1, m.light); }
  } else {
    R(g, h.x - 5, h.y - 1, 12, 4, m.base);
    for (let i = 0; i < 5; i++) {
      R(g, h.x - 4 + i * 2, h.y, 1, 1, L ? m.aura[(i + Math.floor(t / 300)) % 3] : m.light);
      R(g, h.x - 3 + i * 2, h.y + 1, 1, 1, m.light);
    }
  }
  if (m.trim) R(g, h.x - (it.base === 1 ? 1 : 5), h.y + 2 + (it.base === 1 ? 0 : 0), it.base === 1 ? 6 : 12, 1, m.trim);
  if (m.detail) R(g, h.x + 1, h.y - 1, 1, 1, m.detail);
}

const WORN = {
  cape: wornCape, chest: wornChest, trinket: wornTrinket, gloves: wornGloves, boots: wornBoots,
  helmet: wornHead, headphones: wornHead, weapon: wornWeapon, shield: wornOffHand, mug: wornOffHand, keyboard: wornOffHand,
};

// Where on the body an item's aura shows.
function auraSpot(A, it) {
  const T = A.torso, H = A.head;
  switch (it.slot) {
    case 'head': return { x: H.x - 2, y: H.y - 5, w: 18, h: 9 };
    case 'hands': return { x: A.handL.x - 2, y: Math.min(A.handL.y, A.handR.y) - 2, w: 22, h: 7 };
    case 'feet': return { x: A.footL.x - 2, y: A.footL.y - 3, w: 16, h: 6 };
    case 'main_hand': return { x: A.handL.x - 3, y: A.handL.y - 17, w: 8, h: 19 };
    case 'off_hand': return { x: A.handR.x - 3, y: A.handR.y - 6, w: 10, h: 10 };
    case 'back': return { x: T.x - 4, y: T.y + 12, w: 20, h: 12 };
    default: return { x: T.x - 2, y: T.y, w: 16, h: 12 };
  }
}

function drawAura(g, A, it, t) {
  const c = AURA[it.aura] || AURA.sparkle, s = auraSpot(A, it), seed = (hash(it.id) % 997) / 997;
  for (let i = 0; i < 7; i++) {
    const ph = (t / 1700 + i / 7 + seed) % 1;
    const rx = s.x + (hash(it.id + i) % s.w), ry = s.y + (hash(it.id + 'y' + i) % s.h);
    const wob = Math.round(Math.sin(ph * 6.3 + i) * 1.5);
    switch (it.aura) {
      case 'ember': if (ph < 0.85) R(g, rx + wob, s.y + s.h - Math.round(ph * (s.h + 7)), 1, 1, c[i % 3]); break;
      case 'frost': R(g, rx + wob, s.y - 3 + Math.round(ph * (s.h + 6)), 1, 1, c[i % 3]); break;
      case 'leaf': R(g, rx + wob * 2, s.y - 2 + Math.round(ph * (s.h + 5)), 1 + (i % 2), 1, c[i % 3]); break;
      case 'shadow': if (ph < 0.9) R(g, rx + wob, s.y + s.h - Math.round(ph * s.h * 1.3), 1, 2, c[i % 3]); break;
      case 'storm':
        if (Math.floor(t / 90 + i * 3) % 7 === 0) for (let k = 0; k < 3; k++) R(g, rx + (k % 2), ry + k, 1, 1, c[k % 3]);
        break;
      default: // sparkle
        if (ph < 0.25) {
          R(g, rx, ry, 1, 1, c[2]);
          if (ph < 0.12) { R(g, rx - 1, ry, 1, 1, c[0]); R(g, rx + 1, ry, 1, 1, c[0]); R(g, rx, ry - 1, 1, 1, c[0]); R(g, rx, ry + 1, 1, 1, c[0]); }
        }
    }
  }
}

/// Draw the character on a DOLL_W x DOLL_H canvas: the layers back to front, as the hub ordered them.
function renderDoll(g, look, layers, byId, anim, ms, t) {
  g.clearRect(0, 0, DOLL_W, DOLL_H);
  const p = pose(anim, ms), A = anchors(p), c = lookColors(look);
  g.save();
  g.translate(OX, OY);
  for (const L of layers) {
    if (L.part === 'body') drawBody(g, A, look, p);
    else if (L.part === 'hair') drawHair(g, A, look);
    else if (L.item) {
      const it = byId.get(L.item);
      if (it && WORN[it.kind]) WORN[it.kind](g, A, it, material(it), t, c);
    }
  }
  g.restore();
  outline(g, DOLL_W, DOLL_H);
  const legend = layers.filter((L) => L.item && byId.get(L.item)?.rarity === 'legendary').map((L) => byId.get(L.item));
  g.save();
  g.translate(OX, OY);
  if (layers.some((L) => L.part === 'effects')) for (const it of legend) drawAura(g, A, it, t);
  // Behind everything: a soft glow around legendaries, and the shadow on the ground.
  g.globalCompositeOperation = 'destination-over';
  for (const it of legend) {
    const s = auraSpot(A, it), a = 0.14 + 0.08 * Math.sin(t / 500);
    g.globalAlpha = a;
    R(g, s.x - 1, s.y, s.w + 2, s.h, (AURA[it.aura] || AURA.sparkle)[0]);
    R(g, s.x, s.y - 1, s.w, s.h + 2, (AURA[it.aura] || AURA.sparkle)[0]);
    g.globalAlpha = 1;
  }
  const lift = -p.by;
  R(g, 9 + Math.floor(lift / 3), 45, 14 - Math.floor(lift / 3) * 2, 2, 'rgba(0,0,0,0.28)');
  g.globalCompositeOperation = 'source-over';
  g.restore();
}

// ------------------------------------------------------------------ inventory icons (16x16)

function renderIcon(g, it) {
  g.clearRect(0, 0, 16, 16);
  const m = material(it), b = it.base || 0, L = it.rarity === 'legendary';
  const diag = (x, y, n, w, c) => { for (let i = 0; i < n; i++) R(g, x + i, y - i, w, w, c); };
  switch (it.kind) {
    case 'weapon':
      if (b === 0) { diag(4, 10, 8, 2, m.light); diag(5, 10, 7, 1, m.base); R(g, 2, 10, 5, 2, m.trim || m.dark); R(g, 4, 8, 2, 5, m.trim || m.dark); diag(1, 14, 3, 1, WOOD_D); }
      else if (b === 1) { diag(2, 14, 11, 1, WOOD); R(g, 7, 2, 5, 6, m.base); R(g, 6, 3, 1, 4, m.light); }
      else if (b === 2) { diag(2, 14, 8, 1, WOOD); R(g, 9, 2, 5, 5, m.base); R(g, 11, 1, 1, 1, m.light); R(g, 14, 4, 1, 1, m.light); R(g, 8, 4, 1, 1, m.light); }
      else if (b === 3) { diag(1, 15, 11, 1, WOOD); R(g, 11, 2, 3, 3, m.light); R(g, 13, 1, 2, 2, m.light); }
      else { diag(2, 15, 10, 1, WOOD); R(g, 10, 1, 4, 4, m.detail || m.trim || m.light); R(g, 10, 1, 1, 1, '#ffffff'); }
      if (L) R(g, 8, 5, 2, 2, m.aura[0]);
      break;
    case 'helmet':
      if (b === 0) { R(g, 3, 3, 10, 8, m.base); R(g, 2, 5, 12, 6, m.base); R(g, 7, 9, 2, 4, m.dark); R(g, 4, 4, 3, 1, m.light); if (m.trim) R(g, 2, 10, 12, 1, m.trim); }
      else if (b === 1) { R(g, 3, 5, 10, 6, m.base); R(g, 4, 4, 8, 1, m.base); R(g, 9, 10, 6, 2, m.dark); R(g, 7, 3, 2, 1, m.light); if (m.trim) R(g, 3, 9, 10, 1, m.trim); }
      else if (b === 2) { R(g, 3, 3, 10, 11, m.base); R(g, 5, 6, 6, 7, m.dark); if (m.trim) R(g, 3, 3, 10, 1, m.trim); }
      else { R(g, 2, 7, 12, 2, mix(m.base, '#d9b24a', 0.5)); R(g, 7, 5, 2, 3, m.detail || m.trim || '#e04848'); }
      if (m.detail && b !== 3) R(g, 7, 6, 2, 2, m.detail);
      if (L) { R(g, 1, 1, 2, 4, m.light); R(g, 13, 1, 2, 4, m.light); }
      break;
    case 'headphones':
      R(g, 4, 2, 8, 1, m.dark); R(g, 3, 3, 1, 4, m.dark); R(g, 12, 3, 1, 4, m.dark);
      R(g, 1, 7, 4, 6, b === 2 ? m.light : m.base); R(g, 11, 7, 4, 6, b === 2 ? m.light : m.base);
      if (b === 1) { R(g, 4, 12, 4, 1, m.dark); R(g, 8, 11, 2, 2, m.light); }
      if (m.trim) { R(g, 1, 12, 4, 1, m.trim); R(g, 11, 12, 4, 1, m.trim); }
      if (L) R(g, 7, 0, 2, 2, m.aura[0]);
      break;
    case 'chest':
      R(g, 4, 3, 8, 11, m.base); R(g, 1, 3, 3, b === 0 ? 4 : 7, m.base); R(g, 12, 3, 3, b === 0 ? 4 : 7, m.base);
      R(g, 6, 3, 4, 2, m.dark);
      if (b === 1) for (let y = 5; y < 13; y++) for (let x = 4; x < 12; x++) if ((x + y) % 2 === 0) R(g, x, y, 1, 1, m.light);
      if (b === 2) R(g, 5, 9, 6, 3, m.dark);
      if (b === 3) R(g, 7, 5, 2, 9, m.dark);
      if (m.trim) R(g, 4, 13, 8, 1, m.trim);
      if (m.detail) R(g, 7, 7, 2, 2, m.detail);
      if (L) { R(g, 0, 2, 5, 3, m.light); R(g, 11, 2, 5, 3, m.light); }
      break;
    case 'gloves':
      R(g, 4, 5, 7, 8, m.base); R(g, 11, 7, 2, 3, m.base); R(g, 4, 3, 2, 3, m.base); R(g, 6, 2, 2, 3, m.base); R(g, 8, 3, 2, 3, m.base);
      R(g, 4, 12, 7, 2, m.trim || m.dark);
      if (b === 1) R(g, 4, 6, 7, 1, m.light);
      if (m.detail) R(g, 7, 8, 1, 1, m.detail);
      if (L) R(g, 6, 9, 2, 1, m.aura[0]);
      break;
    case 'boots':
      R(g, 5, 2, 5, 9, m.base); R(g, 5, 10, 9, 4, m.base); R(g, 5, 13, 9, 1, m.dark);
      if (b === 2) { R(g, 5, 4, 5, 1, m.dark); R(g, 5, 7, 5, 1, m.dark); }
      if (m.trim) R(g, 5, 2, 5, 1, m.trim);
      if (L) { R(g, 1, 5, 4, 1, '#fbf7ef'); R(g, 2, 4, 3, 1, '#fbf7ef'); }
      break;
    case 'cape':
      R(g, 4, 2, 8, 2, m.light); R(g, 3, 4, 10, b === 2 ? 6 : 10, m.base); R(g, 2, b === 2 ? 9 : 13, 12, 1, m.trim || m.dark);
      if (b === 1) R(g, 5, 1, 6, 2, m.dark);
      if (L) for (let x = 2; x < 14; x += 2) R(g, x, 14, 1, 1, m.aura[0]);
      break;
    case 'shield':
      if (b === 3) { // mousepad
        R(g, 1, 4, 14, 9, m.base); R(g, 2, 3, 12, 11, m.base); R(g, 1, 12, 14, 1, m.dark);
        if (m.trim) { R(g, 2, 3, 12, 1, m.trim); R(g, 2, 13, 12, 1, m.trim); }
        R(g, 9, 6, 3, 5, '#d8d8e0'); R(g, 10, 6, 1, 1, '#9d9d9d'); R(g, 10, 4, 1, 2, '#6e7681');
        if (m.detail) R(g, 3, 10, 3, 1, m.detail);
        if (L) R(g, 4, 5, 2, 2, m.aura[0]);
        break;
      }
      if (b === 1) { R(g, 4, 3, 8, 10, m.base); R(g, 3, 4, 10, 8, m.base); R(g, 6, 6, 4, 4, m.light); }
      else { R(g, 3, 2, 10, 9, m.base); R(g, 4, 11, 8, 1, m.base); R(g, 5, 12, 6, 1, m.base); R(g, 6, 13, 4, 1, m.base); R(g, 7, 14, 2, 1, m.base); R(g, 7, 5, 2, 2, m.light); }
      if (m.trim) R(g, 3, 2, 10, 1, m.trim);
      if (m.detail) { R(g, 7, 3, 2, 9, m.detail); R(g, 4, 6, 8, 2, m.detail); }
      if (L) R(g, 6, 5, 4, 4, m.aura[0]);
      break;
    case 'mug':
      if (b === 2) { R(g, 5, 2, 6, 13, m.base); R(g, 5, 1, 6, 2, m.dark); }
      else { R(g, 3, 5, 8, 9, m.base); R(g, 3, 5, 8, 1, '#5a3a22'); R(g, 11, 7, 2, 1, m.dark); R(g, 12, 7, 1, 5, m.dark); R(g, 11, 11, 2, 1, m.dark); }
      if (b === 1) R(g, 2, 4, 10, 1, m.dark);
      if (m.trim) R(g, b === 2 ? 5 : 3, 9, b === 2 ? 6 : 8, 1, m.trim);
      R(g, 6, 2, 1, 2, L ? m.aura[0] : '#e8e8f0'); R(g, 8, 1, 1, 2, L ? m.aura[1] : '#e8e8f0');
      break;
    case 'keyboard':
      if (b === 1) { R(g, 3, 3, 10, 10, m.base); for (let i = 0; i < 3; i++) for (let j = 0; j < 3; j++) R(g, 4 + i * 3, 4 + j * 3, 2, 2, L ? m.aura[(i + j) % 3] : m.light); }
      else if (b === 2) { R(g, 0, 5, 7, 6, m.base); R(g, 9, 5, 7, 6, m.base); for (let i = 0; i < 3; i++) { R(g, 1 + i * 2, 7, 1, 1, m.light); R(g, 10 + i * 2, 7, 1, 1, m.light); } }
      else { R(g, 1, 5, 14, 6, m.base); for (let i = 0; i < 6; i++) { R(g, 2 + i * 2, 6, 1, 1, L ? m.aura[i % 3] : m.light); R(g, 3 + i * 2, 8, 1, 1, m.light); } R(g, 4, 9, 8, 1, m.light); }
      if (m.trim) R(g, 1, 11, 14, 1, m.trim);
      break;
    default: { // trinket
      const gem = m.detail || m.trim || '#e04848';
      if (b === 3) { R(g, 3, 8, 9, 5, '#ffd23f'); R(g, 8, 3, 5, 5, '#ffd23f'); R(g, 13, 5, 2, 2, '#ff8a1f'); R(g, 10, 4, 1, 1, INK); if (m.trim) R(g, 8, 8, 4, 1, m.trim); }
      else if (b === 2) { R(g, 4, 5, 8, 8, m.base); g.clearRect(6, 7, 4, 4); R(g, 6, 3, 4, 3, gem); }
      else if (b === 1) { diag(3, 7, 5, 1, m.light); R(g, 8, 3, 1, 1, m.light); for (let i = 0; i < 5; i++) R(g, 8 + i, 3 + i, 1, 1, m.light); R(g, 6, 8, 5, 5, gem); R(g, 7, 9, 1, 1, '#ffffff'); }
      else { R(g, 7, 1, 1, 5, WOOD_D); R(g, 4, 6, 7, 7, m.base); R(g, 6, 8, 3, 3, gem); }
      if (L) R(g, 12, 2, 2, 2, m.aura[2]);
    }
  }
  outline(g, 16, 16);
}

function iconCanvas(it, size = 40) {
  const [c, g] = newCanvas(16, 16);
  renderIcon(g, it);
  c.style.width = c.style.height = size + 'px';
  c.className = 'icon';
  return c;
}

// ------------------------------------------------------------------ you in the town

let player = null; // model.player
let itemsById = new Map();
const me = { shop: null, pos: null, walk: null, until: 0, jumpAt: -1e9, dir: 1 };
// A stroll, slower than the agents' errands (16), and a good while in each shop.
const ME_SPEED = 9;
const STAY_MS = [45000, 90000];
/// After the agent you came for is answered, stay this long before strolling on.
const LINGER_MS = 20000;
const stay = () => STAY_MS[0] + Math.random() * (STAY_MS[1] - STAY_MS[0]);

function look() { return (player && player.appearance) || DEFAULT_LOOK; }
function wornIn(slot) { const id = player && player.equipped[slot]; return id ? itemsById.get(id) : null; }

/// Where you stand at a shop: beside its door, left of where agents wait and messengers stop.
function standAt(s) { return { x: s.doorX - 18, y: s.baseY + 6 }; }

function needyShop() {
  const order = { permission: 0, waiting: 1 };
  const list = model.agents.filter((a) => a.state in order && !walking.has(a.key)).sort((a, b) => order[a.state] - order[b.state] || a.ts - b.ts);
  for (const a of list) if (shops.has(a.shop)) return shops.get(a.shop);
  return null;
}

function nextTarget() {
  const needy = needyShop();
  if (needy) return needy;
  const all = [...shops.values()];
  const quests = all.filter((s) => s.quests && s.quests.length);
  const pool = quests.length && Math.random() < 0.5 ? quests : all;
  return pool[Math.floor(Math.random() * pool.length)];
}

function rememberShop() {
  try { localStorage.setItem('claude-town-me', me.shop); } catch (e) { /* storage off */ }
}

function stepMe(t) {
  if (!shops.size) return;
  if (!me.shop || !shops.has(me.shop)) {
    // Back where you were last time, if that shop is still in town.
    let saved = null;
    try { saved = localStorage.getItem('claude-town-me'); } catch (e) { /* storage off */ }
    const s = (saved && shops.get(saved)) || needyShop() || [...shops.values()][0];
    me.shop = s.id; me.pos = standAt(s); me.walk = null; me.until = t + 4000;
  }
  if (me.walk) {
    const w = me.walk, d = ((t - w.start) / 1000) * ME_SPEED;
    if (d >= w.len) { me.pos = w.pts[w.pts.length - 1]; me.shop = w.to; me.walk = null; me.until = t + stay(); rememberShop(); }
    else { const p = pointAt(w.pts, d); me.dir = p.x < me.pos.x ? -1 : p.x > me.pos.x ? 1 : me.dir; me.pos = p; }
    return;
  }
  const needy = needyShop();
  // Agents waiting for you call you over at once; otherwise wander after a while.
  const here = shops.get(me.shop);
  if (needy && needy.id === me.shop) { me.until = Math.max(me.until, t + LINGER_MS); return; }
  if (!needy && t < me.until) return;
  const to = needy || nextTarget();
  if (!to || to.id === me.shop) { me.until = t + stay(); return; }
  const pts = route(here, to);
  // Start where you stand and end where you stand at the other shop.
  pts[0] = { ...me.pos };
  pts.splice(1, 0, { x: me.pos.x, y: streetY(here.row) });
  pts.splice(2, 1);
  const end = standAt(to);
  pts[pts.length - 2] = { x: end.x, y: streetY(to.row) };
  pts[pts.length - 1] = end;
  me.walk = { pts, len: lengthOf(pts), start: t, to: to.id };
}

/// You, at the town's scale: 9x13 pixels, a little taller than the agents, in the colours of your gear.
function drawMe(t) {
  if (!player) return;
  stepMe(t);
  if (!me.pos) return;
  const lk = look(), c = lookColors(lk);
  const jumpMs = t - me.jumpAt;
  const jy = jumpMs < 900 ? -Math.round(Math.max(0, Math.sin((jumpMs / 900) * Math.PI)) * 6) : 0;
  const x = me.pos.x - 4, y = me.pos.y - 13 + jy;
  const col = (slot) => { const it = wornIn(slot); return it ? material(it) : null; };
  const back = col('back'), chest = col('chest'), hands = col('hands'), feet = col('feet'), head = wornIn('head');
  const main = col('main_hand'), off = col('off_hand');
  const hairC = HAIR_COLORS[lk.hair_color] || HAIR_COLORS[0];
  if (back) px(x + 1, y + 5, 7, 7, back.base);
  // head: hair or helmet, face
  px(x + 2, y + 2, 5, 3, c.skin);
  px(x + 3, y + 3, 1, 1, INK);
  px(x + 5, y + 3, 1, 1, INK);
  const helmet = head && head.kind === 'helmet' && head.base !== 3 ? material(head) : null;
  px(x + 2, y, 5, 2, helmet ? helmet.base : hairC);
  if (helmet) { px(x + 1, y + 1, 1, 3, helmet.base); px(x + 7, y + 1, 1, 3, helmet.base); }
  else if (lk.hair_style === 2) { px(x + 1, y + 1, 1, 4, hairC); px(x + 7, y + 1, 1, 4, hairC); }
  if (head && head.kind === 'headphones') { const m = material(head); px(x + 1, y + 2, 1, 2, m.base); px(x + 7, y + 2, 1, 2, m.base); px(x + 2, y - 1, 5, 1, m.dark); }
  if (head && head.kind === 'helmet' && head.base === 3) px(x + 2, y + 1, 5, 1, '#d9b24a');
  // body
  const walkF = me.walk ? Math.floor(t / 280) % 2 : -1;
  px(x + 2, y + 5, 5, 4, chest ? chest.base : c.shirt);
  if (chest && chest.trim) px(x + 2, y + 8, 5, 1, chest.trim);
  px(x + 1, y + 5, 1, 3, chest ? chest.base : c.shirt);
  px(x + 7, y + 5, 1, 3, chest ? chest.base : c.shirt);
  px(x + 1, y + 8, 1, 1, hands ? hands.base : c.skin);
  px(x + 7, y + 8, 1, 1, hands ? hands.base : c.skin);
  if (walkF === 0) { px(x + 2, y + 9, 2, 3, c.pants); px(x + 5, y + 9, 2, 2, c.pants); }
  else if (walkF === 1) { px(x + 2, y + 9, 2, 2, c.pants); px(x + 5, y + 9, 2, 3, c.pants); }
  else { px(x + 2, y + 9, 2, 3, c.pants); px(x + 5, y + 9, 2, 3, c.pants); }
  px(x + 2, y + 12 - (walkF === 1 ? 1 : 0), 2, 1, feet ? feet.base : c.shoe);
  px(x + 5, y + 12 - (walkF === 0 ? 1 : 0), 2, 1, feet ? feet.base : c.shoe);
  if (main) px(x, y + 1, 1, 8, main.light);
  if (off) px(x + 8, y + 6, 2, 3, off.base);
  // Legendaries sparkle, even this small.
  const legend = Object.values(player.equipped).map((id) => itemsById.get(id)).find((i) => i && i.rarity === 'legendary');
  if (legend) {
    const ph = Math.floor(t / 260) % 6, a = AURA[legend.aura] || AURA.sparkle;
    px(x + [0, 8, 1, 7, 4, 9][ph], y + [2, 4, 10, 1, -2, 9][ph], 1, 1, a[ph % 3]);
  }
  // A level-up: sparkles all around.
  if (jumpMs < 1400) for (let i = 0; i < 8; i++) {
    const a = (i / 8) * Math.PI * 2 + jumpMs / 300, r = 4 + (jumpMs / 1400) * 9;
    px(Math.round(me.pos.x + Math.cos(a) * r), Math.round(y + 6 + Math.sin(a) * r), 1, 1, i % 2 ? P.yellow : P.white);
  }
  hit('player', 'me', x - 1, y - 2, 12, 17);
  if (hover && hover.kind === 'player') {
    const label = fit((lk.name || 'You') + ' LV' + player.level, 18), w = textWidth(label) + 6;
    px(Math.round(me.pos.x - w / 2), y - 11, w, 9, P.ink);
    text(label, Math.round(me.pos.x - w / 2) + 3, y - 9, P.yellow);
  }
}

// A scroll over a shop that has a quest for you.
const SCROLL = ['.wwwww.', 'wpppppw', '.pkkkp.', '.ppppp.', '.pkkkp.', '.ppppp.', 'wpppppw', '.wwwww.'];
function drawQuestMarker(s, t) {
  const x = s.left - 9, y = s.top - 12 + (Math.floor(t / 700) % 2);
  sprite(SCROLL, x, y, { w: P.woodD, p: '#f3e3b5', k: P.ink });
  if (s.quests.length > 1) { px(x + 5, y - 3, 5, 6, P.red); text(String(Math.min(9, s.quests.length)), x + 6, y - 2, P.white); }
}

function questTip(s) {
  return (player ? player.quests : []).filter((q) => s.quests.includes(q.id));
}

function meTip() {
  const out = [el('b', '', look().name || 'You'), el('div', '', `${player.class.name}, level ${player.level} · ${player.xp} / ${player.need} XP`)];
  out.push(el('div', 'dim', me.walk ? 'On the way to ' + (shops.get(me.walk.to)?.name || 'a shop') : 'At ' + (shops.get(me.shop)?.name || 'the town square')));
  out.push(el('div', 'dim', 'Click to open your character sheet'));
  return out;
}

// ------------------------------------------------------------------ the XP bar

let xpCanvas = null, xpG = null, xpFlashUntil = 0;
const XP_W = 168, XP_H = 15;

function drawXpBar(t) {
  if (!xpG || !player) return;
  const g = xpG, flash = t < xpFlashUntil && Math.floor(t / 120) % 2 === 0;
  g.clearRect(0, 0, XP_W, XP_H);
  // Level badge.
  R(g, 0, 0, 22, XP_H, INK);
  R(g, 1, 1, 20, XP_H - 2, flash ? '#fff3a0' : '#ffd23f');
  R(g, 1, XP_H - 3, 20, 2, '#e0a020');
  const lv = String(player.level);
  textOn(g, lv, 11 - Math.floor((lv.length * 4 - 1) / 2), 5, INK);
  // Bar.
  const bx = 23, bw = XP_W - bx;
  R(g, bx, 2, bw, XP_H - 4, INK);
  R(g, bx + 1, 3, bw - 2, XP_H - 6, '#2b2b3a');
  const fill = Math.round((bw - 2) * Math.min(1, player.xp / Math.max(1, player.need)));
  R(g, bx + 1, 3, fill, XP_H - 6, flash ? '#fff3a0' : '#8a6bff');
  R(g, bx + 1, 3, fill, 1, '#b8a8ff');
  R(g, bx + 1, XP_H - 4, fill, 1, '#5a44c8');
  for (let i = 1; i < 10; i++) R(g, bx + 1 + Math.round(((bw - 2) * i) / 10), 4, 1, XP_H - 8, 'rgba(0,0,0,0.35)');
  const label = `${player.xp}/${player.need} XP`;
  const lx = bx + Math.round((bw - (label.length * 4 - 1)) / 2);
  textOn(g, label, lx + 1, 6, INK);
  textOn(g, label, lx, 5, '#ffffff');
}

function xpTip() {
  const out = [el('b', '', `${player.class.name}, level ${player.level}`), el('div', '', `${player.xp} / ${player.need} XP to level ${player.level + 1} · ${player.total} XP in all from ${player.issues} issue${player.issues === 1 ? '' : 's'}`)];
  if (player.recent.length) {
    out.push(el('div', 'dim', 'Recent XP:'));
    for (const r of player.recent) out.push(el('div', '', `+${r.xp}  ${r.title}`), el('div', 'dim', `   ${r.project}#${r.iid} · weight ${r.weight ?? '–'}`));
  } else out.push(el('div', 'dim', 'Close an issue assigned to you to earn XP.'));
  const s = player.sync;
  if (!s.configured) out.push(el('div', 'dim', 'Set gitlab_url in ~/.config/claude-hub/config.toml to earn XP.'));
  else if (s.error) out.push(el('div', 'dim', 'GitLab sync failed: ' + s.error));
  else if (s.last_ok) out.push(el('div', 'dim', 'Synced ' + ago(s.last_ok)));
  return out;
}

function ago(secs) {
  const d = Math.max(0, Date.now() / 1000 - secs);
  if (d < 90) return 'just now';
  if (d < 5400) return Math.round(d / 60) + ' min ago';
  if (d < 129600) return Math.round(d / 3600) + ' h ago';
  return Math.round(d / 86400) + ' days ago';
}

// ------------------------------------------------------------------ the dock: character, bag, quests

let dockTab = null; // 'character' | 'bag' | 'quests' | null (collapsed)
let dockSig = '';
let hoverItem = null;
let dragId = null;
let dollCanvas = null, dollG = null, dollJumpAt = -1e9;
const $p = (id) => document.getElementById(id);

try { dockTab = localStorage.getItem('claude-town-dock') || null; } catch (e) { /* storage off */ }

function setDock(tabName) {
  dockTab = dockTab === tabName ? null : tabName;
  try { if (dockTab) localStorage.setItem('claude-town-dock', dockTab); else localStorage.removeItem('claude-town-dock'); } catch (e) { /* storage off */ }
  renderDock(true);
}

function openDock(tabName) {
  if (dockTab !== tabName) setDock(tabName);
}

function stripIcon(rows, colors) {
  const [c, g] = newCanvas(12, 12);
  for (let r = 0; r < rows.length; r++) for (let x = 0; x < rows[r].length; x++) R(g, x, r, 1, 1, colors[rows[r][x]]);
  return c;
}
const STRIP_ICONS = {
  character: [['....hhhh....', '...hhhhhh...', '...hssssh...', '...hskssk...', '....ssss....', '...cccccc...', '..scccccccs.', '..s.cccc.s..', '....cccc....', '....pp.pp...', '....pp.pp...', '....kk.kk...'], { h: '#5a3a22', s: '#eec09a', k: INK, c: '#4f7cac', p: '#2f3a4f' }],
  bag: [['....kkkk....', '...k....k...', '...k....k...', '.bbbbbbbbbb.', 'bBBBBBBBBBBb', 'bBBBByyBBBBb', 'bBBBByyBBBBb', 'bBBBBBBBBBBb', 'bBBBBBBBBBBb', 'bBBBBBBBBBBb', '.bbbbbbbbbb.', '............'], { k: '#6e4a2a', b: '#6e4a2a', B: '#a0703f', y: '#ffd23f' }],
  quests: [['.wwwwwwwwww.', 'wppppppppppw', '.pkkkkkkkkp.', '.pppppppppp.', '.pkkkkkkp...', '.pppppppppp.', '.pkkkkkkkkp.', '.pppppppppp.', '.pkkkkkp....', '.pppppppppp.', 'wppppppppppw', '.wwwwwwwwww.'], { w: '#6e4a2a', p: '#f3e3b5', k: '#2b2b3a' }],
};

function initDock() {
  const strip = $p('dock-strip');
  for (const [name, label, key] of [['character', 'Character', 'p'], ['bag', 'Bag', 'i'], ['quests', 'Quests', 'q']]) {
    const b = el('button', 'dtab');
    b.type = 'button';
    b.dataset.tab = name;
    b.title = `${label} (${key})`;
    b.setAttribute('aria-label', label);
    const [rows, colors] = STRIP_ICONS[name];
    b.append(stripIcon(rows, colors));
    b.addEventListener('click', () => setDock(name));
    strip.append(b);
  }
  strip.append(el('div', '', ''), Object.assign(el('div', 'dlevel'), { id: 'dock-level' }), Object.assign(el('div', 'dvbar'), { id: 'dock-vbar' }));
  $p('dock-vbar').append(el('i'));
  [dollCanvas, dollG] = newCanvas(DOLL_W, DOLL_H);
  dollCanvas.id = 'doll';
  dollCanvas.title = 'Click to jump';
  dollCanvas.addEventListener('click', () => { dollJumpAt = performance.now(); });
}

function renderDock(force) {
  const dock = $p('dock');
  if (!dock || !player) return;
  dock.classList.toggle('open', !!dockTab);
  for (const b of dock.querySelectorAll('.dtab')) b.classList.toggle('on', b.dataset.tab === dockTab);
  $p('dock-level').textContent = player.level;
  $p('dock-level').title = `Level ${player.level}`;
  $p('dock-vbar').firstChild.style.height = Math.round((100 * player.xp) / Math.max(1, player.need)) + '%';
  $p('dock-vbar').title = `${player.xp} / ${player.need} XP`;
  const body = $p('dock-body');
  body.hidden = !dockTab;
  updateDockMode();
  if (!dockTab) { dockSig = ''; return; }
  const sig = JSON.stringify([dockTab, player.items, player.equipped, player.quests, player.recent, player.level, player.xp, player.appearance, player.totals]);
  if (!force && sig === dockSig) return;
  dockSig = sig;
  const parts = [header()];
  if (dockTab === 'character') parts.push(sheet(), bagGrid(true));
  else if (dockTab === 'bag') parts.push(bagGrid(false));
  else parts.push(questLog());
  body.replaceChildren(...parts);
}

function header() {
  const h = el('div', 'dhead');
  const name = el('div', 'dname', look().name || 'You');
  const lv = el('span', 'dlv', ` ${player.class.name} · Level ${player.level}`);
  name.append(lv);
  const bar = el('div', 'dxp');
  const fill = el('i');
  fill.style.width = Math.round((100 * player.xp) / Math.max(1, player.need)) + '%';
  bar.append(fill);
  bar.title = `${player.total} XP in all`;
  h.append(name, bar, el('div', 'dxpt', `${player.xp} / ${player.need} XP to level ${player.level + 1}`));
  return h;
}

// ---- The character sheet: the character in the middle of the grey dock, the slots around it in the
// classic layout. Every piece has a fixed position inside a fixed-size frame, so the character can
// never cover a slot; on a narrow dock the whole frame scales down together.

const FRAME_W = 340, FRAME_H = 328, BOX = 48;
const SLOT_AT = {
  head: [0, 0], back: [0, 58], chest: [0, 116], trinket: [0, 174],
  hands: [FRAME_W - BOX, 0], feet: [FRAME_W - BOX, 58],
  main_hand: [FRAME_W / 2 - BOX - 6, 280], off_hand: [FRAME_W / 2 + 6, 280],
};
// The doll at 4x (192x264), centred between the columns (48 px + 26 px gap on each side), above the hands.
const DOLL_AT = [(FRAME_W - DOLL_W * 4) / 2, 6];

function slotBox(slot) {
  const it = wornIn(slot);
  const box = el('div', 'slot');
  box.dataset.slot = slot;
  box.style.left = SLOT_AT[slot][0] + 'px';
  box.style.top = SLOT_AT[slot][1] + 'px';
  if (it) {
    box.append(iconCanvas(it));
    box.style.borderColor = RARITY[it.rarity].color;
    if (it.rarity === 'legendary') box.classList.add('legend');
    box.draggable = true;
    box.addEventListener('dragstart', (e) => startDrag(e, it.id));
    box.addEventListener('dragend', endDrag);
    box.addEventListener('dblclick', () => playerAct({ op: 'unequip', slot }));
    box.addEventListener('mouseenter', (e) => showItemTip(it, e, false));
    box.addEventListener('mousemove', moveItemTip);
    box.addEventListener('mouseleave', hideItemTip);
  } else {
    box.append(el('span', 'ph', SLOT_LABEL[slot]));
    box.title = `${SLOT_LABEL[slot]}: ${(player.slots.find((s) => s.slot === slot)?.kinds || []).map((k) => KIND_LABEL[k]).join(', ')}`;
  }
  box.addEventListener('dragover', (e) => {
    const d = dragId && itemsById.get(dragId);
    if (d && d.slot === slot) { e.preventDefault(); e.dataTransfer.dropEffect = 'move'; }
  });
  box.addEventListener('drop', (e) => {
    e.preventDefault();
    const id = dragId || e.dataTransfer.getData('text/plain');
    endDrag();
    if (id && itemsById.get(id)?.slot === slot && player.equipped[slot] !== id) playerAct({ op: 'equip', item: id, slot });
  });
  return box;
}

function sheet() {
  const wrap = el('div', 'sheet');
  const frame = el('div', 'frame');
  frame.style.width = FRAME_W + 'px';
  frame.style.height = FRAME_H + 'px';
  dollCanvas.style.left = DOLL_AT[0] + 'px';
  dollCanvas.style.top = DOLL_AT[1] + 'px';
  frame.append(dollCanvas, ...Object.keys(SLOT_AT).map(slotBox));
  const holder = el('div', 'frameholder');
  holder.append(frame);
  wrap.append(holder);
  fitFrame(holder, frame);
  const change = el('button', 'linkbtn', 'Change look');
  change.type = 'button';
  change.addEventListener('click', openCreator);
  wrap.append(statsBox(), classBox(), change);
  return wrap;
}

/// Shrink the whole frame, slots and character together, when the dock is narrower than it.
function fitFrame(holder, frame) {
  const body = $p('dock-body');
  const room = (body && body.clientWidth ? body.clientWidth : 400) - 32;
  const k = Math.min(1, room / FRAME_W);
  frame.style.transform = k < 1 ? `scale(${k})` : '';
  holder.style.height = Math.ceil(FRAME_H * k) + 'px';
  holder.style.width = Math.ceil(FRAME_W * k) + 'px';
}

function sizeSheet() {
  const holder = document.querySelector('#dock .frameholder');
  if (holder) fitFrame(holder, holder.firstChild);
}

/// Your class and how close the others are.
function classBox() {
  const c = player.class, box = el('div', 'classbox');
  const head = el('div', 'cname2', c.name);
  box.append(head, el('div', 'cflavor', c.calling ? `The ${c.calling}. ${c.flavor}` : c.flavor));
  const top = c.standings.filter((s) => s.count > 0);
  if (top.length) {
    const list = el('div', 'cstand');
    for (const s of top) {
      const row = el('div', 'crow2' + (s.key === c.key ? ' on' : ''));
      row.append(el('span', '', s.name), el('span', 'cn', `${s.count} closed`));
      list.append(row);
    }
    box.append(list);
  }
  return box;
}

function statsBox() {
  const box = el('div', 'stats');
  box.id = 'dock-stats';
  fillStats(box);
  return box;
}

/// The totals of what you wear; while you hover a bag item, how it would change them.
function fillStats(box) {
  box = box || $p('dock-stats');
  if (!box) return;
  const cmp = hoverItem && !Object.values(player.equipped).includes(hoverItem.id) ? hoverItem.compare || {} : {};
  box.replaceChildren(...STATS.map((s) => {
    const row = el('div', 'stat');
    row.append(el('span', 'sl', STAT_LABEL[s]), el('span', 'sv', String(player.totals[s] || 0)));
    const d = cmp[s];
    if (d) row.append(el('span', d > 0 ? 'up' : 'down', ` ${d > 0 ? '+' : ''}${d}`));
    return row;
  }));
}

function bagGrid(compact) {
  const wrap = el('div', 'bagwrap');
  const bag = player.bag.map((id) => itemsById.get(id)).filter(Boolean);
  wrap.append(el('div', 'dsub', `Bag · ${bag.length} item${bag.length === 1 ? '' : 's'}`));
  const grid = el('div', 'grid');
  for (const it of bag) {
    const cell = el('div', 'cell');
    cell.style.borderColor = RARITY[it.rarity].color;
    if (it.rarity === 'legendary') cell.classList.add('legend');
    cell.append(iconCanvas(it));
    cell.draggable = true;
    cell.tabIndex = 0;
    cell.setAttribute('aria-label', `${it.name}, ${RARITY[it.rarity].label} ${KIND_LABEL[it.kind]}. Enter equips.`);
    cell.addEventListener('dragstart', (e) => startDrag(e, it.id));
    cell.addEventListener('dragend', endDrag);
    cell.addEventListener('dblclick', () => playerAct({ op: 'equip', item: it.id }));
    cell.addEventListener('keydown', (e) => { if (e.key === 'Enter') playerAct({ op: 'equip', item: it.id }); });
    cell.addEventListener('mouseenter', (e) => { hoverItem = it; fillStats(); showItemTip(it, e, true); });
    cell.addEventListener('mousemove', moveItemTip);
    cell.addEventListener('mouseleave', () => { hoverItem = null; fillStats(); hideItemTip(); });
    grid.append(cell);
  }
  const min = compact ? 12 : 24;
  for (let i = bag.length; i < Math.max(min, Math.ceil(bag.length / 6) * 6); i++) grid.append(el('div', 'cell empty'));
  // Drop a worn item on the bag to take it off.
  grid.addEventListener('dragover', (e) => {
    if (dragId && Object.values(player.equipped).includes(dragId)) { e.preventDefault(); grid.classList.add('drop'); }
  });
  grid.addEventListener('dragleave', () => grid.classList.remove('drop'));
  grid.addEventListener('drop', (e) => {
    e.preventDefault();
    grid.classList.remove('drop');
    const id = dragId;
    endDrag();
    if (id && Object.values(player.equipped).includes(id)) playerAct({ op: 'unequip', item: id });
  });
  wrap.append(grid);
  if (!bag.length) wrap.append(el('div', 'dhint', player.items.length ? 'Everything you own is equipped.' : 'Loot drops when you level up. Close issues assigned to you to earn XP.'));
  else wrap.append(el('div', 'dhint', 'Drag an item onto its slot, or double-click it, to equip it.'));
  return wrap;
}

function startDrag(e, id) {
  dragId = id;
  hideItemTip();
  e.dataTransfer.setData('text/plain', id);
  e.dataTransfer.effectAllowed = 'move';
  const it = itemsById.get(id);
  // Light up the slot it fits; dim the rest.
  for (const s of document.querySelectorAll('#dock .slot')) {
    s.classList.toggle('valid', s.dataset.slot === it.slot);
    s.classList.toggle('invalid', s.dataset.slot !== it.slot);
  }
}

function endDrag() {
  dragId = null;
  for (const s of document.querySelectorAll('#dock .slot')) s.classList.remove('valid', 'invalid');
}

function questLog() {
  const wrap = el('div', 'quests');
  wrap.append(el('div', 'dsub', `Quests · ${player.quests.length} open`));
  if (!player.quests.length) wrap.append(el('div', 'dhint', player.sync.configured ? 'No open issues are assigned to you.' : 'Set gitlab_url in ~/.config/claude-hub/config.toml: your open GitLab issues become quests.'));
  for (const q of player.quests) {
    const card = el('div', 'quest');
    const title = el('a', 'qt', q.title);
    if (/^https?:\/\//.test(q.url)) { title.href = q.url; title.target = '_blank'; title.rel = 'noopener noreferrer'; }
    const meta = el('div', 'qm', `${q.project}#${q.iid}`);
    const reward = el('div', 'qr');
    reward.append(el('b', '', `+${q.xp} XP`), ` ${q.weight == null ? 'no weight' : 'weight ' + q.weight}`);
    if (q.due) reward.append(el('span', 'qdue', ` · due ${q.due}`));
    card.append(title, meta, reward);
    if (q.labels.length) {
      const labels = el('div', 'qlabels');
      for (const l of q.labels.slice(0, 5)) labels.append(el('span', 'qlabel', l));
      card.append(labels);
    }
    if (q.shop && shops.has(q.shop)) {
      const go = el('button', 'linkbtn', `Show ${shops.get(q.shop).name} in town`);
      go.type = 'button';
      go.addEventListener('click', () => showShop(q.shop));
      card.append(go);
    }
    wrap.append(card);
  }
  if (player.recent.length) {
    wrap.append(el('div', 'dsub', 'Completed'));
    for (const r of player.recent) {
      const row = el('div', 'done');
      row.append(el('span', 'dx', `+${r.xp}`), ' ', r.title, el('span', 'qm', `  ${r.project}#${r.iid}`));
      wrap.append(row);
    }
  }
  return wrap;
}

function showShop(id) {
  const s = shops.get(id);
  if (!s) return;
  cam.x = Math.round(s.cx - view.w / 2);
  cam.y = Math.round(s.baseY - view.h / 2);
  select({ kind: 'shop', id });
}

/// When the window is too narrow for the town between the dock and the panel, the dock floats over the town.
function updateDockMode() {
  const dock = $p('dock'), panelEl = document.getElementById('panel');
  if (!dock) return;
  const panelW = panelEl && !panelEl.hidden ? panelEl.offsetWidth : 0;
  dock.classList.toggle('over', !!dockTab && window.innerWidth - 44 - 400 - panelW < 520);
}

// ------------------------------------------------------------------ item tooltip

function showItemTip(it, e, compare) {
  const t = $p('itip');
  t.replaceChildren();
  const r = RARITY[it.rarity];
  const name = el('div', 'n', it.name);
  name.style.color = r.color;
  const row = el('div', 'row');
  row.append(el('span', '', SLOT_LABEL[it.slot]), el('span', '', KIND_LABEL[it.kind]));
  t.append(name, row, el('div', 'lvl', `Item Level ${it.item_level}`), el('div', 'rar', r.label));
  if (it.rarity === 'legendary') t.append(el('div', 'grows', 'Grows with you: its item level and stats follow your level.'));
  const worn = compare ? wornIn(it.slot) : null;
  for (const s of STATS) {
    if (!it.stats[s]) continue;
    const line = el('div', 'st', `+${it.stats[s]} ${STAT_LABEL[s]}`);
    t.append(line);
  }
  if (compare && it.compare) {
    const ds = STATS.filter((s) => it.compare[s]);
    if (ds.length) {
      const cmp = el('div', 'cmp', worn ? `Instead of ${worn.name}: ` : 'Equipping it: ');
      ds.forEach((s, i) => { const d = it.compare[s]; cmp.append(el('span', d > 0 ? 'up' : 'down', `${d > 0 ? '+' : ''}${d} ${STAT_LABEL[s]}`), i < ds.length - 1 ? ', ' : ''); });
      t.append(cmp);
    }
  }
  if (it.flavor) t.append(el('div', 'fl', `“${it.flavor}”`));
  t.append(el('div', 'src', `Dropped at level ${it.dropped_at_level}` + (it.trigger ? `, for closing ${it.trigger.project}#${it.trigger.iid}: ${it.trigger.title}` : '')));
  t.hidden = false;
  moveItemTip(e);
}

function moveItemTip(e) {
  const t = $p('itip');
  if (t.hidden) return;
  const w = t.offsetWidth, h = t.offsetHeight;
  t.style.left = Math.max(8, Math.min(e.clientX + 16, window.innerWidth - w - 8)) + 'px';
  t.style.top = Math.max(8, Math.min(e.clientY + 16, window.innerHeight - h - 8)) + 'px';
}

function hideItemTip() { const t = $p('itip'); if (t) t.hidden = true; }

// ------------------------------------------------------------------ actions

async function playerAct(body) {
  try {
    const res = await fetch('/api/player', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', 'X-Town-Token': TOKEN },
      body: JSON.stringify(body),
    });
    const data = await res.json();
    if (!res.ok) { toast(data.error || 'That did not work.', true); return false; }
    if (data.ok && body.op !== 'ack') sfx('equip');
    return true;
  } catch (e) {
    toast('The town server does not answer.', true);
    return false;
  }
}

// ------------------------------------------------------------------ level-ups, loot and quests done

const queue = [];
const celebrated = { level: 0, seq: 0 };
let busy = false;

function queueCelebrations() {
  const fresh = player.completed.filter((q) => q.seq > celebrated.seq);
  if (fresh.length) {
    celebrated.seq = Math.max(...fresh.map((q) => q.seq));
    // A big sync: one toast for all of them.
    if (player.completed_total > 2) queue.push({ type: 'quest', q: { title: `${player.completed_total} issues closed`, xp: player.completed_xp } });
    else for (const q of fresh) queue.push({ type: 'quest', q });
  }
  for (const c of player.celebrate) if (c.level > celebrated.level) { queue.push({ type: 'level', ...c }); celebrated.level = c.level; }
  runQueue();
}

function runQueue() {
  if (busy || !queue.length) return;
  // Not over the character maker: wait until it is closed.
  if (!$p('creator').hidden) { setTimeout(runQueue, 500); return; }
  busy = true;
  const next = queue.shift();
  if (next.type === 'quest') {
    sfx('quest');
    toast(`Quest complete! ${next.q.title}  +${next.q.xp} XP`);
    setTimeout(() => {
      if (!queue.some((x) => x.type === 'quest')) playerAct({ op: 'ack', seq: player.last_seq });
      busy = false;
      runQueue();
    }, 2600);
  } else levelUp(next);
}

function levelUp(c) {
  const t = performance.now();
  me.jumpAt = t;
  dollJumpAt = t;
  xpFlashUntil = t + 1600;
  sfx('level');
  const it = c.item && itemsById.get(c.item);
  const box = $p('loot');
  const card = el('div', 'lootcard');
  const [banner, bg] = newCanvas(64, 9);
  textOn(bg, `LEVEL ${c.level}`, Math.round((64 - (`LEVEL ${c.level}`.length * 4 - 1)) / 2), 2, '#ffd23f');
  banner.className = 'lvbanner';
  card.append(banner);
  if (it) {
    card.style.borderColor = RARITY[it.rarity].color;
    card.style.boxShadow = `0 0 32px ${RARITY[it.rarity].color}66`;
    const icon = iconCanvas(it, 96);
    icon.classList.add('drop');
    const name = el('div', 'ln', it.name);
    name.style.color = RARITY[it.rarity].color;
    card.append(el('div', 'lsub', 'You found loot'), icon, name, el('div', 'lk', `${RARITY[it.rarity].label} ${KIND_LABEL[it.kind]} · item level ${it.item_level}`));
    if (it.rarity === 'legendary') card.append(el('div', 'grows', 'Grows with you as you level up.'));
    const stats = el('div', 'lstats', STATS.filter((s) => it.stats[s]).map((s) => `+${it.stats[s]} ${STAT_LABEL[s]}`).join('   '));
    card.append(stats);
    if (it.flavor) card.append(el('div', 'fl', `“${it.flavor}”`));
    if (it.trigger) card.append(el('div', 'src', `For closing ${it.trigger.project}#${it.trigger.iid}: ${it.trigger.title}`));
    setTimeout(() => sfx('loot', it.rarity), 500);
  }
  const buttons = el('div', 'lbtns');
  const done = (equip) => {
    clearTimeout(timer);
    box.hidden = true;
    box.replaceChildren();
    if (equip && it) playerAct({ op: 'equip', item: it.id });
    playerAct({ op: 'ack', level: c.level }).then(() => { busy = false; runQueue(); });
  };
  if (it) {
    const eq = el('button', 'lbtn main', 'Equip');
    eq.type = 'button';
    eq.addEventListener('click', () => done(true));
    const keep = el('button', 'lbtn', 'Put in bag');
    keep.type = 'button';
    keep.addEventListener('click', () => done(false));
    buttons.append(eq, keep);
  } else {
    const ok = el('button', 'lbtn main', 'Nice');
    ok.type = 'button';
    ok.addEventListener('click', () => done(false));
    buttons.append(ok);
  }
  card.append(buttons);
  box.replaceChildren(card);
  box.hidden = false;
  lootDone = done;
  // Into the bag by itself after a while, so a big sync does not need a click per level.
  const timer = setTimeout(() => done(false), 15000);
  buttons.firstChild.focus();
}

let lootDone = null;
let toastTimer = null;
function toast(msg, error) {
  const t = $p('toast');
  if (!t) return;
  t.textContent = msg;
  t.classList.toggle('err', !!error);
  t.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { t.hidden = true; }, 2500);
}

// ------------------------------------------------------------------ sound (only with sound = true)

let audio = null;
function sfx(kind, rarity) {
  if (!player || !player.sound) return;
  try {
    audio = audio || new AudioContext();
    if (audio.state === 'suspended') audio.resume();
    const notes = {
      level: [523, 659, 784, 1047],
      quest: [659, 988],
      equip: [330, 440],
      loot: { poor: [392], common: [523], uncommon: [523, 784], rare: [587, 880], epic: [659, 988, 1319], legendary: [523, 784, 1047, 1568] }[rarity] || [523],
    }[kind] || [440];
    notes.forEach((f, i) => {
      const o = audio.createOscillator(), g = audio.createGain(), at = audio.currentTime + i * 0.09;
      o.type = kind === 'equip' ? 'triangle' : 'square';
      o.frequency.value = f;
      g.gain.setValueAtTime(0.0001, at);
      g.gain.exponentialRampToValueAtTime(0.05, at + 0.01);
      g.gain.exponentialRampToValueAtTime(0.0001, at + 0.16);
      o.connect(g).connect(audio.destination);
      o.start(at);
      o.stop(at + 0.18);
    });
  } catch (e) { /* no audio */ }
}

// ------------------------------------------------------------------ making your character

let creatorLook = null, creatorG = null, creatorShown = false;

function openCreator() {
  const box = $p('creator');
  creatorLook = { ...DEFAULT_LOOK, ...(player.appearance || {}) };
  const card = el('div', 'lootcard creator');
  card.append(el('div', 'ln', player.appearance ? 'Change your look' : 'Make your character'));
  const [c, g] = newCanvas(DOLL_W, DOLL_H);
  c.className = 'cprev';
  creatorG = g;
  card.append(c);
  const name = el('input', 'cname');
  name.value = creatorLook.name;
  name.maxLength = 24;
  name.setAttribute('aria-label', 'Name');
  name.addEventListener('input', () => { creatorLook.name = name.value; });
  // Enter saves; looked up when pressed, since the button is made further down.
  name.addEventListener('keydown', (e) => { if (e.key === 'Enter') { e.preventDefault(); card.querySelector('.lbtn.main').click(); } });
  card.append(name);
  const swatches = (label, key, colors) => {
    const row = el('div', 'crow');
    row.append(el('span', 'cl', label));
    colors.forEach((col, i) => {
      const b = el('button', 'sw' + (creatorLook[key] === i ? ' on' : ''));
      b.type = 'button';
      b.style.background = col;
      b.setAttribute('aria-label', `${label} ${i + 1}`);
      b.addEventListener('click', () => { creatorLook[key] = i; row.querySelectorAll('.sw').forEach((x, j) => x.classList.toggle('on', j === i)); });
      row.append(b);
    });
    return row;
  };
  const styles = el('div', 'crow');
  styles.append(el('span', 'cl', 'Hair'));
  HAIR_STYLES.forEach((s, i) => {
    const b = el('button', 'chip' + (creatorLook.hair_style === i ? ' on' : ''), s);
    b.type = 'button';
    b.addEventListener('click', () => { creatorLook.hair_style = i; styles.querySelectorAll('.chip').forEach((x, j) => x.classList.toggle('on', j === i)); });
    styles.append(b);
  });
  card.append(swatches('Skin', 'skin', SKINS), styles, swatches('Colour', 'hair_color', HAIR_COLORS), swatches('Outfit', 'outfit', OUTFITS));
  const buttons = el('div', 'lbtns');
  const save = el('button', 'lbtn main', 'Save');
  save.type = 'button';
  save.addEventListener('click', async () => { if (await playerAct({ op: 'appearance', appearance: creatorLook })) closeCreator(); });
  const later = el('button', 'lbtn', player.appearance ? 'Cancel' : 'Later');
  later.type = 'button';
  later.addEventListener('click', closeCreator);
  buttons.append(save, later);
  card.append(buttons);
  box.replaceChildren(card);
  box.hidden = false;
  name.focus();
}

function closeCreator() {
  const box = $p('creator');
  box.hidden = true;
  box.replaceChildren();
  creatorG = null;
}

// ------------------------------------------------------------------ hooks for town.js

let uiReady = false;

/// A new model arrived.
function onPlayer(pl) {
  if (!pl) return;
  player = pl;
  itemsById = new Map(pl.items.map((i) => [i.id, i]));
  if (!uiReady) {
    uiReady = true;
    initDock();
    xpCanvas = $p('xpbar');
    xpCanvas.width = XP_W;
    xpCanvas.height = XP_H;
    xpG = xpCanvas.getContext('2d');
    xpG.imageSmoothingEnabled = false;
    xpCanvas.addEventListener('mousemove', (e) => showTipFor(xpTip(), e));
    xpCanvas.addEventListener('mouseleave', () => { tip.hidden = true; });
    xpCanvas.addEventListener('click', () => openDock('character'));
    new ResizeObserver(() => { updateDockMode(); sizeSheet(); }).observe(document.getElementById('app'));
  }
  const scale = window.innerWidth < 700 ? 2 : 3;
  xpCanvas.style.width = XP_W * scale + 'px';
  xpCanvas.style.height = XP_H * scale + 'px';
  renderDock(false);
  queueCelebrations();
  if (!pl.appearance && !creatorShown) { creatorShown = true; openCreator(); }
}

/// Every frame: the XP bar, the doll in the sheet and the preview in the creator.
function playerFrame(t) {
  if (!player) return;
  drawXpBar(t);
  if (dockTab === 'character' && dollG) {
    const j = t - dollJumpAt;
    const anim = j < 900 ? 'jump' : me.walk ? 'walk' : 'idle';
    renderDoll(dollG, look(), player.layers, itemsById, anim, anim === 'jump' ? j : t, t);
  }
  if (creatorG) {
    // The preview shows every animation in turn: idle, walk, jump.
    const ph = t % 6000, anim = ph < 3000 ? 'idle' : ph < 5000 ? 'walk' : 'jump';
    renderDoll(creatorG, creatorLook, [{ part: 'body' }, { part: 'hair' }], new Map(), anim, anim === 'jump' ? ph - 5000 : t, t);
  }
}

function showTipFor(nodes, e) {
  tip.replaceChildren(...nodes);
  tip.hidden = false;
  const r = stage.getBoundingClientRect();
  tip.style.left = Math.max(8, Math.min(e.clientX - r.left + 14, r.width - tip.offsetWidth - 8)) + 'px';
  tip.style.top = Math.max(8, Math.min(e.clientY - r.top - tip.offsetHeight - 14, r.height - tip.offsetHeight - 8)) + 'px';
}

/// p, i, q open the dock's tabs (character, bag, quests); Esc closes the loot card, the creator, then the dock. True when handled.
function playerKey(e) {
  if (!$p('loot').hidden) {
    if (e.key === 'Escape') { e.preventDefault(); lootDone && lootDone(false); return true; }
    return false;
  }
  if (!$p('creator').hidden) {
    if (e.key === 'Escape') { closeCreator(); return true; }
    return false;
  }
  // p, not c: c always means Claude, as in the hub.
  const tabs = { p: 'character', i: 'bag', q: 'quests' };
  if (tabs[e.key]) { e.preventDefault(); setDock(tabs[e.key]); return true; }
  if (e.key === 'Escape' && !selected && dockTab) { setDock(dockTab); return true; }
  return false;
}
