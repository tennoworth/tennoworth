import { describe, expect, it } from 'vitest';
import { bugReportUrl, feedbackLink, feedbackProblems, feedbackSnapshot, type FeedbackState } from './feedback';
import { DesktopCmdError } from '../../contracts/errors';
import type { UpdateStatus } from '../../contracts/update';
import { readFileSync } from 'node:fs';

// Vite refuses to serve dot-directories, so the form is read from disk,
// relative to frontend/ where the suite runs.
const bugForm = readFileSync('../.github/ISSUE_TEMPLATE/bug-report.yml', 'utf8');

const state: FeedbackState = {
  capturedAt: '2026-09-09T00:00:00Z', build: 'abc1234', appVersion: '0.8.0', platform: 'linux', view: 'landing',
  phase: 'error', scanning: false, scanError: 'The calculation contains an out-of-range number.',
  autoScan: null, wfmSession: 'logged_out', wfmError: null,
  marketLoaded: true, marketError: null, theme: 'dark', width: 1280, height: 720,
};

describe('feedback diagnostics', () => {
  it('prefills the existing GitHub form with a reviewable snapshot', () => {
    const snapshot = feedbackSnapshot(state, null, null);
    const link = feedbackLink(snapshot, true);
    const url = new URL(link.url);
    expect(url.origin).toBe('https://github.com');
    expect(url.searchParams.get('template')).toBe('bug-report.yml');
    expect(url.searchParams.get('surface')).toBe('Desktop app');
    expect(url.searchParams.get('operating-system')).toBe('Linux');
    expect(url.searchParams.get('attachments')).toContain(JSON.stringify(snapshot, null, 2));
    expect(snapshot.inventory.error).toBe('numeric_out_of_range');
    expect(snapshot.update.status).toBe('unavailable');
    expect(link.needsAttachment).toBe(false);
  });

  it('reports the installed version even when the updater never answers', () => {
    const snapshot = feedbackSnapshot(state, null, null);
    expect(snapshot.app.version).toBe('0.8.0');
    expect(new URL(feedbackLink(snapshot, false).url).searchParams.get('version')).toBe('0.8.0 (build abc1234)');
    const unknown = feedbackSnapshot({ ...state, appVersion: null }, null, null);
    expect(new URL(feedbackLink(unknown, false).url).searchParams.get('version')).toBe('Unknown version (build abc1234)');
    const update = { current_version: '0.8.1', checked: true, available: false, support: 'supported', version: null, notes: null } as UpdateStatus;
    expect(feedbackSnapshot(state, update, null).app.version).toBe('0.8.1');
  });

  it('excludes diagnostics when unchecked', () => {
    const url = new URL(feedbackLink(feedbackSnapshot(state, null, null), false).url);
    expect(url.searchParams.has('attachments')).toBe(false);
  });

  it('does not serialize arbitrary errors or extra account/inventory properties', () => {
    const snapshot = feedbackSnapshot({
      ...state,
      scanError: 'token secret-user-value at /home/private-user/data',
      marketError: new Error('timeout token secret-user-value'),
      wfmError: new DesktopCmdError('wfm', '/v2/me returned 403 Forbidden: secret-user-value'),
      autoScan: { enabled: true, cadenceMinutes: 30, adoptAutomatically: true, held: false, gameRunning: true, lastScanAt: null, lastError: 'secret-user-value HTTP 500', nextCheckAt: null },
      accountId: 'secret-user-value', inventory: ['private item'],
    } as FeedbackState, null, null);
    const text = JSON.stringify(snapshot);
    expect(text).not.toContain('secret-user-value');
    expect(text).not.toContain('private');
    expect(snapshot.inventory.error).toBe('unclassified_error');
    expect(snapshot.market.error).toBe('timeout');
    expect(snapshot.wfm).toEqual({ session: 'logged_out', error: 'wfm', httpStatus: 403 });
    expect(snapshot.autoScan).toMatchObject({ enabled: true, gameRunning: true, error: 'unclassified_error', httpStatus: 500 });
  });

  it('uses a file fallback when the encoded URL is too large', () => {
    const snapshot = feedbackSnapshot({ ...state, view: 'x'.repeat(7000) }, null, null);
    const link = feedbackLink(snapshot, true);
    expect(link.needsAttachment).toBe(true);
    expect(link.url.length).toBeLessThan(6000);
    expect(new URL(link.url).searchParams.has('attachments')).toBe(false);
  });
});

describe('feedback problems', () => {
  it('names each failing area in words, without error text', () => {
    const snapshot = feedbackSnapshot({
      ...state,
      scanError: "Warframe doesn't appear to be running.",
      wfmError: new DesktopCmdError('signin_window_failed', 'The page crashed at /home/user'),
      marketError: null,
    }, null, { operation: 'install', status: 'failed', error: 'signature_error' });
    expect(feedbackProblems(snapshot)).toEqual([
      { area: 'Scan', summary: 'Warframe was not running' },
      { area: 'warframe.market sign-in', summary: 'the sign-in window failed' },
      { area: 'Update', summary: 'a signature check failed' },
    ]);
  });

  it('adds the HTTP status and is empty when nothing failed', () => {
    const rejected = feedbackSnapshot({ ...state, scanError: 'Inventory endpoint returned HTTP 403 Forbidden (12 bytes).' }, null, null);
    expect(feedbackProblems(rejected)).toEqual([{ area: 'Scan', summary: 'the inventory request was refused (HTTP 403)' }]);
    expect(feedbackProblems(feedbackSnapshot({ ...state, scanError: null }, null, null))).toEqual([]);
  });
});

describe('bug report links', () => {
  // GitHub drops a prefill whose id the form lacks, or that targets a dropdown,
  // without any error - the report just arrives with the field empty.
  it('prefill only fields the form has as text inputs', () => {
    const fields = new Map([...bugForm.matchAll(/- type: (\w+)\n\s+id: ([\w-]+)/g)].map(m => [m[2], m[1]]));
    const snapshot = feedbackSnapshot(state, null, null);
    const url = new URL(feedbackLink(snapshot, true).url);
    const keys = [...url.searchParams.keys()].filter(k => k !== 'template');
    expect(keys.sort()).toEqual(['attachments', 'operating-system', 'surface', 'version']);
    for (const key of keys) expect(['input', 'textarea']).toContain(fields.get(key));
    expect(url.searchParams.get('template')).toBe('bug-report.yml');
  });

  it('prefills only the fields it is given', () => {
    const url = new URL(bugReportUrl({ surface: 'Website', version: 'build abc1234' }));
    expect(url.searchParams.get('surface')).toBe('Website');
    expect(url.searchParams.get('version')).toBe('build abc1234');
    expect(url.searchParams.has('operating-system')).toBe(false);
    expect(url.searchParams.has('attachments')).toBe(false);
  });
});
