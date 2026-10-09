// Field shimmer: a moving light and lit edge rules drawn over a text field to
// show that it is waiting for input or has just received focus from a
// shortcut. It is a state signal, never ambient decoration; see
// docs/design-system.md, "Field shimmer".
//
// The geometry and timing follow the YoRHa input shader: a wide light that
// travels back and forth (14 s each way), a narrow sheen riding ahead of it
// that also brightens the edge rules, 4px scanlines, and sparse twinkling
// sparks on a drifting 96x28px tile grid.

export type ShimmerMode = 'off' | 'live' | 'settled';

export interface ShimmerParams {
  mode: ShimmerMode;
  /** Sparks are drawn only in the dark theme, and only where this is set. */
  sparks?: boolean;
  /** Increment to run one light pass across the field, then show `mode`. */
  pulse?: number;
}

export type ShimmerFrame =
  | { kind: 'off' }
  | { kind: 'settled' }
  | { kind: 'live'; center: number }
  | { kind: 'pulse'; center: number };

export const PULSE_MS = 1100;

/** The wide light's centre, as a fraction of the field width. */
export function sweepCenter(seconds: number): number {
  return 0.5 + 0.38 * Math.sin(seconds * 0.2244);
}

/** A single eased crossing from beyond the left edge to beyond the right. */
export function pulseCenter(ageMs: number): number {
  const p = Math.min(1, Math.max(0, ageMs / PULSE_MS));
  return -0.35 + 1.7 * (1 - (1 - p) ** 3);
}

export function resolveFrame(
  mode: ShimmerMode,
  seconds: number,
  pulseAgeMs: number | null,
  reducedMotion: boolean,
): ShimmerFrame {
  if (mode === 'off') return { kind: 'off' };
  if (reducedMotion) return { kind: 'settled' };
  if (pulseAgeMs !== null && pulseAgeMs >= 0 && pulseAgeMs < PULSE_MS) {
    return { kind: 'pulse', center: pulseCenter(pulseAgeMs) };
  }
  return mode === 'live' ? { kind: 'live', center: sweepCenter(seconds) } : { kind: 'settled' };
}

export const wave = (x: number, center: number): number => Math.exp(-(((x - center) / 0.25) ** 2));
export const sheen = (x: number, center: number): number => Math.exp(-(((x - center - 0.14) / 0.075) ** 2));

const TILE_W = 96;
const TILE_H = 28;
const DENSITY = 0.55;

function hash(n: number): number {
  const s = Math.sin(n * 127.1 + 311.7) * 43758.5453;
  return s - Math.floor(s);
}

export interface Spark { x: number; y: number; alpha: number; diamond: boolean }

/** Sparks visible in a w x h field at time `seconds`; deterministic in its inputs. */
export function sparksAt(w: number, h: number, seconds: number): Spark[] {
  const ox = seconds * 2;
  const oy = seconds * 0.45;
  const out: Spark[] = [];
  const x0 = Math.floor(-ox / TILE_W) - 1;
  const x1 = Math.floor((w - ox) / TILE_W) + 1;
  const y0 = Math.floor(-oy / TILE_H) - 1;
  const y1 = Math.floor((h - oy) / TILE_H) + 1;
  for (let cy = y0; cy <= y1; cy++) {
    for (let cx = x0; cx <= x1; cx++) {
      const seed = cx * 17 + cy * 131;
      if (hash(seed) > DENSITY) continue;
      const x = (cx + hash(seed + 1)) * TILE_W + ox;
      const y = (cy + hash(seed + 2)) * TILE_H + oy;
      if (x < 0 || x > w || y < 0 || y > h) continue;
      const alpha = (0.5 + 0.5 * Math.sin(seconds * 0.65 + hash(seed + 4) * Math.PI * 2)) ** 2;
      out.push({ x, y, alpha, diamond: hash(seed + 3) > 0.8 });
    }
  }
  return out;
}

// ---- rendering -------------------------------------------------------------

type Rgb = [number, number, number];

interface Instance {
  node: HTMLElement;
  canvas: HTMLCanvasElement;
  ctx: CanvasRenderingContext2D;
  params: ShimmerParams;
  pulseAt: number | null;
  visible: boolean;
  w: number;
  h: number;
}

