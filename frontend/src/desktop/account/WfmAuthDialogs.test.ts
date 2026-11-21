import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { cleanup, screen, waitFor, within } from '@testing-library/svelte';
import { renderDesktop as render } from '../../dev/preview/render-desktop';
import { installTauri, removeTauri } from '../../dev/testing/test-utils';
import WfmAuthDialogs from './WfmAuthDialogs.svelte';

const shown = vi.fn();

beforeEach(() => {
  shown.mockClear();
  Object.defineProperty(HTMLDialogElement.prototype, 'showModal', {
    configurable: true,
    value() { shown(this.dataset.testid); this.setAttribute('open', ''); },
  });
  Object.defineProperty(HTMLDialogElement.prototype, 'close', {
    configurable: true,
    value() { this.removeAttribute('open'); },
  });
});

afterEach(() => {
  cleanup();
  removeTauri();
});

function mount(unlock: () => unknown) {
  const invoke = vi.fn(async (command: string) => (command === 'unlock_jwt' ? unlock() : null));
  installTauri(invoke, undefined);
  const onunlocked = vi.fn();
  return { ...render(WfmAuthDialogs, { props: { onunlocked } }), onunlocked, invoke };
}

const isOpen = (id: string) => screen.getByTestId(id).hasAttribute('open');

it('a saved sign-in opens with nothing on screen', async () => {
  const { component, onunlocked, invoke } = mount(() => null);
  await component.open('needs_unlock', 'list');
  expect(invoke).toHaveBeenCalledWith('unlock_jwt');
  expect(onunlocked).toHaveBeenCalledWith('list');
  expect(shown).not.toHaveBeenCalled();
});

it('a sign-in that can no longer open goes straight to signing in', async () => {
  const { component, onunlocked } = mount(() => { throw { code: 'needs_login', message: 'Log in.' }; });
  await component.open('needs_unlock');
  await waitFor(() => expect(isOpen('wfm-login-dialog')).toBe(true));
  expect(isOpen('wfm-unlock-dialog')).toBe(false);
  expect(onunlocked).not.toHaveBeenCalled();
});

it('a keyring that did not answer says so and can retry', async () => {
  let answer: () => unknown = () => { throw { code: 'keyring_unavailable', message: 'Your system keyring didn’t answer.' }; };
  const { component, onunlocked } = mount(() => answer());
  await component.open('needs_unlock');
  await waitFor(() => expect(isOpen('wfm-unlock-dialog')).toBe(true));
  const dialog = within(screen.getByTestId('wfm-unlock-dialog'));
  expect(dialog.getByTestId('wfm-auth-error').textContent).toMatch(/keyring didn.t answer/);
  expect(dialog.queryByText('Report a bug')).toBeNull();

  answer = () => null;
  dialog.getByRole('button', { name: 'Try again' }).click();
  await waitFor(() => expect(onunlocked).toHaveBeenCalled());
  expect(isOpen('wfm-unlock-dialog')).toBe(false);
});

it('signing in asks for no passphrase and starts on Continue', async () => {
  const { component } = mount(() => null);
  await component.open('needs_login');
  expect(isOpen('wfm-login-dialog')).toBe(true);
  expect(document.activeElement?.textContent?.trim()).toBe('Continue to warframe.market');
  expect(screen.getByTestId('wfm-login-dialog').querySelector('input[type="password"]')).toBeNull();
});
