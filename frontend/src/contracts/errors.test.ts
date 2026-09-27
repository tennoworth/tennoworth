import { describe, expect, it } from 'vitest';
import fixture from '../../../tests/fixtures/scan-error-categories/cases.json';
import { DesktopCmdError, diagnosticCode, diagnosticError, diagnosticHttpStatus } from './errors';

// The Rust side of this gate (wfm-core acquisition::message_fixture_tests)
// proves the scan still produces each message; this proves how it is reported.
describe('scan failure categories', () => {
  it.each(fixture.cases.map(c => [c.source, c] as const))('%s', (_source, c) => {
    expect(diagnosticError(c.message)).toBe(c.category);
    expect(diagnosticHttpStatus(c.message)).toBe(c.httpStatus);
  });

  it('keeps the category when the scan context wraps the message', () => {
    const credentials = fixture.cases.find(c => c.source === 'no_credentials')!;
    expect(diagnosticError(`memory scan failed: ${credentials.message}`)).toBe('credentials_not_found');
  });
});

describe('diagnostic classification', () => {
  it.each([
    ['Parsed 1404 items', 'unclassified_error'],
    ['404 Not Found https://example.invalid/latest.json', 'not_found'],
    ['bad signature token', 'signature_error'],
    ['connection failed host', 'connection_error'],
  ])('%s is %s', (text, category) => {
    expect(diagnosticError(text)).toBe(category);
  });

  it('reads a status from either wording and never from other numbers', () => {
    expect(diagnosticHttpStatus('/v2/me returned 403 Forbidden')).toBe(403);
    expect(diagnosticHttpStatus('Inventory endpoint returned HTTP 502 Bad Gateway (0 bytes).')).toBe(502);
    expect(diagnosticHttpStatus('Parsed 1404 items')).toBeNull();
    expect(diagnosticHttpStatus('returned 4031 rows')).toBeNull();
  });

  it('reports a native error code, not its message', () => {
    expect(diagnosticCode(new DesktopCmdError('signin_window_failed', 'The page crashed at /home/user'))).toBe('signin_window_failed');
    expect(diagnosticCode(new DesktopCmdError('Not A Code', 'x'))).toBe('unclassified_error');
    expect(diagnosticCode(new Error('plain'))).toBeNull();
    expect(diagnosticCode('text')).toBeNull();
  });
});