const instances = new Set<Instance>();
let raf = 0;

const reducedQuery = (): MediaQueryList | null =>
  typeof window !== 'undefined' && typeof window.matchMedia === 'function'
    ? window.matchMedia('(prefers-reduced-motion: reduce)')
    : null;

function isDark(): boolean {
  return document.documentElement.dataset.mode === 'dark';
}

function toRgb(ctx: CanvasRenderingContext2D, color: string): Rgb {
  ctx.fillStyle = '#000';
  ctx.fillStyle = color.trim() || '#000';
  const v = String(ctx.fillStyle);
  if (v.startsWith('#') && v.length === 7) {
    const n = parseInt(v.slice(1), 16);
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
  }
  const m = v.match(/[\d.]+/g);
  return m ? [Number(m[0]), Number(m[1]), Number(m[2])] : [0, 0, 0];
}

const rgba = (c: Rgb, a: number): string =>
  `rgba(${c[0]},${c[1]},${c[2]},${Math.max(0, Math.min(1, a)).toFixed(3)})`;

function frameFor(inst: Instance, now: number): ShimmerFrame {
  const age = inst.pulseAt === null ? null : now - inst.pulseAt;
  return resolveFrame(inst.params.mode, (now / 1000) % 3600, age, reducedQuery()?.matches ?? false);
}

function draw(inst: Instance, now: number): ShimmerFrame {
  const { ctx, w, h } = inst;
  ctx.clearRect(0, 0, w, h);
  const frame = frameFor(inst, now);
  if (frame.kind === 'off' || w === 0 || h === 0) return frame;

  const dark = isDark();
  const style = getComputedStyle(inst.node);
  const glint = toRgb(ctx, style.getPropertyValue('--ink-bar'));
  const center = frame.kind === 'settled' ? null : frame.center;

  if (center !== null) {
    let waveAmt = dark ? 0.16 : 0.07;
    let sheenAmt = dark ? 0.12 : 0.06;
    if (frame.kind === 'pulse') { waveAmt *= 0.6; sheenAmt *= 1.6; }
    const g = ctx.createLinearGradient(0, 0, w, 0);
    for (let i = 0; i <= 32; i++) {
      const x = i / 32;
      g.addColorStop(x, rgba(glint, waveAmt * wave(x, center) + sheenAmt * sheen(x, center)));
    }
    ctx.fillStyle = g;
    ctx.fillRect(0, 0, w, h);
    ctx.fillStyle = rgba(glint, 0.025);
    for (let y = 1; y < h; y += 4) ctx.fillRect(0, y, w, 1);
  }

  if (frame.kind === 'live' && inst.params.sparks && dark) {
    const spark = toRgb(ctx, style.getPropertyValue('--fg'));
    for (const s of sparksAt(w, h, (now / 1000) % 3600)) {
      const halo = ctx.createRadialGradient(s.x, s.y, 0, s.x, s.y, 4.5);
      halo.addColorStop(0, rgba(spark, 0.22 * s.alpha));
      halo.addColorStop(1, rgba(spark, 0));
      ctx.fillStyle = halo;
      ctx.fillRect(s.x - 5, s.y - 5, 10, 10);
      ctx.fillStyle = rgba(spark, 0.85 * s.alpha);
      ctx.beginPath();
      if (s.diamond) {
        ctx.moveTo(s.x, s.y - 1.8);
        ctx.lineTo(s.x + 1.8, s.y);
        ctx.lineTo(s.x, s.y + 1.8);
        ctx.lineTo(s.x - 1.8, s.y);
        ctx.closePath();
      } else {
        ctx.arc(s.x, s.y, 1.1, 0, Math.PI * 2);
      }
      ctx.fill();
    }
  }

  // Edge rules stay lit in every visible state and brighten under the sheen.
  const base = dark ? 0.45 : 0.35;
  const boost = dark ? 0.55 : 0.4;
  const e = ctx.createLinearGradient(0, 0, w, 0);
  for (let i = 0; i <= 32; i++) {
    const x = i / 32;
    e.addColorStop(x, rgba(glint, base + (center === null ? 0 : boost * sheen(x, center))));
  }
  ctx.fillStyle = e;
  ctx.fillRect(0, 0, w, 1);
  ctx.fillRect(0, h - 1, w, 1);
  return frame;
}

