// The page side of rust/market-domain-web: the view rules of market-domain,
// compiled to wasm32 and instantiated once before anything renders, so the
// rules are plain synchronous calls inside $derived. main.ts fetches the
// module and awaits startNative() before any surface mounts; the prerender and
// the unit suite hand its bytes to installNative().
//
// Two kinds of export: rules over plain numbers are wasm functions of numbers;
// rules over snapshot objects go through `call`, a JSON request in wasm
// memory (contracts/generated/web.ts types both ends).
import type { WebRequest, WebResponse } from '../contracts/generated/web';

interface WebExports {
  memory: WebAssembly.Memory;
  web_alloc(len: number): number;
  web_free(ptr: number, len: number): void;
  web_call(ptr: number, len: number): number;
  web_result_len(): number;
  web_sellable_qty(count: number, reserve: number, leveled: number): number;
  web_spare_qty(count: number, keptLvl: number, leveled: number): number;
  web_clearing_price(vol: number, lowSell: number, avg: number, medianNow: number, median90d: number): number;
  web_weekend_nudge(weekday: number): number;
}

let exports: WebExports | null = null;

export const MISSING_NATIVE = 'The view rules are a wasm32 build of rust/market-domain-web. Build it with `bun run build:web-domain` in frontend/ (Rust with the wasm32-unknown-unknown target).';

export function installNative(bytes: BufferSource): void {
  exports = new WebAssembly.Instance(new WebAssembly.Module(bytes)).exports as unknown as WebExports;
}

/** Compile off the main thread's synchronous path; main.ts awaits this. */
export async function startNative(bytes: BufferSource): Promise<void> {
  const { instance } = await WebAssembly.instantiate(bytes);
  exports = instance.exports as unknown as WebExports;
}

export function native(): WebExports {
  if (!exports) throw new Error(`View rules used before the module was instantiated. ${MISSING_NATIVE}`);
  return exports;
}

type ResultOf<Op extends WebRequest['op']> = Extract<WebResponse, { op: Op }>['result'];

/** Answer one object-shaped rule; throws the native error text. */
export function call<R extends WebRequest>(request: R): ResultOf<R['op']> {
  const wasm = native();
  const bytes = new TextEncoder().encode(JSON.stringify(request));
  const input = wasm.web_alloc(bytes.length);
  new Uint8Array(wasm.memory.buffer, input, bytes.length).set(bytes);
  const output = wasm.web_call(input, bytes.length);
  const length = wasm.web_result_len();
  // A fresh view: the call can grow memory and detach the old buffer.
  const text = new TextDecoder().decode(new Uint8Array(wasm.memory.buffer, output, length));
  wasm.web_free(input, bytes.length);
  wasm.web_free(output, length);
  const answer = JSON.parse(text) as { ok: WebResponse } | { error: string };
  if ('error' in answer) throw new Error(answer.error);
  return answer.ok.result as ResultOf<R['op']>;
}
