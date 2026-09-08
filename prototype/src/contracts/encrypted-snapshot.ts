

export interface EncryptedBlob {
  format: string;
  created: string;
  kdf: { name: string; hash: string; iterations: number; salt: string };
  cipher: { name: string; iv: string };
  ciphertext: string;
}
