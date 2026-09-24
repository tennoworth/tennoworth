import { expect, it, vi } from 'vitest';
import { ListingController } from './controller.svelte';
import type { PendingPlan } from '../../contracts/data';

/** A saved batch with one item still to send. */
const unfinished: PendingPlan = {
  plan_id: 'p',
  started_at: '2026-09-21T10:00:00Z',
  items: [{ slug: 'primed_flow', platinum: 26, quantity: 2, order_type: 'sell', visible: false, status: 'pending' }],
};

function controller(getPendingPlan: () => Promise<PendingPlan | null>) {
  return new ListingController(
    {
      getPendingPlan: vi.fn(getPendingPlan),
      resumePendingPlan: vi.fn(),
      discardPendingPlan: vi.fn(),
      status: vi.fn(),
      logout: vi.fn(),
    },
    () => {},
  );
}

it('reads the saved batch straight through when nothing is in flight', async () => {
  const port = vi.fn().mockResolvedValue(unfinished);
  const c = controller(port);

  await c.refreshPendingPlan();

  expect(port).toHaveBeenCalledTimes(1);
  expect(c.pendingPlan).toEqual(unfinished);
});

/// The defect: the native side writes the plan before the first request and
/// deletes it once the batch completes, so a read during a send captures work
/// in progress. Nothing re-read it afterwards, so closing the review mid-send
/// left a phantom interrupted batch that Resume could only answer with
/// `no_pending`.
it('holds a read made during a send and replays it once the send settles', async () => {
  let settle!: () => void;
  let reads = 0;
  const c = controller(() => {
    reads += 1;
    return Promise.resolve(unfinished);
  });

  const send = c.trackSend(() => new Promise<void>((resolve) => { settle = resolve; }));

  // The review closes while the send is still running. The plan file exists
  // right now and would read as an all-pending interrupted batch, so no read is
  // allowed to happen yet.
  await c.refreshPendingPlan();
  expect(reads, 'an in-flight read must not happen at all').toBe(0);
  expect(c.pendingPlan, 'and must not invent a batch').toBeNull();

  settle!();
  await send;

  expect(reads, 'the settled read happens exactly once').toBe(1);
  expect(c.pendingPlan, 'and shows the batch as it stands after the send').toEqual(unfinished);
});

it('clears a stale batch once a settled read finds nothing left', async () => {
  let settle!: () => void;
  const c = controller(() => Promise.resolve(null));

  const send = c.trackSend(() => new Promise<void>((resolve) => { settle = resolve; }));
  await c.refreshPendingPlan();
  settle!();
  await send;

  expect(c.pendingPlan, 'a completed batch leaves nothing to resume').toBeNull();
});

it('a read after a failed send still shows what was left unfinished', async () => {
  let fail!: () => void;
  const c = controller(() => Promise.resolve(unfinished));

  const send = c.trackSend(() => new Promise<void>((_, reject) => { fail = () => reject(new Error('offline')); }));

  await c.refreshPendingPlan();
  fail!();
  await send.catch(() => {});

  expect(c.pendingPlan, 'the unsent batch is reported').toEqual(unfinished);
});

it('a send that settles without a pending read does not read one', async () => {
  const port = vi.fn().mockResolvedValue(null);
  const c = controller(port);

  await c.trackSend(async () => 'done');

  expect(port, 'no read was asked for, so none happens').not.toHaveBeenCalled();
});

it('a read failure leaves the last known batch alone', async () => {
  const c = controller(() => Promise.reject(new Error('ipc down')));
  c.pendingPlan = unfinished;

  await c.refreshPendingPlan();

  expect(c.pendingPlan, 'a failed read is not an absent batch').toEqual(unfinished);
});

it('reports a pending read failure without losing the saved batch', async () => {
  const c = controller(() => Promise.reject(new Error('journal unreadable')));
  c.pendingPlan = unfinished;
  await c.refreshPendingPlan();
  expect(c.pendingPlan).toEqual(unfinished);
  expect(c.resumePhase).toBe('error');
  expect(c.resumeError).toContain('journal unreadable');
});

it('keeps a failed post-resume journal read visible', async () => {
  const c = controller(() => Promise.reject(new Error('journal unreadable')));
  c.pendingPlan = unfinished;
  await c.doResume();
  expect(c.pendingPlan).toEqual(unfinished);
  expect(c.resumePhase).toBe('error');
  expect(c.resumeError).toContain('journal unreadable');
});
