/** A generated fallback icon.
 *
 *  appimagetool refuses to package without an icon, and a new project has none,
 *  so the packager draws one: a rounded square in the Voussoir accent color with a
 *  simple voussoir-arch glyph. Pure PNG encoding, no image library.
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

/** Draw the fallback icon: a dark rounded tile with a lit archway cut out of it.
 *
 *  This is the same figure as `brand/voussoir.svg`, expressed in fractions of the
 *  tile so it scales to any size: an opening whose crown is a semicircle of the
 *  doorway's own half-width, springing to straight jambs, with a thin warm rim
 *  showing around the stone. Dark tile and warm opening rather than the reverse,
 *  because a lit doorway is the thing the eye should land on.
 */
export function defaultIcon(size = 256): Uint8Array {
  const px = new Uint8Array(size * size * 4);
  // Every measurement comes from the 256-unit grid the SVG uses and is snapped
  // to a whole pixel. A fractional edge has no subpixel colour to blend with
  // here, so it would resolve as a one-pixel-thick lopsided border instead of a
  // smooth edge.
  const at = (units: number) => Math.round((size * units) / 256);

  const radius = at(56);
  const margin = at(16);
  const halfWidth = at(48);
  const spring = at(144);
  const bottom = at(240);
  const rim = Math.max(1, at(4));

  // The figure is mirrored about the centre of the pixel grid. Pairing a floor
  // with a ceil keeps the left and right edges exactly equidistant from that
  // axis: `size / 2` is half a pixel right of centre on an even size, and
  // rounding both sides the same way would still bias one of them outward.
  const axis = (size - 1) / 2;
  const left = Math.floor(axis - halfWidth);
  const right = Math.ceil(axis + halfWidth);
  const crownY = spring - halfWidth;
  const rimLeft = Math.floor(axis - halfWidth - rim);
  const rimRight = Math.ceil(axis + halfWidth + rim);
  const rimCrown = crownY - rim;

  // The warm side of the palette, brightest at the crown so the opening reads as
  // lit from above rather than as a flat rectangle of orange.
  const crown = [255, 201, 122];
  const waist = [240, 122, 34];
  const sill = [124, 38, 5];
  const stoneTop = [39, 44, 56];
  const stoneBottom = [12, 15, 20];

  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const i = (y * size + x) * 4;
      // Half-open on the far edges: pixel indices run 0..size-1, so `<=` would
      // paint one extra column and row and make the right and bottom margins
      // narrower than the left and top by a pixel.
      const inTile = x >= margin && x < size - margin && y >= margin && y < size - margin;
      if (!inTile || cornerDistance(x, y, size, radius, margin) > radius) continue; // transparent

      const dx = x - axis;
      const inDoorway =
        x >= left &&
        x <= right &&
        y >= crownY &&
        y < bottom &&
        (y >= spring || dx * dx + (y - spring) ** 2 <= halfWidth * halfWidth);

      if (inDoorway) {
        // Light falls off down the opening, so the freshest edge is the crown.
        const t = (y - crownY) / (bottom - crownY);
        const lower = mix(crown, waist, Math.min(1, t / 0.6));
        const colour = t < 0.6 ? lower : mix(waist, sill, (t - 0.6) / 0.4);
        set(px, i, colour);
        continue;
      }

      // A hairline of warmth just outside the opening, which is what stops the
      // dark tile from looking flat at small sizes.
      const nearRim =
        x >= rimLeft &&
        x <= rimRight &&
        y >= rimCrown &&
        y < bottom &&
        (y >= spring ||
          dx * dx + (y - spring) ** 2 <= (halfWidth + rim) * (halfWidth + rim));
      if (nearRim) {
        set(px, i, waist);
        continue;
      }

      const t = (y - margin) / (bottom - margin);
      set(px, i, mix(stoneTop, stoneBottom, Math.min(1, Math.max(0, t))));
    }
  }
  return encodePng(size, size, px);
}

/** Writes an opaque pixel. */
function set(px: Uint8Array, i: number, colour: number[]): void {
  px[i] = colour[0]!;
  px[i + 1] = colour[1]!;
  px[i + 2] = colour[2]!;
  px[i + 3] = 255;
}

/** Linear blend between two RGB triples. */
function mix(from: number[], to: number[], t: number): number[] {
  return [
    Math.round(from[0]! + (to[0]! - from[0]!) * t),
    Math.round(from[1]! + (to[1]! - from[1]!) * t),
    Math.round(from[2]! + (to[2]! - from[2]!) * t),
  ];
}

/** Distance from the nearest rounded-corner center, 0 when not near a corner. */
function cornerDistance(
  x: number,
  y: number,
  size: number,
  radius: number,
  margin: number,
): number {
  const lo = margin + radius;
  // The last index is `size - 1`, so the far corner has to be measured from
  // there. Using `size - margin - radius` puts it one pixel outboard and makes
  // the right-hand corners flatter than the left.
  const hi = size - 1 - margin - radius;
  if (hi <= lo) return 0; // too small for a corner to exist
  const cx = x < lo ? lo : x > hi ? hi : x;
  const cy = y < lo ? lo : y > hi ? hi : y;
  if (cx === x && cy === y) return 0;
  return Math.hypot(x - cx, y - cy);
}
