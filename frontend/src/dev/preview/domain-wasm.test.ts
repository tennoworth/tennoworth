import { expect, it } from 'vitest';
import { loadDomainEvaluator, MISSING_DOMAIN_WASM } from './domain-wasm';

// The loaded module's behavior is the native crate's; its tests
// (rust/market-domain-wasm) and the browser suite run it. What is TypeScript's
// own is saying how to build it when it is absent.
it('names the build step when the module is missing, not a wasm compile error', async () => {
  const devServerFallback = () => Promise.resolve(new Response('<!doctype html><html></html>', { status: 200 }));
  await expect(loadDomainEvaluator(devServerFallback)).rejects.toThrow(MISSING_DOMAIN_WASM);
  await expect(loadDomainEvaluator(() => Promise.resolve(new Response('', { status: 404 })))).rejects.toThrow(MISSING_DOMAIN_WASM);
  await expect(loadDomainEvaluator(() => Promise.reject(new Error('offline')))).rejects.toThrow(MISSING_DOMAIN_WASM);
});
