import type { DomainRequest, DomainResponse } from '../contracts/generated/domain';

interface DomainExports {
  memory: WebAssembly.Memory;
  domain_alloc(len: number): number;
  domain_free(ptr: number, len: number): void;
  domain_evaluate(ptr: number, len: number): number;
  domain_result_len(): number;
}

export const MISSING_DOMAIN_WASM = 'The desktop preview runs the native calculations from WebAssembly. Build them with `bun run build:domain-wasm` in frontend/ (Rust with the wasm32-unknown-unknown target).';

/**
 * The desktop's `evaluate_domain`, run from the wasm32 build of
 * rust/market-domain-wasm, so the preview shows native results. The evaluator
 * returns the native response and throws the native error text - a string, as
 * the Tauri command rejects with.
 */
export async function loadDomainEvaluator(
  fetchModule: () => Promise<Response> = () => fetch(new URL('./generated/market-domain.wasm', import.meta.url)),
): Promise<(request: DomainRequest) => DomainResponse> {
  const response = await fetchModule().catch(() => null);
  const bytes = response?.ok ? new Uint8Array(await response.arrayBuffer()) : null;
  // The dev server answers a missing file with index.html and a 200, so check
  // for the wasm magic number rather than trusting the status.
  if (!bytes || String.fromCharCode(...bytes.subarray(0, 4)) !== '\0asm') throw new Error(MISSING_DOMAIN_WASM);
  const { instance } = await WebAssembly.instantiate(bytes);
  const wasm = instance.exports as unknown as DomainExports;
  const encoder = new TextEncoder();
  const decoder = new TextDecoder();
  return request => {
    const bytes = encoder.encode(JSON.stringify(request));
    const input = wasm.domain_alloc(bytes.length);
    new Uint8Array(wasm.memory.buffer, input, bytes.length).set(bytes);
    const output = wasm.domain_evaluate(input, bytes.length);
    const length = wasm.domain_result_len();
    // Read through a fresh view: evaluation can grow memory and detach the old buffer.
    const text = decoder.decode(new Uint8Array(wasm.memory.buffer, output, length));
    wasm.domain_free(input, bytes.length);
    wasm.domain_free(output, length);
    const answer = JSON.parse(text) as { ok: DomainResponse } | { error: string };
    if ('error' in answer) throw answer.error;
    return answer.ok;
  };
}
