import { describe, expect, test } from "bun:test";
import { inflateSync } from "node:zlib";
import { defaultIcon } from "./icon.ts";

/** One decoded image: raw RGBA rows plus a pixel accessor. */
type Rgba = { size: number; raw: Uint8Array };

/** Decodes a PNG produced by our own encoder. */
function decode(png: Uint8Array, size: number): Rgba {
  // `inflateSync` from node:zlib unwraps the zlib header. Bun's own helper does
  // not, and reports "invalid stored block lengths" on a stream a viewer reads
  // fine, which is a trap worth naming rather than rediscovering.
  return { size, raw: inflateSync(Buffer.from(idat(png))) };
}

/** The RGBA of one pixel. `raw` keeps a filter byte at the start of each row. */
function pixel(image: Rgba, x: number, y: number): number[] {
  const at = y * (image.size * 4 + 1) + 1 + x * 4;
  const { raw } = image;
  return [raw[at]!, raw[at + 1]!, raw[at + 2]!, raw[at + 3]!];
}

/** Extracts and concatenates every IDAT chunk's payload. */
function idat(png: Uint8Array): Uint8Array {
  const parts: Uint8Array[] = [];
  let at = 8;
  while (at < png.length) {
    const length =
      (png[at]! << 24) | (png[at + 1]! << 16) | (png[at + 2]! << 8) | png[at + 3]!;
    const type = String.fromCharCode(...png.subarray(at + 4, at + 8));
    if (type === "IDAT") parts.push(png.subarray(at + 8, at + 8 + length));
    at += 12 + length;
  }
  const total = parts.reduce((sum, part) => sum + part.length, 0);
  const out = new Uint8Array(total);
  let off = 0;
  for (const part of parts) {
    out.set(part, off);
    off += part.length;
  }
  return out;
}

const isOpaque = (px: number[]) => px[3]! > 128;
const isWarm = (px: number[]) => px[0]! - px[2]! > 50;

describe("defaultIcon", () => {
  test("is mirrored exactly about its vertical axis", () => {
    // The icon is drawn on a pixel grid, so the only correct symmetry is exact.
    // Off-by-one bounds and a fractional centre both showed up here as a right
    // edge one pixel out from the left, which is invisible in the numbers but
    // obvious as a lopsided border.
    for (const size of [16, 24, 32, 64, 128, 256]) {
      const image = decode(defaultIcon(size), size);
      let mismatches = 0;
      for (let y = 0; y < size; y++) {
        for (let x = 0; x < size; x++) {
          const left = isOpaque(pixel(image, x, y));
          const right = isOpaque(pixel(image, size - 1 - x, y));
          if (left !== right) mismatches++;
        }
      }
      expect({ size, mismatches }).toEqual({ size, mismatches: 0 });
    }
  });

  test("leaves an equal margin on all four sides", () => {
    for (const size of [16, 32, 64, 256]) {
      const image = decode(defaultIcon(size), size);
      const middle = size >> 1;

      let left = 0;
      while (left < size && !isOpaque(pixel(image, left, middle))) left++;
      let right = size - 1;
      while (right > 0 && !isOpaque(pixel(image, right, middle))) right--;
      let top = 0;
      while (top < size && !isOpaque(pixel(image, middle, top))) top++;
      let bottom = size - 1;
      while (bottom > 0 && !isOpaque(pixel(image, middle, bottom))) bottom--;

      expect({ size, side: left, opposite: size - 1 - right }).toEqual({
        size,
        side: left,
        opposite: left,
      });
      expect({ size, side: top, opposite: size - 1 - bottom }).toEqual({
        size,
        side: top,
        opposite: top,
      });
    }
  });

  test("puts the lit doorway on the centre column and the stone around it", () => {
    const size = 64;
    const image = decode(defaultIcon(size), size);
    const middle = size >> 1;

    // The opening is warm and the tile is not, which is the whole figure: a lit
    // archway cut out of dark stone, not the inverse.
    expect(isWarm(pixel(image, middle, Math.floor(size * 0.85)))).toBe(true);
    expect(isWarm(pixel(image, middle - Math.floor(size * 0.32), middle))).toBe(false);
    // Above the crown there is stone, so the opening does not reach the top edge.
    expect(isWarm(pixel(image, middle, Math.floor(size * 0.28)))).toBe(false);
  });

  test("scales without leaving the tile transparent at the centre", () => {
    for (const size of [8, 16, 256]) {
      const image = decode(defaultIcon(size), size);
      const middle = size >> 1;
      expect(isOpaque(pixel(image, middle, middle))).toBe(true);
    }
  });
});
