// Draws the app icon (door with a keyhole) as a PNG without image dependencies; `bun tauri icon app-icon.png` fans it out.
import { deflateSync } from "node:zlib";

const SIZE = 1024;
const px = new Uint8Array(SIZE * SIZE * 4);

const inRoundRect = (x: number, y: number, r: number) => {
  const dx = Math.max(Math.abs(x - SIZE / 2) - (SIZE / 2 - r), 0);
  const dy = Math.max(Math.abs(y - SIZE / 2) - (SIZE / 2 - r), 0);
  return dx * dx + dy * dy <= r * r;
};

for (let y = 0; y < SIZE; y++) {
  for (let x = 0; x < SIZE; x++) {
    const i = (y * SIZE + x) * 4;
    if (!inRoundRect(x, y, 200)) continue;
    const t = y / SIZE;
    let [r, g, b] = [14 + 10 * t, 17 + 14 * t, 23 + 26 * t];
    const inDoor = x > 312 && x < 712 && y > 192 && y < 832;
    if (inDoor) [r, g, b] = [30, 41, 59];
    const keyCircle = (x - 512) ** 2 + (y - 470) ** 2 < 62 ** 2;
    const keySlot = Math.abs(x - 512) < 24 + (y - 520) * 0.12 && y > 500 && y < 650;
    if (inDoor && (keyCircle || keySlot)) [r, g, b] = [251, 191, 36];
    px.set([r, g, b, 255], i);
  }
}

const crc = (buf: Uint8Array) => {
  let c = ~0;
  for (const byte of buf) {
    c ^= byte;
    for (let k = 0; k < 8; k++) c = (c >>> 1) ^ (0xedb88320 & -(c & 1));
  }
  return ~c >>> 0;
};
const chunk = (type: string, data: Uint8Array) => {
  const out = new Uint8Array(12 + data.length);
  const view = new DataView(out.buffer);
  view.setUint32(0, data.length);
  out.set(new TextEncoder().encode(type), 4);
  out.set(data, 8);
  view.setUint32(8 + data.length, crc(out.subarray(4, 8 + data.length)));
  return out;
};

const raw = new Uint8Array((SIZE * 4 + 1) * SIZE);
for (let y = 0; y < SIZE; y++) raw.set(px.subarray(y * SIZE * 4, (y + 1) * SIZE * 4), y * (SIZE * 4 + 1) + 1);
const header = new Uint8Array(13);
const hv = new DataView(header.buffer);
hv.setUint32(0, SIZE);
hv.setUint32(4, SIZE);
header.set([8, 6, 0, 0, 0], 8);

const png = Buffer.concat([
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  chunk("IHDR", header),
  chunk("IDAT", deflateSync(raw)),
  chunk("IEND", new Uint8Array(0)),
]);
await Bun.write("app-icon.png", png);
console.log(`app-icon.png ${png.length} bytes`);
