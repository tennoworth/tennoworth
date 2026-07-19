// @ts-nocheck — vitest runs these as JS-style fixtures; full TS shapes here would be busy-work without catching real bugs.
import { describe, it, expect, beforeEach, vi } from 'vitest';
import {
  buildAssistantContext, askAssistant, assistantErrorMessage, AssistantError,
} from './assistant.js';

function row(overrides = {}) {
  return {
    name: 'item',
    owned: 1,
    sellable: 1,
    avg_price: 10,
    volume_48h: 5,
    sell_score: 1,
    vault_status: null,
    ...overrides,
  };
}

describe('buildAssistantContext', () => {
  it('returns null for an empty row list', () => {
    expect(buildAssistantContext([], { generatedAt: '2 h ago' })).toBeNull();
  });

  it('returns null for missing/non-array input', () => {
    expect(buildAssistantContext(null, {})).toBeNull();
    expect(buildAssistantContext(undefined, {})).toBeNull();
  });

  it('caps items at the top 100 by sell_score, descending', () => {
    const rows = Array.from({ length: 150 }, (_, i) => row({ name: `item-${i}`, sell_score: i }));
    const ctx = JSON.parse(buildAssistantContext(rows, {}));
    expect(ctx.items).toHaveLength(100);
    // Highest sell_score (149) first, and only the top 100 scores survive.
    expect(ctx.items[0].name).toBe('item-149');
    expect(ctx.items[99].name).toBe('item-50');
    expect(ctx.items.some((it) => it.name === 'item-49')).toBe(false);
  });

  it('totals still cover every row, not just the capped top 100', () => {
    const rows = Array.from({ length: 150 }, (_, i) =>
      row({ name: `item-${i}`, sell_score: i, owned: 2, sellable: 2, avg_price: 10 }));
    const ctx = JSON.parse(buildAssistantContext(rows, {}));
    expect(ctx.totals.distinct_items).toBe(150);
    expect(ctx.totals.total_owned).toBe(300);
    expect(ctx.totals.total_estimated_plat).toBe(150 * 2 * 10);
  });

  it('reports sellable, not owned, for each item and in totals math', () => {
    const rows = [row({ owned: 10, sellable: 4, avg_price: 100, sell_score: 5 })];
    const ctx = JSON.parse(buildAssistantContext(rows, {}));
    expect(ctx.items[0].owned).toBe(10);
    expect(ctx.items[0].sellable).toBe(4);
    // 4 sellable × 100 avg = 400, not 10 × 100 = 1000.
    expect(ctx.totals.total_estimated_plat).toBe(400);
  });

  it('labels the volume field vol_48h, not vol or daily', () => {
    const rows = [row({ volume_48h: 37 })];
    const ctx = JSON.parse(buildAssistantContext(rows, {}));
    expect(ctx.items[0].vol_48h).toBe(37);
    expect(ctx.items[0]).not.toHaveProperty('vol');
    expect(ctx.items[0]).not.toHaveProperty('daily_vol');
    expect(ctx.items[0]).not.toHaveProperty('vol_daily');
  });

  it('computes totals math exactly for a small, hand-checked set', () => {
    const rows = [
      row({ owned: 3, sellable: 3, avg_price: 20, sell_score: 1 }),
      row({ owned: 5, sellable: 2, avg_price: 50, sell_score: 2 }),
    ];
    const ctx = JSON.parse(buildAssistantContext(rows, {}));
    expect(ctx.totals.distinct_items).toBe(2);
    expect(ctx.totals.total_owned).toBe(8);
    // 3×20 + 2×50 = 60 + 100 = 160
    expect(ctx.totals.total_estimated_plat).toBe(160);
  });

  it('passes market_data_age through from meta.generatedAt verbatim', () => {
    const ctx = JSON.parse(buildAssistantContext([row()], { generatedAt: '3 h ago' }));
    expect(ctx.market_data_age).toBe('3 h ago');
  });

  it('falls back to "unknown" market_data_age when meta is absent or null', () => {
    expect(JSON.parse(buildAssistantContext([row()])).market_data_age).toBe('unknown');
    expect(JSON.parse(buildAssistantContext([row()], { generatedAt: null })).market_data_age).toBe('unknown');
  });

  it('carries vault status through per item, including null', () => {
    const rows = [
      row({ name: 'vaulted-part', vault_status: 'vaulted' }),
      row({ name: 'plain-part', vault_status: null }),
    ];
    const ctx = JSON.parse(buildAssistantContext(rows, {}));
    const byName = Object.fromEntries(ctx.items.map((it) => [it.name, it.vault]));
    expect(byName['vaulted-part']).toBe('vaulted');
    expect(byName['plain-part']).toBeNull();
  });

  it('does not mutate the input row array (stays pure)', () => {
    const rows = [row({ sell_score: 1 }), row({ sell_score: 2 })];
    const snapshot = JSON.stringify(rows);
    buildAssistantContext(rows, {});
    expect(JSON.stringify(rows)).toBe(snapshot);
  });
});

