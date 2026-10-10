import { describe, expect, it } from 'vitest';
import { ActivityLog } from './activity.svelte';

describe('ActivityLog', () => {
  it('keeps the last result after work finishes', async () => {
    const log = new ActivityLog(() => 1);
    await log.track('Sending listings…', async () => 2, (n) => `Listed ${n}`, () => 'Listing failed');
    expect(log.current).toMatchObject({ label: 'Listed 2', tone: 'done' });
  });

  it('reports a failure and rethrows it', async () => {
    const log = new ActivityLog();
    await expect(log.track('Scanning…', async () => { throw new Error('boom'); }, () => 'ok', () => 'Scan failed')).rejects.toThrow('boom');
    expect(log.current).toMatchObject({ label: 'Scan failed', tone: 'bad' });
  });

  it('does not let an older result overwrite newer work in progress', () => {
    const log = new ActivityLog();
    const scan = log.begin('Scanning game…');
    const send = log.begin('Sending listings…');
    log.finish(scan, 'Scan applied');
    expect(log.current).toMatchObject({ id: send, label: 'Sending listings…', tone: 'busy' });
    log.finish(send, 'Listed 1');
    expect(log.current).toMatchObject({ label: 'Listed 1', tone: 'done' });
  });

  it('updates progress only for the activity still shown', () => {
    const log = new ActivityLog();
    const first = log.begin('Checking 0/3');
    log.progress(first, 'Checking 1/3');
    expect(log.current?.label).toBe('Checking 1/3');
    log.finish(first, 'Done');
    log.progress(first, 'Checking 2/3');
    expect(log.current?.label).toBe('Done');
  });
});
