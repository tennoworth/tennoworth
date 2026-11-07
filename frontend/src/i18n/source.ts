// How an English message is declared. See en.ts.

export interface Source {
  text: string;
  note: string;
  /** Characters available where space is tight; the check reports longer translations. */
  max?: number;
}

export const m = (text: string, note: string, max?: number): Source => ({ text, note, max });
