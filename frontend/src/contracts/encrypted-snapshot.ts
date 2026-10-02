

export interface EncryptedBlob {
  format: string;
  created: string;
  kdf: { name: string; hash: string; iterations: number; salt: string };
  cipher: { name: string; iv: string };
  ciphertext: string;
}

// Native login counts Unicode scalar values, so astral characters count once.
export function hasMinimumPassphraseLength(passphrase: string): boolean {
  return [...passphrase].length >= 12;
}
