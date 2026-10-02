import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { cleanup, screen, waitFor } from '@testing-library/svelte';
import { renderDesktop as render } from '../../dev/render-desktop';
import { installTauri, removeTauri } from '../../dev/test-utils';
import WfmAuthDialogs from './WfmAuthDialogs.svelte';

beforeEach(() => {
  Object.defineProperty(HTMLDialogElement.prototype, 'showModal', {
    configurable: true,
    value() { this.setAttribute('open', ''); },
  });
});

afterEach(() => {
  cleanup();
  removeTauri();
});

function mount(keyring: boolean) {
  installTauri(vi.fn(async (command: string) => {
    if (command === 'wfm_remember_available') return keyring;
    if (command === 'try_silent_unlock') return false;
    return null;
  }), undefined);
  return render(WfmAuthDialogs, { props: { onunlocked: vi.fn() } });
}

// With KDE Wallet switched off the tick was accepted and the next launch asked
// for the passphrase again, with nothing on screen to say why.
it.each(['needs_unlock', 'needs_login'])('%s says when the keyring cannot remember this device', async code => {
  const { component } = mount(false);
  await component.open(code);
  await waitFor(() => expect(document.querySelector('dialog[open] [data-testid="wfm-remember-unavailable"]')?.textContent).toMatch(/keyring isn't available/));
});

it('says nothing extra when a keyring answers', async () => {
  const { component } = mount(true);
  await component.open('needs_unlock');
  await waitFor(() => expect(screen.getByTestId('wfm-unlock-dialog').hasAttribute('open')).toBe(true));
  await new Promise(resolve => setTimeout(resolve, 0));
  expect(document.querySelector('[data-testid="wfm-remember-unavailable"]')).toBeNull();
});
