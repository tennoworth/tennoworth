import type { CmdError } from './generated/desktop';

export class DesktopCmdError extends Error implements CmdError {
  code: string;
  constructor(code: string, message: string) {
    super(message);
    this.name = 'DesktopCmdError';
    this.code = code;
  }
}

export function humanError(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export function diagnosticError(error: unknown): string | null {
  if (!error) return null;
  const text = error instanceof Error ? error.message : String(error);
  // Return fixed categories, never fragments of errors that may contain secrets.
  if (/out-of-range number|number out of range/i.test(text)) return 'numeric_out_of_range';
  if (/404|not found/i.test(text)) return 'not_found';
  if (/permission|access denied|operation not permitted/i.test(text)) return 'permission_denied';
  if (/timed? out|timeout/i.test(text)) return 'timeout';
  if (/signature/i.test(text)) return 'signature_error';
  if (/network|connect|offline/i.test(text)) return 'connection_error';
  return 'unclassified_error';
}
