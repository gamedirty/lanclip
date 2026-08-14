// Generates app icons without external dependencies:
//   icons/32x32.png, icons/128x128.png, icons/icon.ico
// A clipboard glyph: indigo rounded square + white board + clip + text lines.
import { deflateSync } from "node:zlib";
import { writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const outDir = join(root, "src-tauri", "icons");

// ---------- PNG encoder ----------
const CRC_TABLE = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c >>> 0;
  }
  return t;
})();

function crc32(buf) {
  let c = 0xffffffff;
  for (let i = 0; i < buf.length; i++) c = CRC_TABLE[(c ^ buf[i]) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const out = Buffer.alloc(8 + data.length + 4);
  out.writeUInt32BE(data.length, 0);
  out.write(type, 4, "ascii");
  data.copy(out, 8);
  out.writeUInt32BE(crc32(Buffer.concat([Buffer.from(type, "ascii"), data])), 8 + data.length);
  return out;
}

function encodePNG(w, h, rgba) {
  const sig = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0);
  ihdr.writeUInt32BE(h, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // color type RGBA
  const stride = w * 4;
  const raw = Buffer.alloc((stride + 1) * h);
  for (let y = 0; y < h; y++) {
    raw[y * (stride + 1)] = 0; // filter: none
    rgba.copy(raw, y * (stride + 1) + 1, y * stride, (y + 1) * stride);
  }
  const idat = deflateSync(raw, { level: 9 });
  return Buffer.concat([sig, chunk("IHDR", ihdr), chunk("IDAT", idat), chunk("IEND", Buffer.alloc(0))]);
}

// ---------- tiny raster canvas ----------
function makeCanvas(size) {
  return { size, px: new Uint8Array(size * size * 4) };
}

function blend(c, x, y, [r, g, b, a]) {
  const i = (y * c.size + x) * 4;
  const sa = a / 255;
  const da = c.px[i + 3] / 255;
  const oa = sa + da * (1 - sa);
  if (oa === 0) return;
  c.px[i] = Math.round((r * sa + c.px[i] * da * (1 - sa)) / oa);
  c.px[i + 1] = Math.round((g * sa + c.px[i + 1] * da * (1 - sa)) / oa);
  c.px[i + 2] = Math.round((b * sa + c.px[i + 2] * da * (1 - sa)) / oa);
  c.px[i + 3] = Math.round(oa * 255);
}

function insideRoundRect(px, py, x, y, w, h, r) {
  if (px < x || px >= x + w || py < y || py >= y + h) return false;
  const rx = x + w - r;
  const ry = y + h - r;
  const cx = px < x + r ? x + r : px > rx ? rx : px;
  const cy = py < y + r ? y + r : py > ry ? ry : py;
  const dx = px - cx;
  const dy = py - cy;
  return dx * dx + dy * dy <= r * r;
}

function fillRoundRect(c, x, y, w, h, r, color) {
  // 2x supersampling for smooth edges
  for (let py = Math.floor(y) - 1; py < Math.ceil(y + h) + 1; py++) {
    for (let pxx = Math.floor(x) - 1; pxx < Math.ceil(x + w) + 1; pxx++) {
      if (pxx < 0 || py < 0 || pxx >= c.size || py >= c.size) continue;
      let cov = 0;
      for (const [ox, oy] of [[0.25, 0.25], [0.75, 0.25], [0.25, 0.75], [0.75, 0.75]]) {
        if (insideRoundRect(pxx + ox, py + oy, x, y, w, h, r)) cov++;
      }
      if (cov > 0) blend(c, pxx, py, [color[0], color[1], color[2], (color[3] * cov) / 4]);
    }
  }
}

function drawIcon(size) {
  const c = makeCanvas(size);
  const s = (v) => v * size; // scale helper

  // background: indigo rounded square
  fillRoundRect(c, s(0.04), s(0.04), s(0.92), s(0.92), s(0.2), [79, 70, 229, 255]);

  // clipboard board: white
  const bw = s(0.56);
  const bh = s(0.62);
  const bx = (size - bw) / 2;
  const by = s(0.2);
  fillRoundRect(c, bx, by, bw, bh, s(0.07), [255, 255, 255, 255]);

  // clip: indigo pill overlapping the board top
  const cw = s(0.24);
  const chh = s(0.11);
  fillRoundRect(c, (size - cw) / 2, by - s(0.045), cw, chh, s(0.035), [67, 56, 202, 255]);

  // text lines: slate
  fillRoundRect(c, bx + bw * 0.16, by + bh * 0.26, bw * 0.68, s(0.045), s(0.02), [148, 163, 184, 255]);
  fillRoundRect(c, bx + bw * 0.16, by + bh * 0.44, bw * 0.68, s(0.045), s(0.02), [148, 163, 184, 255]);
  fillRoundRect(c, bx + bw * 0.16, by + bh * 0.62, bw * 0.42, s(0.045), s(0.02), [148, 163, 184, 255]);
  return c;
}

// ---------- ICO with embedded PNG entries ----------
function makeICO(entries) {
  // entries: [{png: Buffer, size: number}]
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0);
  header.writeUInt16LE(1, 2); // type: icon
  header.writeUInt16LE(entries.length, 4);
  const dir = Buffer.alloc(16 * entries.length);
  let offset = 6 + 16 * entries.length;
  const blobs = [];
  entries.forEach((e, i) => {
    const base = i * 16;
    dir.writeUInt8(e.size >= 256 ? 0 : e.size, base);
    dir.writeUInt8(e.size >= 256 ? 0 : e.size, base + 1);
    dir.writeUInt8(0, base + 2); // colors
    dir.writeUInt8(0, base + 3);
    dir.writeUInt16LE(1, base + 4); // planes
    dir.writeUInt16LE(32, base + 6); // bpp
    dir.writeUInt32LE(e.png.length, base + 8);
    dir.writeUInt32LE(offset, base + 12);
    offset += e.png.length;
    blobs.push(e.png);
  });
  return Buffer.concat([header, dir, ...blobs]);
}

mkdirSync(outDir, { recursive: true });
const png32 = encodePNG(32, 32, Buffer.from(drawIcon(32).px.buffer));
const png128 = encodePNG(128, 128, Buffer.from(drawIcon(128).px.buffer));
writeFileSync(join(outDir, "32x32.png"), png32);
writeFileSync(join(outDir, "128x128.png"), png128);
writeFileSync(join(outDir, "icon.ico"), makeICO([
  { png: png32, size: 32 },
  { png: png128, size: 128 },
]));
console.log("icons written to", outDir);
