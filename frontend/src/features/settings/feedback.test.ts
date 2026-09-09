import { describe, expect, it } from 'vitest';
import { feedbackLink, feedbackSnapshot, type FeedbackState } from './feedback';
import { diagnosticError } from '../../contracts/errors';
const state: FeedbackState = { capturedAt: '2026-09-09T00:00:00Z', build: 'abc1234', platform: 'linux', view: 'landing', phase: 'error', scanning: false, scanError: 'The calculation contains an out-of-range number.', marketLoaded: true, marketError: null, theme: 'dark', width: 1280, height: 720 };

describe('feedback diagnostics', () => {
  it('prefills the existing GitHub form with a reviewable snapshot', () => {
    const snapshot = feedbackSnapshot(state, null, null);
    const link = feedbackLink(snapshot, true);
    const url = new URL(link.url);
    expect(url.origin).toBe('https://github.com');
    expect(url.searchParams.get('template')).toBe('bug-report.yml');
    expect(url.searchParams.get('operating-system')).toBe('Linux');
    expect(url.searchParams.get('attachments')).toContain(JSON.stringify(snapshot, null, 2));
    expect(snapshot.inventory.error).toBe('numeric_out_of_range');
    expect(snapshot.update.status).toBe('unavailable');
    expect(link.needsAttachment).toBe(false);
  });
  it('excludes diagnostics when unchecked', () => {
    const url = new URL(feedbackLink(feedbackSnapshot(state, null, null), false).url);
    expect(url.searchParams.has('attachments')).toBe(false);
  });
  it('does not serialize arbitrary errors or extra account/inventory properties', () => {
    const snapshot = feedbackSnapshot({ ...state, scanError: 'token secret-user-value at /home/private-user/data', marketError: new Error('timeout token secret-user-value'), accountId: 'secret-user-value', inventory: ['private item'] } as FeedbackState, null, null);
    const text = JSON.stringify(snapshot);
    expect(text).not.toContain('secret-user-value');
    expect(text).not.toContain('private');
    expect(snapshot.inventory.error).toBe('unclassified_error');
    expect(snapshot.market.error).toBe('timeout');
  });
  it('uses a file fallback when the encoded URL is too large', () => {
    const snapshot = feedbackSnapshot({ ...state, view: 'x'.repeat(7000) }, null, null);
    const link = feedbackLink(snapshot, true);
    expect(link.needsAttachment).toBe(true);
    expect(link.url.length).toBeLessThan(6000);
    expect(new URL(link.url).searchParams.has('attachments')).toBe(false);
  });
  it.each([['404 Not Found https://private', 'not_found'], ['Permission denied /home/user', 'permission_denied'], ['bad signature token', 'signature_error'], ['connection failed host', 'connection_error']])('classifies %s without copying error text', (error, category) => {
    expect(diagnosticError(error)).toBe(category);
  });
});
