import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import ExportImportDialogs from './ExportImportDialogs.svelte';
import { decryptPayload } from '../lib/crypto';

vi.mock('../lib/crypto', () => ({
  encryptPayload: vi.fn(),
  decryptPayload: vi.fn(),
}));

const encryptedFile = JSON.stringify({
  format: 'wfminv-encrypted-v1',
  created: '2026-08-28T12:00:00.000Z',
  kdf: {},
  cipher: {},
  ciphertext: 'test',
});

const restoredPayload = {
  invName: 'restored inventory',
  ts: 1_777_374_400_000,
  owned: [['ash_prime_blueprint', { qty: 2 }]],
};

function inventory(count = 2) {
  return new Map(
    Array.from({ length: count }, (_, index) => [
      `item_${index}|`,
      { slug: `item_${index}`, subtype: null, qty: 1, leveled: 0 },
    ]),
  );
}

function mount(owned = inventory()) {
  const onimport = vi.fn().mockResolvedValue(undefined);
  const view = render(ExportImportDialogs, {
    props: { owned, inventoryName: 'current', lastUpdated: 123, onimport },
  });
  return { ...view, onimport };
}

async function chooseBackup(container: HTMLElement) {
  const file = new File([encryptedFile], 'wfminv-backup.json', { type: 'application/json' });
  // jsdom's File implementation does not consistently expose Blob.text().
  Object.defineProperty(file, 'text', { value: vi.fn().mockResolvedValue(encryptedFile) });
  const input = container.querySelector('input[type="file"]') as HTMLInputElement;
  Object.defineProperty(input, 'files', { configurable: true, value: [file] });
  await fireEvent.change(input);
}

async function enterPassphrase() {
  await fireEvent.input(document.querySelector('input[autocomplete="current-password"]') as HTMLInputElement, {
    target: { value: 'correct horse battery staple' },
  });
}

beforeEach(() => {
  vi.mocked(decryptPayload).mockReset();
  Object.defineProperty(HTMLDialogElement.prototype, 'showModal', {
    configurable: true,
    value() { this.setAttribute('open', ''); },
  });
  Object.defineProperty(HTMLDialogElement.prototype, 'close', {
    configurable: true,
    value() { this.removeAttribute('open'); },
  });
});

afterEach(cleanup);

describe('encrypted inventory restore confirmation', () => {
  it('does not decrypt a non-empty inventory before explicit in-app confirmation', async () => {
    const { container, onimport } = mount(inventory(2));
    await chooseBackup(container);
    await enterPassphrase();

    await fireEvent.click(screen.getByRole('button', { name: 'Review restore' }));

    expect(screen.getByText('Replace current inventory?')).toBeTruthy();
    expect(screen.getByText(/current 2-item inventory/)).toBeTruthy();
    expect(screen.getByText(/wfminv-backup\.json/)).toBeTruthy();
    expect(document.querySelector('input[type="file"]')).toBeNull();
    await waitFor(() => expect(document.activeElement).toBe(screen.getByText('Replace current inventory?').parentElement));
    expect(decryptPayload).not.toHaveBeenCalled();
    expect(onimport).not.toHaveBeenCalled();

    await fireEvent.click(screen.getByRole('button', { name: 'Back' }));
    expect(screen.getByRole('button', { name: 'Review restore' })).toBeTruthy();
    await waitFor(() => expect(document.activeElement).toBe(document.querySelector('input[autocomplete="current-password"]')));
    expect((document.activeElement as HTMLInputElement).value).toBe('correct horse battery staple');
    expect(decryptPayload).not.toHaveBeenCalled();
  });

  it('decrypts and replaces only after Confirm restore', async () => {
    vi.mocked(decryptPayload).mockResolvedValue(restoredPayload);
    const { container, onimport } = mount(inventory(2));
    await chooseBackup(container);
    await enterPassphrase();

    await fireEvent.click(screen.getByRole('button', { name: 'Review restore' }));
    await fireEvent.click(screen.getByRole('button', { name: 'Confirm restore' }));

    await waitFor(() => expect(decryptPayload).toHaveBeenCalledTimes(1));
    expect(decryptPayload).toHaveBeenCalledWith(
      expect.objectContaining({ format: 'wfminv-encrypted-v1' }),
      'correct horse battery staple',
    );
    await waitFor(() => expect(onimport).toHaveBeenCalledTimes(1));
    expect(document.querySelector('dialog[open]')).toBeNull();
    const result = onimport.mock.calls[0][0];
    expect(result.invName).toBe('restored inventory');
    expect(result.ts).toBe(restoredPayload.ts);
    expect(result.ownedMap.get('ash_prime_blueprint|')).toEqual({
      qty: 2,
      slug: 'ash_prime_blueprint',
      subtype: null,
      leveled: 0,
    });
  });

  it('restores directly when there is no current inventory to replace', async () => {
    vi.mocked(decryptPayload).mockResolvedValue(restoredPayload);
    const { container, onimport } = mount(new Map());
    await chooseBackup(container);
    await enterPassphrase();

    expect(screen.queryByRole('button', { name: 'Review restore' })).toBeNull();
    await fireEvent.click(screen.getByRole('button', { name: 'Decrypt' }));

    await waitFor(() => expect(decryptPayload).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(onimport).toHaveBeenCalledTimes(1));
    expect(screen.queryByText('Replace current inventory?')).toBeNull();
  });

  it('returns to the passphrase form without importing when decryption fails', async () => {
    vi.mocked(decryptPayload).mockRejectedValue(new Error('Wrong passphrase, or the file was modified.'));
    const { container, onimport } = mount(inventory(1));
    await chooseBackup(container);
    await enterPassphrase();

    await fireEvent.click(screen.getByRole('button', { name: 'Review restore' }));
    await fireEvent.click(screen.getByRole('button', { name: 'Confirm restore' }));

    await waitFor(() => {
      const openDialog = document.querySelector('dialog[open]') as HTMLDialogElement;
      expect(within(openDialog).getByRole('alert').textContent).toBe('Wrong passphrase, or the file was modified.');
    });
    expect(screen.getByRole('button', { name: 'Review restore' })).toBeTruthy();
    expect(onimport).not.toHaveBeenCalled();
  });
});
