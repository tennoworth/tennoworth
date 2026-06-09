// Passphrase-derived AES-GCM, via WebCrypto. Used for the "export / re-import
// on another device" flow that replaces the auth feature we deliberately
// don't have. Pattern stolen from Standard Notes / Path of Building.
//
// Format version 1:
//   {
//     format:    "wfminv-encrypted-v1",
//     created:   "<ISO timestamp>",
//     kdf:       { name: "PBKDF2", hash: "SHA-256", iterations: 600000, salt: <b64> },
//     cipher:    { name: "AES-GCM", iv: <b64> },
//     ciphertext: <b64>
//   }

const FORMAT = 'wfminv-encrypted-v1';
const KDF_ITERATIONS = 600_000;            // OWASP 2023 recommendation for SHA-256
const KEY_LEN_BITS = 256;
const SALT_BYTES = 16;
const IV_BYTES = 12;

export interface EncryptedBlob {
  format: string;
  created: string;
  kdf: { name: string; hash: string; iterations: number; salt: string };
  cipher: { name: string; iv: string };
  ciphertext: string;
}

function toB64(buf: ArrayBuffer | Uint8Array): string {
  // Works for both ArrayBuffer and TypedArray.
  const bytes = buf instanceof Uint8Array ? buf : new Uint8Array(buf);
  let bin = '';
  for (let i = 0; i < bytes.length; i++) bin += String.fromCharCode(bytes[i]);
  return btoa(bin);
}

function fromB64(s: string): Uint8Array {
  const bin = atob(s);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

async function deriveKey(passphrase: string, salt: Uint8Array): Promise<CryptoKey> {
  const baseKey = await crypto.subtle.importKey(
    'raw',
    new TextEncoder().encode(passphrase),
    'PBKDF2',
    false,
    ['deriveKey'],
  );
  // BufferSource cast: TS 6 narrowed Uint8Array's buffer to ArrayBufferLike,
  // but WebCrypto's deriveKey signature wants ArrayBufferView<ArrayBuffer>
  // (excluding SharedArrayBuffer). Our salt is always a fresh
  // crypto.getRandomValues, never shared — safe to cast.
  return crypto.subtle.deriveKey(
    { name: 'PBKDF2', salt: salt as BufferSource, iterations: KDF_ITERATIONS, hash: 'SHA-256' },
    baseKey,
    { name: 'AES-GCM', length: KEY_LEN_BITS },
    false,
    ['encrypt', 'decrypt'],
  );
}

export async function encryptPayload(payload: unknown, passphrase: string): Promise<EncryptedBlob> {
  if (!passphrase || passphrase.length < 4) {
    throw new Error('Passphrase must be at least 4 characters.');
  }
  const salt = crypto.getRandomValues(new Uint8Array(SALT_BYTES));
  const iv = crypto.getRandomValues(new Uint8Array(IV_BYTES));
  const key = await deriveKey(passphrase, salt);
  const plaintext = new TextEncoder().encode(JSON.stringify(payload));
  const ciphertext = await crypto.subtle.encrypt(
    { name: 'AES-GCM', iv },
    key,
    plaintext,
  );
  return {
    format: FORMAT,
    created: new Date().toISOString(),
    kdf: {
      name: 'PBKDF2',
      hash: 'SHA-256',
      iterations: KDF_ITERATIONS,
      salt: toB64(salt),
    },
    cipher: { name: 'AES-GCM', iv: toB64(iv) },
    ciphertext: toB64(ciphertext),
  };
}

export async function decryptPayload(blob: EncryptedBlob, passphrase: string): Promise<unknown> {
  if (!isEncrypted(blob)) {
    throw new Error('Not a wfminv encrypted snapshot.');
  }
  if (blob.kdf?.name !== 'PBKDF2' || blob.cipher?.name !== 'AES-GCM') {
    throw new Error('Unsupported crypto parameters in snapshot.');
  }
  const salt = fromB64(blob.kdf.salt);
  const iv = fromB64(blob.cipher.iv);
  const ciphertext = fromB64(blob.ciphertext);
  const key = await deriveKey(passphrase, salt);
  let plaintext;
  try {
    plaintext = await crypto.subtle.decrypt({ name: 'AES-GCM', iv: iv as BufferSource }, key, ciphertext as BufferSource);
  } catch {
    // AES-GCM auth failure is the standard "wrong key" signal.
    throw new Error('Wrong passphrase, or the file was modified.');
  }
  try {
    return JSON.parse(new TextDecoder().decode(plaintext));
  } catch {
    throw new Error('Decrypted but contents are not valid JSON.');
  }
}

export function isEncrypted(blob: unknown): blob is EncryptedBlob {
  return !!(blob && typeof blob === 'object' && (blob as { format?: unknown }).format === FORMAT);
}