function loop(now: number): void {
  raf = 0;
  if (document.hidden) return;
  let animating = false;
  for (const inst of instances) {
    if (!inst.visible) continue;
    const k = draw(inst, now).kind;
    if (k === 'live' || k === 'pulse') animating = true;
  }
  if (animating) raf = requestAnimationFrame(loop);
}

function kick(): void {
  if (!raf && typeof requestAnimationFrame === 'function') raf = requestAnimationFrame(loop);
}

function redrawAll(): void {
  const now = performance.now();
  for (const inst of instances) draw(inst, now);
  kick();
}

let themeObserver: MutationObserver | null = null;

function watchEnvironment(): void {
  if (themeObserver || typeof MutationObserver === 'undefined') return;
  themeObserver = new MutationObserver(redrawAll);
  themeObserver.observe(document.documentElement, { attributes: true, attributeFilter: ['data-mode', 'data-look'] });
  reducedQuery()?.addEventListener('change', redrawAll);
  document.addEventListener('visibilitychange', kick);
}

function unwatchEnvironment(): void {
  if (!themeObserver || instances.size > 0) return;
  themeObserver.disconnect();
  themeObserver = null;
  reducedQuery()?.removeEventListener('change', redrawAll);
  document.removeEventListener('visibilitychange', kick);
}

/**
 * Draws the shimmer over the element's box. Put it on a positioned wrapper
 * around the input (`.shimmer-field`); the canvas ignores the pointer and is
 * hidden from assistive technology. Inert where canvas or observers are
 * unavailable.
 */
export function shimmer(node: HTMLElement, initial: ShimmerParams) {
  let inst: Instance | null = null;
  let resize: ResizeObserver | null = null;
  let intersect: IntersectionObserver | null = null;
  let pending = initial;
  let lastPulse = initial.pulse ?? 0;
  // Kept outside the instance: a pulse can arrive just before the focus that turns the field on.
  let pulseAt: number | null = null;

  function measure(): void {
    if (!inst) return;
    const dpr = window.devicePixelRatio || 1;
    inst.w = node.clientWidth;
    inst.h = node.clientHeight;
    inst.canvas.width = Math.round(inst.w * dpr);
    inst.canvas.height = Math.round(inst.h * dpr);
    inst.ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    draw(inst, performance.now());
  }

  // The canvas is created on first use, so a field that never shimmers costs nothing.
  function ensure(): Instance | null {
    if (inst) return inst;
    if (typeof ResizeObserver === 'undefined') return null;
    const canvas = document.createElement('canvas');
    canvas.setAttribute('aria-hidden', 'true');
    let ctx: CanvasRenderingContext2D | null = null;
    try { ctx = canvas.getContext('2d'); } catch { ctx = null; }
    if (!ctx) return null;
    node.append(canvas);
    inst = { node, canvas, ctx, params: pending, pulseAt, visible: true, w: 0, h: 0 };
    instances.add(inst);
    watchEnvironment();
    resize = new ResizeObserver(measure);
    resize.observe(node);
    if (typeof IntersectionObserver !== 'undefined') {
      intersect = new IntersectionObserver(([entry]) => {
        if (!inst || !entry) return;
        inst.visible = entry.isIntersecting;
        kick();
      });
      intersect.observe(node);
    }
    measure();
    return inst;
  }

  function apply(params: ShimmerParams): void {
    pending = params;
    const pulsed = (params.pulse ?? 0) !== lastPulse;
    lastPulse = params.pulse ?? 0;
    if (pulsed) pulseAt = performance.now();
    if (!inst && params.mode === 'off') return;
    const i = ensure();
    if (!i) return;
    i.params = params;
    i.pulseAt = pulseAt;
    draw(i, performance.now());
    kick();
  }

  apply(initial);

  return {
    update: apply,
    destroy(): void {
      resize?.disconnect();
      intersect?.disconnect();
      if (inst) {
        instances.delete(inst);
        inst.canvas.remove();
      }
      unwatchEnvironment();
    },
  };
}
