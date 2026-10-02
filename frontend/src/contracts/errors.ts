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

/**
 * Reduce an error to one fixed category, never a fragment of its text, which
 * may carry secrets. Scan failures are matched by their own wording first:
 * their help text mentions "network" and would otherwise read as a connection
 * problem. That wording is pinned by tests/fixtures/scan-error-categories.
 */
export function diagnosticError(error: unknown): string | null {
  if (!error) return null;
  const text = humanError(error);
  if (/doesn't appear to be running/i.test(text)) return 'game_not_running';
  if (/no accountId\/nonce pair found/i.test(text)) return 'credentials_not_found';
  if (/scan is already in progress/i.test(text)) return 'scan_busy';
  if (/inventory endpoint returned HTTP/i.test(text)) return 'endpoint_rejected';
  if (/permission|access (is )?denied|operation not permitted|OpenProcess failed/i.test(text)) return 'permission_denied';
  if (/out-of-range number|number out of range/i.test(text)) return 'numeric_out_of_range';
  if (/\b404\b|not found/i.test(text)) return 'not_found';
  if (/timed? out|timeout/i.test(text)) return 'timeout';
  if (/signature/i.test(text)) return 'signature_error';
  if (/network|connect|offline/i.test(text)) return 'connection_error';
  return 'unclassified_error';
}

/** The HTTP status an error names ("HTTP 403", "/v2/me returned 403"), never its reason phrase. */
export function diagnosticHttpStatus(error: unknown): number | null {
  if (!error) return null;
  const match = /\b(?:HTTP|returned)\s+([1-5]\d\d)\b/.exec(humanError(error));
  return match ? Number(match[1]) : null;
}

/** A native command's error code. Codes are fixed identifiers; anything else is not reported. */
export function diagnosticCode(error: unknown): string | null {
  if (!(error instanceof DesktopCmdError)) return null;
  return /^[a-z][a-z0-9_]{0,39}$/.test(error.code) ? error.code : 'unclassified_error';
}
