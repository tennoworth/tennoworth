import { afterEach, expect, it, vi } from 'vitest';
import { createToastQueue } from './toast-queue.svelte';

afterEach(() => { vi.useRealTimers(); });

it.each([4500, 5000])('expires each toast independently after %i ms', duration => {
  vi.useFakeTimers();
  const queue = createToastQueue(duration);
  queue.push('first');
  vi.advanceTimersByTime(1000);
  queue.push('second', 'error');
  expect(queue.toasts.map(toast => toast.kind)).toEqual(['success', 'error']);
  vi.advanceTimersByTime(duration - 1001);
  expect(queue.toasts).toHaveLength(2);
  vi.advanceTimersByTime(1);
  expect(queue.toasts.map(toast => toast.text)).toEqual(['second']);
  vi.advanceTimersByTime(1000);
  expect(queue.toasts).toEqual([]);
  expect(vi.getTimerCount()).toBe(0);
});

it('cancels a manually dismissed timer without affecting other toasts', () => {
  vi.useFakeTimers();
  const queue = createToastQueue(4500);
  queue.push('first');
  queue.push('second');
  queue.dismiss(queue.toasts[0].id);
  queue.dismiss(-1);
  expect(queue.toasts.map(toast => toast.text)).toEqual(['second']);
  expect(vi.getTimerCount()).toBe(1);
  queue.dispose();
  expect(vi.getTimerCount()).toBe(0);
});

it('cancels all timers on disposal and ignores late async notifications', () => {
  vi.useFakeTimers();
  const queue = createToastQueue(5000);
  queue.push('first');
  queue.push('second');
  queue.dispose();
  queue.dispose();
  queue.push('late');
  expect(vi.getTimerCount()).toBe(0);
  expect(queue.toasts.map(toast => toast.text)).toEqual(['first', 'second']);
});
