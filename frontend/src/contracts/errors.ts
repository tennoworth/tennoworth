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
