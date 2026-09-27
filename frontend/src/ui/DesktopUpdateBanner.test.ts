import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import DesktopUpdateBanner from './DesktopUpdateBanner.svelte';
import { DESKTOP_CONTEXT } from '../contracts/services';
import type { UpdateStatus } from '../contracts/update';

const available: UpdateStatus = {
  checked: true,
  available: true,
  support: 'supported',
  version: '0.8.0',
  current_version: '0.7.107',
  notes: null,
} as UpdateStatus;

function mount(installUpdate: () => Promise<void>) {
  const services = {
    updateStatus: vi.fn(async () => available),
    checkUpdate: vi.fn(async () => available),
    installUpdate: vi.fn(installUpdate),
    restartApp: vi.fn(async () => {}),
    onUpdateAvailable: vi.fn(() => () => {}),
  };
  render(DesktopUpdateBanner, { context: new Map([[DESKTOP_CONTEXT, services]]) });
  return services;
}

afterEach(() => { cleanup(); vi.restoreAllMocks(); });

describe('desktop update banner install failures', () => {
  it('turns a rejected signature into a one-time manual download, not a raw error', async () => {
    mount(async () => { throw new Error('Signature verification failed'); });
    await fireEvent.click(await screen.findByRole('button', { name: 'Install update' }));
    const note = await screen.findByTestId('update-manual-install');
    expect(note.textContent).toContain('Download it once');
    const link = note.querySelector('a');
    expect(link?.getAttribute('href')).toBe('https://github.com/tennoworth/tennoworth/releases/latest');
    expect(link?.getAttribute('target')).toBe('_blank');
    expect(screen.queryByText('Signature verification failed')).toBeNull();
  });

  it('keeps showing other install failures as they are', async () => {
    mount(async () => { throw new Error('download failed: connection reset'); });
    await fireEvent.click(await screen.findByRole('button', { name: 'Install update' }));
    expect(await screen.findByText('download failed: connection reset')).toBeTruthy();
    expect(screen.queryByTestId('update-manual-install')).toBeNull();
  });
});
