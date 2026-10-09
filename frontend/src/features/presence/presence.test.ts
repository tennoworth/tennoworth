// Trade presence: the strip and Settings must show what warframe.market last
// committed, and a pick must never be reported as applied when it was not.
import { describe, expect, it, vi } from 'vitest';
import { PresenceController } from './presence.svelte';
import { keepForLabel, presenceLine, presenceProblem, presenceUsable, presenceWord } from './presence';
import { PRESENCE_CHANGED_EVENT } from '../../contracts/events';
import { DesktopCmdError } from '../../contracts/errors';
import type { PresenceStatus } from '../../contracts/desktop';

const BASE: PresenceStatus = {
  signedIn: true, connected: true, status: 'ingame', statusUntil: null, statusSetAt: '2026-10-09T12:02:00Z',
  managed: true, following: true, followPaused: false, gameRunning: true, problem: null, detail: null,
  settings: { followGame: true, whenClosed: 'invisible', keepForMinutes: null },
};
const at = (iso: string) => iso.slice(11, 16);

function port(overrides: Record<string, unknown> = {}) {
  let listener: ((payload: unknown) => void) | null = null;
  const api = {
    presenceStatus: vi.fn(async () => BASE),
    setPresence: vi.fn(async (status: string) => ({ ...BASE, status, managed: false, following: false, followPaused: true }) as PresenceStatus),
    updatePresenceSettings: vi.fn(async (settings: PresenceStatus['settings']) => settings),
    followGameNow: vi.fn(async () => undefined),
    listen: vi.fn((event: string, cb: (payload: unknown) => void) => {
      expect(event).toBe(PRESENCE_CHANGED_EVENT);
      listener = cb;
      return () => { listener = null; };
    }),
    ...overrides,
  };
  const c = new PresenceController({ native: api as never, listen: api.listen as never });
  return { api, c, push: (status: PresenceStatus) => listener?.(status) };
}

describe('PresenceController', () => {
  it('reads the current status and follows native pushes', async () => {
    const { c, push } = port();
    const stop = c.start();
    await vi.waitFor(() => expect(c.status?.status).toBe('ingame'));
    push({ ...BASE, status: 'invisible', following: false, followPaused: true });
    expect(c.status?.status).toBe('invisible');
    stop();
  });

  it('a refused pick keeps the committed status and says why', async () => {
    const { c, api } = port({
      setPresence: vi.fn(async () => { throw new DesktopCmdError('presence_not_verified', 'warframe.market refused the change: this account is not verified.'); }),
    });
    c.start();
    await vi.waitFor(() => expect(c.status).not.toBeNull());
    await c.set('online');
    expect(api.setPresence).toHaveBeenCalledWith('online');
    expect(c.status?.status).toBe('ingame');
    expect(c.error).toContain('not verified');
    expect(c.pending).toBeNull();
  });

  it('a push that lands while a pick is in flight wins over the older reply', async () => {
    let release: (s: PresenceStatus) => void = () => {};
    const { c, push } = port({ setPresence: vi.fn(() => new Promise<PresenceStatus>((resolve) => { release = resolve; })) });
    c.start();
    await vi.waitFor(() => expect(c.status).not.toBeNull());
    const picking = c.set('online');
    expect(c.pending).toBe('online');
    push({ ...BASE, status: 'invisible' });
    release({ ...BASE, status: 'online' });
    await picking;
    expect(c.status?.status).toBe('invisible');
  });

  it('saving settings keeps the rest of the status', async () => {
    const { c } = port();
    c.start();
    await vi.waitFor(() => expect(c.status).not.toBeNull());
    await c.saveSettings({ followGame: false, whenClosed: 'online', keepForMinutes: 60 });
    expect(c.status?.settings).toEqual({ followGame: false, whenClosed: 'online', keepForMinutes: 60 });
    expect(c.status?.status).toBe('ingame');
  });
});

describe('presence text', () => {
  it('uses warframe.market’s keep-for labels', () => {
    expect([null, 30, 60, 120, 240].map(keepForLabel)).toEqual(['While running', '30m', '1h', '2h', '4h']);
  });

  it('says how the status is held', () => {
    expect(presenceLine(BASE, at)).toBe('Following the game · Warframe running');
    expect(presenceLine({ ...BASE, gameRunning: false }, at)).toBe('Following the game · Warframe closed');
    expect(presenceLine({ ...BASE, following: false, followPaused: true }, at)).toBe('Set by hand · following resumes next game session');
    expect(presenceLine({ ...BASE, following: false, settings: { ...BASE.settings, followGame: false } }, at)).toBe('Kept while TennoWorth runs');
    expect(presenceLine({ ...BASE, following: false, managed: false, status: 'online', statusUntil: '2026-10-09T15:10:00Z' }, at)).toBe('Until 15:10');
    expect(presenceLine({ ...BASE, following: false, managed: false, status: 'invisible' }, at)).toBe('Others see you as Offline');
    expect(presenceLine({ ...BASE, connected: false, problem: 'unreachable' }, at)).toBe('Status channel unreachable · retrying');
  });

  it('never shows a status it does not have', () => {
    expect(presenceWord({ ...BASE, status: null, connected: false })).toBe('Connecting…');
    expect(presenceWord({ ...BASE, problem: 'not_verified' })).toBe('Status refused');
    expect(presenceUsable({ ...BASE, problem: 'not_verified' })).toBe(false);
    expect(presenceUsable({ ...BASE, connected: false })).toBe(false);
    expect(presenceUsable(BASE)).toBe(true);
  });

  it('explains each problem with what happens next', () => {
    expect(presenceProblem(BASE)).toBeNull();
    expect(presenceProblem({ ...BASE, problem: 'refused', detail: 'duration: app.field.tooSmall' })).toContain('app.field.tooSmall');
    expect(presenceProblem({ ...BASE, problem: 'paused' })).toContain('until access resumes');
  });
});
