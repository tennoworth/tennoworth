import type { ToastMessage } from './Toast.svelte';

export function createToastQueue(duration: number) {
  let toasts = $state<ToastMessage[]>([]);
  let sequence = 0;
  let disposed = false;
  const timers = new Map<number, number>();

  function dismiss(id: number): void {
    const timer = timers.get(id);
    if (timer !== undefined) {
      window.clearTimeout(timer);
      timers.delete(id);
    }
    toasts = toasts.filter(toast => toast.id !== id);
  }

  function push(text: string, kind: ToastMessage['kind'] = 'success'): void {
    if (disposed) return;
    const id = ++sequence;
    toasts = [...toasts, { id, kind, text }];
    timers.set(id, window.setTimeout(() => dismiss(id), duration));
  }

  function dispose(): void {
    disposed = true;
    for (const timer of timers.values()) window.clearTimeout(timer);
    timers.clear();
  }

  return { get toasts() { return toasts; }, push, dismiss, dispose };
}
