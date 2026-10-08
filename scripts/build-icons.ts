// Generates the TennoWorth icon set from one drawing of the hex-lattice mark,
// so every size and colour comes from the same source.
//
//   bun scripts/build-icons.ts            rewrite the committed SVGs
//   bun scripts/build-icons.ts --check    fail if a committed SVG is stale
//   bun scripts/build-icons.ts --raster   rewrite the SVGs, then the desktop
//                                         PNG/ICO/ICNS files (needs rsvg-convert
//                                         and ImageMagick 7)
//
// The mark is transparent. It has two drawings: the detailed lattice for 48px
// and up, and a compact one for 32px and below, where the inner hexagon outline
// and centre dot would blur together - heavier strokes and a solid centre.
//
// Colours: classic blue is the mark's own colour and the default everywhere
// (favicon, installed icons, launcher). The desktop App icon setting can also
// draw it in ink or rag, which are the light and dark themes' --fg tokens, read
// from frontend/src/app.css so the setting cannot drift from the palette. Rust
// embeds the ink and rag rasters (shell/app_icon.rs); the setting's previews
// are the public/app-icon/*.svg files, drawn on both themes' --bg.
//
// --check runs before every frontend build, like sync-csp.ts. The PNG/ICO/ICNS
// files are not checked there (CI has no rasteriser): run --raster whenever the
// SVGs change and commit the results with them.

import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'fs';
import { spawnSync } from 'child_process';
import { tmpdir } from 'os';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const DESKTOP_ICONS = 'rust/tennoworth-desktop/icons';

type Mode = 'light' | 'dark';

const BLOCKS: Record<Mode, string> = {
  light: 'html[data-look="yorha"] {',
  dark: 'html[data-look="yorha"][data-mode="dark"] {',
};

function token(css: string, mode: Mode, name: string): string {
  const start = css.indexOf(BLOCKS[mode]);
  if (start < 0) throw new Error(`build-icons: no ${mode} token block in app.css`);
  const block = css.slice(start, css.indexOf('}', start));
  const m = block.match(new RegExp(`--${name}:\\s*(#[0-9A-Fa-f]{6})\\s*;`));
  if (!m) throw new Error(`build-icons: --${name} is not a hex colour in the ${mode} block`);
  return m[1];
}

const BLUE = '#4E9EEA';

const OUTLINE =
  'M22.5 15.546 27.25 7.318H36.75L41.5 15.546H51L55.75 23.773 51 32l4.75 8.227L51 48.454h-9.5l-4.75 8.228h-9.5l-4.75-8.228H13l-4.75-8.227L13 32 8.25 23.773 13 15.546Z';
const CORE = 'M41.5 32 36.75 40.227H27.25L22.5 32l4.75-8.227h9.5Z';
const SPOKES = 'M41.5 15.546l-4.75 8.227m-9.5 0-4.75-8.227M51 32h-9.5m0 16.454-4.75-8.227m-14.25 8.227 4.75-8.227M13 32h9.5';
const DOT = 'M32 28.6 34.944 30.3v3.4L32 35.4l-2.944-1.7v-3.4Z';

type Drawing = 'detail' | 'compact';

function mark(drawing: Drawing, colour: string): string {
  if (drawing === 'compact') {
    return [
      `<path d="${OUTLINE}" fill="none" stroke="${colour}" stroke-width="5" stroke-linejoin="bevel"/>`,
      `<path d="${SPOKES}" fill="none" stroke="${colour}" stroke-width="2.5" stroke-linecap="square"/>`,
      `<path d="${CORE}" fill="${colour}"/>`,
    ].join('\n  ');
  }
  return [
    `<path d="${OUTLINE}" fill="none" stroke="${colour}" stroke-width="2.7" stroke-linejoin="bevel"/>`,
    `<path d="${CORE}${SPOKES}" fill="none" stroke="${colour}" stroke-width="1.55" stroke-linecap="square" stroke-linejoin="miter"/>`,
    `<path d="${DOT}" fill="${colour}"/>`,
  ].join('\n  ');
}

const svg = (drawing: Drawing, colour: string) =>
  `<svg xmlns="http://www.w3.org/2000/svg" viewBox="4 4 56 56" role="img" aria-label="TennoWorth">
  ${mark(drawing, colour)}
</svg>
`;

