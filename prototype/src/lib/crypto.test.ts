// @ts-nocheck — vitest runs these as JS-style fixtures; full TS shapes here would be busy-work without catching real bugs.
import { describe, it, expect, beforeAll } from 'vitest';
import { webcrypto } from 'node:crypto';

// jsdom doesn't ship WebCrypto by default; bolt the Node implementation on
// before the module under test reads `crypto.subtle`.
beforeAll(() => {
  if (!globalThis.crypto?.subtle) {
    globalThis.crypto = webcrypto;
  }
});

import { encryptPayload, decryptPayload, isEncrypted } from './crypto.js';

const SAMPLE = {
  invName: 'inventory.json',
  owned: [
    ['axi_k2_relic', { count: 7, name: 'Axi K2 Relic', type: 'Relics' }],
    ['vitality',     { count: 51, name: 'Vitality',    type: 'Mods' }],
  ],
};

describe('encryptPayload / decryptPayload (round-trip)', () => {
  it('round-trips a payload through encrypt + decrypt with the right passphrase', async () => {
    const blob = await encryptPayload(SAMPLE, 'correct horse battery staple');
    expect(isEncrypted(blob)).toBe(true);
    expect(blob.format).toBe('wfminv-encrypted-v1');
    expect(blob.kdf.iterations).toBeGreaterThanOrEqual(100000);
    const back = await decryptPayload(blob, 'correct horse battery staple');
    expect(back).toEqual(SAMPLE);
  });

  it('produces a different ciphertext each call (fresh salt + IV)', async () => {
    const a = await encryptPayload(SAMPLE, 'samepass');
    const b = await encryptPayload(SAMPLE, 'samepass');
    expect(a.ciphertext).not.toBe(b.ciphertext);
    expect(a.kdf.salt).not.toBe(b.kdf.salt);
    expect(a.cipher.iv).not.toBe(b.cipher.iv);
  });
}, 60_000);

describe('encryptPayload — guardrails', () => {
  it('rejects too-short passphrases', async () => {
    await expect(encryptPayload(SAMPLE, '')).rejects.toThrow(/passphrase/i);
    await expect(encryptPayload(SAMPLE, 'ab')).rejects.toThrow(/passphrase/i);
  });
});

describe('decryptPayload — failure modes', () => {
  it('rejects a wrong passphrase with a useful error', async () => {
    const blob = await encryptPayload(SAMPLE, 'right');
    await expect(decryptPayload(blob, 'wrong')).rejects.toThrow(/passphrase|modified/i);
  }, 60_000);

  it('detects tampering with the ciphertext', async () => {
    const blob = await encryptPayload(SAMPLE, 'right');
    // Flip a base64 character — likely produces a different valid base64 byte
    // but invalid GCM tag.
    blob.ciphertext = blob.ciphertext.slice(0, -2) + 'AB';
    await expect(decryptPayload(blob, 'right')).rejects.toThrow();
  }, 60_000);

  it('refuses to read something that is not our format', async () => {
    await expect(decryptPayload({ hello: 'world' }, 'x')).rejects.toThrow(/wfminv encrypted snapshot/);
    await expect(decryptPayload(null, 'x')).rejects.toThrow();
  });

  it('isEncrypted is false for non-format objects', () => {
    expect(isEncrypted({ format: 'something-else' })).toBe(false);
    expect(isEncrypted(null)).toBe(false);
    expect(isEncrypted('a string')).toBe(false);
  });
});
