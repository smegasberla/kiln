/** A generated fallback icon.
 *
 *  appimagetool refuses to package without an icon, and a new project has none,
 *  so the packager draws one: a rounded square in the Kiln accent color with a
 *  simple kiln-arch glyph. Pure PNG encoding, no image library.
 */

/** PNG signature + a minimal RGBA encoder (filter type 0, zlib store). */
export function encodePng(width: number, height: number, rgba: Uint8Array): Uint8Array {
  const raw = new Uint8Array((width * 4 + 1) * height);
  for (let y = 0; y < height; y++) {
    raw[y * (width * 4 + 1)] = 0; // filter: none
    const src = y * width * 4;
    raw.set(rgba.subarray(src, src + width * 4), y * (width * 4 + 1) + 1);
  }

  const zlib = deflateStored(raw);
  const chunks: Uint8Array[] = [
    u32be(0x89504e47),
    u32be(0x0d0a1a0a),
    chunk("IHDR", ihdr(width, height)),
    chunk("IDAT", zlib),
    chunk("IEND", new Uint8Array(0)),
  ];
  return concat(chunks);
}

function ihdr(w: number, h: number): Uint8Array {
  const b = new Uint8Array(13);
  writeU32(b, 0, w);
  writeU32(b, 4, h);
  b[8] = 8; // bit depth
  b[9] = 6; // color type: RGBA
  return b;
}

/** zlib-wrapped "stored" deflate blocks: valid, if not compressed. */
function deflateStored(data: Uint8Array): Uint8Array {
  const blocks: Uint8Array[] = [new Uint8Array([0x78, 0x01])];
  const MAX = 65535;
  for (let off = 0; off < data.length; off += MAX) {
    const len = Math.min(MAX, data.length - off);
    const last = off + len >= data.length ? 1 : 0;
    const header = new Uint8Array(5);
    header[0] = last;
    header[1] = len & 0xff;
    header[2] = (len >> 8) & 0xff;
    header[3] = ~len & 0xff;
    header[4] = (~len >> 8) & 0xff;
    blocks.push(header, data.subarray(off, off + len));
  }
  blocks.push(u32be(adler32(data)));
  return concat(blocks);
}

function chunk(type: string, payload: Uint8Array): Uint8Array {
  const typeBytes = new TextEncoder().encode(type);
  const body = concat([typeBytes, payload]);
  return concat([u32be(payload.length), body, u32be(crc32(body))]);
}

const CRC_TABLE = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});

function crc32(data: Uint8Array): number {
  let c = 0xffffffff;
  for (const byte of data) c = CRC_TABLE[(c ^ byte) & 0xff]! ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function adler32(data: Uint8Array): number {
  let a = 1;
  let b = 0;
  for (const byte of data) {
    a = (a + byte) % 65521;
    b = (b + a) % 65521;
  }
  return ((b << 16) | a) >>> 0;
}

function u32be(n: number): Uint8Array {
  const b = new Uint8Array(4);
  writeU32(b, 0, n);
  return b;
}

function writeU32(b: Uint8Array, off: number, n: number): void {
  b[off] = (n >>> 24) & 0xff;
  b[off + 1] = (n >>> 16) & 0xff;
  b[off + 2] = (n >>> 8) & 0xff;
  b[off + 3] = n & 0xff;
}

function concat(parts: Uint8Array[]): Uint8Array {
  const total = parts.reduce((sum, p) => sum + p.length, 0);
  const out = new Uint8Array(total);
  let off = 0;
  for (const part of parts) {
    out.set(part, off);
    off += part.length;
  }
  return out;
}

/** Draw the fallback icon: rounded square, arch glyph, transparent corners. */
export function defaultIcon(size = 256): Uint8Array {
  const px = new Uint8Array(size * size * 4);
  const radius = size * 0.22;
  const accent = [168, 85, 43]; // #a8552b, the Kiln accent
  const cream = [246, 235, 222];

  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const i = (y * size + x) * 4;
      const inCorner = cornerDistance(x, y, size, radius) > radius;
      if (inCorner) continue; // transparent

      px[i] = accent[0]!;
      px[i + 1] = accent[1]!;
      px[i + 2] = accent[2]!;
      px[i + 3] = 255;

      // The arch: a rectangle narrowed to the top with a rounded crown.
      const cx = size / 2;
      const halfWidth = size * 0.22;
      const top = size * 0.3;
      const bottom = size * 0.72;
      const inArchBody = y >= top + halfWidth && y <= bottom && Math.abs(x - cx) <= halfWidth;
      const dx = x - cx;
      const dy = y - (top + halfWidth);
      const inArchCrown = y < top + halfWidth && dx * dx + dy * dy <= halfWidth * halfWidth;
      if (inArchBody || inArchCrown) {
        px[i] = cream[0]!;
        px[i + 1] = cream[1]!;
        px[i + 2] = cream[2]!;
      }
    }
  }
  return encodePng(size, size, px);
}

/** Distance from the nearest rounded-corner center, 0 when not near a corner. */
function cornerDistance(x: number, y: number, size: number, radius: number): number {
  const cx = x < radius ? radius : x > size - radius ? size - radius : x;
  const cy = y < radius ? radius : y > size - radius ? size - radius : y;
  if (cx === x && cy === y) return 0;
  return Math.hypot(x - cx, y - cy);
}