// A settings preview: the compact mark on the light theme's ground, then on the
// dark theme's, side by side.
function preview(css: string, onLight: string, onDark: string): string {
  const tile = (x: number, mode: Mode, colour: string) =>
    `<rect x="${x}" width="64" height="64" fill="${token(css, mode, 'bg')}"/>
  <svg x="${x + 12}" y="12" width="40" height="40" viewBox="4 4 56 56">
  ${mark('compact', colour)}
  </svg>`;
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 128 64">
  ${tile(0, 'light', onLight)}
  ${tile(64, 'dark', onDark)}
</svg>
`;
}

function renderSvgs(css: string): Record<string, string> {
  const ink = token(css, 'light', 'fg');
  const rag = token(css, 'dark', 'fg');
  return {
    'frontend/public/favicon.svg': svg('compact', BLUE),
    'frontend/public/app-icon/blue.svg': preview(css, BLUE, BLUE),
    'frontend/public/app-icon/match.svg': preview(css, ink, rag),
    'frontend/public/app-icon/ink.svg': preview(css, ink, ink),
    'frontend/public/app-icon/rag.svg': preview(css, rag, rag),
    [`${DESKTOP_ICONS}/icon.svg`]: svg('detail', BLUE),
    [`${DESKTOP_ICONS}/icon-small.svg`]: svg('compact', BLUE),
    [`${DESKTOP_ICONS}/app-icon/ink.svg`]: svg('compact', ink),
    [`${DESKTOP_ICONS}/app-icon/rag.svg`]: svg('compact', rag),
  };
}

// Pixel sizes at or below this use the compact drawing.
const COMPACT_MAX = 32;

function run(cmd: string, args: string[]) {
  const r = spawnSync(cmd, args);
  if (r.error || r.status !== 0) throw new Error(`build-icons: ${cmd} failed: ${r.error ?? r.stderr}`);
}

// tauri-codegen rejects any bundle PNG that is not RGBA and rsvg-convert may
// write RGB, so ImageMagick re-encodes it (without the timestamp chunks, so an
// unchanged source rasterises to identical bytes).
function rasterise(dir: string, size: number, svgPath?: string): Buffer {
  const source = svgPath ?? (size <= COMPACT_MAX ? 'icon-small.svg' : 'icon.svg');
  const name = `${source.replace(/\W/g, '-')}-${size}`;
  const raw = join(dir, `${name}-raw.png`);
  const out = join(dir, `${name}.png`);
  run('rsvg-convert', ['-w', String(size), '-h', String(size), '-o', raw, join(ROOT, DESKTOP_ICONS, source)]);
  run('magick', [raw, '-strip', '-define', 'png:exclude-chunk=date,time', `PNG32:${out}`]);
  return readFileSync(out);
}

// PNG-compressed entries only, as the previous set had. The FIRST entry is the
// Windows default window (and tray) icon in tauri-codegen, so 32px leads.
function ico(images: [number, Buffer][]): Buffer {
  const head = Buffer.alloc(6 + 16 * images.length);
  head.writeUInt16LE(0, 0);
  head.writeUInt16LE(1, 2);
  head.writeUInt16LE(images.length, 4);
  let offset = head.length;
  images.forEach(([size, png], i) => {
    const e = 6 + 16 * i;
    head.writeUInt8(size >= 256 ? 0 : size, e);
    head.writeUInt8(size >= 256 ? 0 : size, e + 1);
    head.writeUInt16LE(1, e + 4);
    head.writeUInt16LE(32, e + 6);
    head.writeUInt32LE(png.length, e + 8);
    head.writeUInt32LE(offset, e + 12);
    offset += png.length;
  });
  return Buffer.concat([head, ...images.map(([, png]) => png)]);
}

function icns(entries: [string, Buffer][]): Buffer {
  const parts = entries.map(([type, png]) => {
    const h = Buffer.alloc(8);
    h.write(type, 0, 'ascii');
    h.writeUInt32BE(png.length + 8, 4);
    return Buffer.concat([h, png]);
  });
  const head = Buffer.alloc(8);
  head.write('icns', 0, 'ascii');
  head.writeUInt32BE(8 + parts.reduce((n, p) => n + p.length, 0), 4);
  return Buffer.concat([head, ...parts]);
}

function writeRasters() {
  const dir = mkdtempSync(join(tmpdir(), 'tennoworth-icons-'));
  try {
    const png = new Map<number, Buffer>();
    for (const s of [16, 24, 32, 48, 64, 128, 256, 512, 1024]) png.set(s, rasterise(dir, s));
    const at = (s: number) => png.get(s)!;
    const out = (name: string, data: Buffer) => writeFileSync(join(ROOT, DESKTOP_ICONS, name), data);
    out('32x32.png', at(32));
    out('64x64.png', at(64));
    out('128x128.png', at(128));
    out('128x128@2x.png', at(256));
    out('icon.png', at(512));
    out('icon.ico', ico([32, 16, 24, 48, 64, 256].map((s) => [s, at(s)])));
    out(
      'icon.icns',
      icns([
        ['icp4', at(16)], ['icp5', at(32)], ['ic11', at(32)], ['ic12', at(64)],
        ['ic07', at(128)], ['ic13', at(256)], ['ic08', at(256)], ['ic14', at(512)],
        ['ic09', at(512)], ['ic10', at(1024)],
      ]),
    );
    // The App icon setting's alternatives: 32px for the tray, 64px for the
    // window. Classic blue uses the bundle's own files.
    for (const colour of ['ink', 'rag']) {
      for (const size of [32, 64]) {
        out(`app-icon/${colour}-${size}.png`, rasterise(dir, size, `app-icon/${colour}.svg`));
      }
    }
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

if (import.meta.main) {
  const args = process.argv.slice(2);
  const expected = renderSvgs(readFileSync(join(ROOT, 'frontend/src/app.css'), 'utf8'));
  if (args.includes('--check')) {
    const stale = Object.entries(expected).filter(([path, body]) => {
      try {
        return readFileSync(join(ROOT, path), 'utf8') !== body;
      } catch {
        return true;
      }
    });
    if (stale.length) {
      console.error('build-icons: stale icon SVGs (run `bun scripts/build-icons.ts --raster`):');
      for (const [path] of stale) console.error(`  ${path}`);
      process.exit(1);
    }
    console.log('build-icons: icon SVGs are current');
  } else {
    for (const [path, body] of Object.entries(expected)) {
      mkdirSync(dirname(join(ROOT, path)), { recursive: true });
      writeFileSync(join(ROOT, path), body);
    }
    if (args.includes('--raster')) writeRasters();
    console.log(`build-icons: wrote ${Object.keys(expected).length} SVGs${args.includes('--raster') ? ' and the desktop rasters' : ''}`);
  }
}
