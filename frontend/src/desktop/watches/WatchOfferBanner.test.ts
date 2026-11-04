// The banner a price-watch hit leaves in the app: it shows the offer while it
// is fresh, rechecks warframe.market at most once and only while the app is in
// front, and copies through the native command.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest';
import { screen, fireEvent, waitFor, cleanup } from '@testing-library/svelte';
import { renderDesktop as render } from '../../dev/preview/render-desktop';
import WatchOfferBanner from './WatchOfferBanner.svelte';
import { installTauri, removeTauri } from '../../dev/testing/test-utils';
import { WATCH_OFFER_EVENT } from '../../contracts/events';
import { WATCH_OFFER_FRESH_SECS, type WatchOffer } from '../../contracts/desktop';

const nowSecs = () => Math.floor(Date.now() / 1000);
const offer = (over: Partial<WatchOffer> = {}): WatchOffer => ({
  watch_id: 1, slug: 'ash_prime_set', name: 'Ash Prime Set', side: 'sell', threshold: 120, price: 110, rank: null, subtype: null,
  player: 'Seller', whisper: '/w Seller Hi! I want to buy: "Ash Prime Set" for 110 platinum. (warframe.market via TennoWorth)',
  seen_at: nowSecs() - 90, rechecked_at: null, still_listed: null, ...over,
});

let focused = true;
beforeEach(() => { focused = true; vi.spyOn(document, 'hasFocus').mockImplementation(() => focused); });
afterEach(() => { cleanup(); removeTauri(); vi.restoreAllMocks(); });

function runtime(current: () => WatchOffer | null, recheck: () => WatchOffer | null = () => ({ ...current()!, rechecked_at: nowSecs(), still_listed: true })) {
  const calls: string[] = [];
  const handlers: Record<string, (event: { payload: unknown }) => void> = {};
  installTauri(vi.fn(async (cmd: string) => {
    calls.push(cmd);
    if (cmd === 'latest_watch_offer') return current();
    if (cmd === 'recheck_watch_offer') return recheck();
    if (cmd === 'copy_watch_whisper' || cmd === 'dismiss_watch_offer') return null;
    throw new Error(`unexpected ${cmd}`);
  }), vi.fn((name: string, handler: (event: { payload: unknown }) => void) => { handlers[name] = handler; return Promise.resolve(() => {}); }));
  return { calls, handlers };
}

describe('WatchOfferBanner', () => {
  it('names the player, the price and the whisper, and copies through Rust', async () => {
    const { calls } = runtime(() => offer());
    render(WatchOfferBanner, { props: {} });
    const banner = await screen.findByTestId('watch-offer');
    expect(banner.textContent).toContain('Seller sells Ash Prime Set at 110p');
    expect(banner.textContent).toContain('(you wanted ≤ 120p)');
    expect(banner.textContent).toContain('1 min ago');
    expect(banner.textContent).toContain('/w Seller Hi!');
    await fireEvent.click(screen.getByRole('button', { name: 'Copy whisper' }));
    await screen.findByRole('button', { name: 'Whisper copied' });
    expect(calls).toContain('copy_watch_whisper');
  });

  it('rechecks once when the app is in front and says when the order is gone', async () => {
    const { calls } = runtime(() => offer(), () => offer({ rechecked_at: nowSecs(), still_listed: false }));
    render(WatchOfferBanner, { props: {} });
    await screen.findByText(/No longer listed at your price/);
    window.dispatchEvent(new Event('focus'));
    window.dispatchEvent(new Event('focus'));
    expect(calls.filter((c) => c === 'recheck_watch_offer')).toHaveLength(1);
  });

  it('asks warframe.market nothing while the player is in game, then once on switching over', async () => {
    focused = false;
    const { calls } = runtime(() => offer());
    render(WatchOfferBanner, { props: {} });
    await screen.findByTestId('watch-offer');
    expect(calls).not.toContain('recheck_watch_offer');
    focused = true;
    window.dispatchEvent(new Event('focus'));
    await screen.findByText(/Still listed/);
    expect(calls.filter((c) => c === 'recheck_watch_offer')).toHaveLength(1);
  });

  it('shows nothing for an expired offer and follows the offer event', async () => {
    let current: WatchOffer | null = offer({ seen_at: nowSecs() - WATCH_OFFER_FRESH_SECS - 1 });
    const { handlers, calls } = runtime(() => current);
    render(WatchOfferBanner, { props: {} });
    await waitFor(() => expect(calls).toContain('latest_watch_offer'));
    expect(screen.queryByTestId('watch-offer')).toBeNull();
    current = offer({ player: 'Fresh' });
    handlers[WATCH_OFFER_EVENT]({ payload: null });
    expect((await screen.findByTestId('watch-offer')).textContent).toContain('Fresh sells');
  });

  it('keeps the whisper readable when copying fails', async () => {
    installTauri(vi.fn(async (cmd: string) => {
      if (cmd === 'latest_watch_offer') return offer({ rechecked_at: nowSecs(), still_listed: true });
      if (cmd === 'copy_watch_whisper') throw new Error('Could not copy the whisper. Copy it from the text instead.');
      throw new Error(`unexpected ${cmd}`);
    }), vi.fn(() => Promise.resolve(() => {})));
    render(WatchOfferBanner, { props: {} });
    await fireEvent.click(await screen.findByRole('button', { name: 'Copy whisper' }));
    expect((await screen.findByRole('alert')).textContent).toContain('Copy it from the text instead');
    expect(screen.getByTestId('watch-offer').textContent).toContain('/w Seller Hi!');
  });

  it('dismissing hides it and tells Rust', async () => {
    const { calls } = runtime(() => offer({ rechecked_at: nowSecs(), still_listed: true }));
    render(WatchOfferBanner, { props: {} });
    await fireEvent.click(await screen.findByRole('button', { name: 'Dismiss price watch hit' }));
    expect(screen.queryByTestId('watch-offer')).toBeNull();
    expect(calls).toContain('dismiss_watch_offer');
  });
});