describe('askAssistant', () => {
  beforeEach(() => {
    globalThis.fetch = vi.fn();
  });

  const cfg = { baseUrl: 'http://x', token: 'tok' };

  it('POSTs question/history/context with X-Session-Token, trimming history to 12', async () => {
    globalThis.fetch.mockResolvedValue({
      ok: true,
      json: async () => ({ answer: 'sell your prime parts', usage: { prompt_tokens: 10, completion_tokens: 5 } }),
    });
    const history = Array.from({ length: 20 }, (_, i) => ({ role: 'user', content: `q${i}` }));
    const resp = await askAssistant(cfg, 'what should I sell?', history, '{"items":[]}');
    expect(resp.answer).toBe('sell your prime parts');
    expect(resp.usage).toEqual({ prompt_tokens: 10, completion_tokens: 5 });

    const [url, init] = globalThis.fetch.mock.calls[0];
    expect(url).toBe('http://x/assistant');
    expect(init.method).toBe('POST');
    expect(init.headers['X-Session-Token']).toBe('tok');
    const sentBody = JSON.parse(init.body);
    expect(sentBody.question).toBe('what should I sell?');
    expect(sentBody.context).toBe('{"items":[]}');
    expect(sentBody.history).toHaveLength(12);
    expect(sentBody.history[0].content).toBe('q8'); // last 12 of 20 → q8..q19
    expect(sentBody.history[11].content).toBe('q19');
  });

  it('defaults usage fields to 0 when the companion omits them', async () => {
    globalThis.fetch.mockResolvedValue({ ok: true, json: async () => ({ answer: 'ok' }) });
    const resp = await askAssistant(cfg, 'q', [], null);
    expect(resp.usage).toEqual({ prompt_tokens: 0, completion_tokens: 0 });
  });

  it('maps 503 no_api_key to AssistantError with code no_api_key', async () => {
    globalThis.fetch.mockResolvedValue({
      ok: false, status: 503,
      json: async () => ({ error: 'no_api_key' }),
    });
    await expect(askAssistant(cfg, 'q', [], null)).rejects.toMatchObject({ code: 'no_api_key' });
  });

  it('maps 502 upstream to AssistantError with code upstream and the detail preserved', async () => {
    globalThis.fetch.mockResolvedValue({
      ok: false, status: 502,
      json: async () => ({ error: 'upstream', detail: 'DeepSeek timed out' }),
    });
    await expect(askAssistant(cfg, 'q', [], null)).rejects.toMatchObject({
      code: 'upstream',
      detail: 'DeepSeek timed out',
    });
  });

  it('maps 401 to AssistantError with code auth regardless of body', async () => {
    globalThis.fetch.mockResolvedValue({
      ok: false, status: 401,
      json: async () => ({ error: 'bad token' }),
    });
    await expect(askAssistant(cfg, 'q', [], null)).rejects.toMatchObject({ code: 'auth' });
  });

  it('maps a network failure (fetch throws) to AssistantError with code network', async () => {
    globalThis.fetch.mockRejectedValue(new TypeError('fetch failed'));
    await expect(askAssistant(cfg, 'q', [], null)).rejects.toMatchObject({ code: 'network' });
  });

  it('maps an unrecognized error body to code unknown instead of throwing raw', async () => {
    globalThis.fetch.mockResolvedValue({
      ok: false, status: 500,
      json: async () => ({ error: 'something else' }),
    });
    await expect(askAssistant(cfg, 'q', [], null)).rejects.toMatchObject({ code: 'unknown' });
  });

  it('maps a malformed 200 body to code unknown', async () => {
    globalThis.fetch.mockResolvedValue({ ok: true, json: async () => ({ notAnAnswer: true }) });
    await expect(askAssistant(cfg, 'q', [], null)).rejects.toMatchObject({ code: 'unknown' });
  });
});

describe('assistantErrorMessage', () => {
  it('maps no_api_key to the DeepSeek key hint', () => {
    expect(assistantErrorMessage(new AssistantError('no_api_key')))
      .toMatch(/DEEPSEEK_API_KEY|deepseek-key/);
  });

  it('maps upstream to "The AI service failed: <detail>"', () => {
    expect(assistantErrorMessage(new AssistantError('upstream', 'timeout')))
      .toBe('The AI service failed: timeout');
  });

  it('maps auth to the same rejected-token copy as other companion auth failures', () => {
    expect(assistantErrorMessage(new AssistantError('auth')))
      .toMatch(/token/i);
  });

  it('maps network to "Companion unreachable."', () => {
    expect(assistantErrorMessage(new AssistantError('network'))).toBe('Companion unreachable.');
  });

  it('falls back to the raw message for a plain Error', () => {
    expect(assistantErrorMessage(new Error('boom'))).toBe('boom');
  });
});
