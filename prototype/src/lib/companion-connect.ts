// The companion-connection verification logic — the part of App.svelte's
// verifyCompanion/probeLoopbackPermission that's actual decision-making
// (classify what a ping + pending-plan check mean, detect a browser-level
// permission block) rather than $state orchestration. The stale-async guard
// (verifyGen) and every $state write stay in App.svelte, same as the rest of
// this file's Svelte-only wiring — only the classification itself moves here,
// so it's unit-testable independent of the component.

import { pingCompanion, getPendingPlan, CompanionUnreachableError } from './companion';
import type { CompanionConfig, PendingPlan } from './types';

export type CompanionVerifyResult =
  | { status: 'connected'; platform: string | null; assistant: boolean; pendingPlan: PendingPlan | null }
  | { status: 'unreachable'; message: string }
  | { status: 'error'; message: string };

/**
 * Ping the companion, then confirm the saved token with one authed call
 * (getPendingPlan — 404/null on "no plan", but throws on a bad token).
 * `/health` alone can't tell a live server with the WRONG token from a
 * correctly-configured one, so 'connected' is never returned without this
 * second call succeeding.
 */
export async function verifyCompanionConnection(config: CompanionConfig): Promise<CompanionVerifyResult> {
  try {
    const r = await pingCompanion(config);
    // A network reject HERE is a raw TypeError (not CompanionUnreachableError,
    // which only /health throws), so it never classifies as 'unreachable' —
    // /health already proved reachability.
    const pendingPlan = await getPendingPlan(config);
    return {
      status: 'connected',
      platform: r?.platform ?? null,
      assistant: r?.assistant === true,
      pendingPlan,
    };
  } catch (e: any) {
    // Only the /health fetch rejecting on an HTTPS origin means "blocked or
    // serve-down" — the case the cross-view banner exists for. Everything
    // else (bad token, non-OK HTTP, a post-/health network blip) is 'error'.
    if (e instanceof CompanionUnreachableError && location.protocol === 'https:') {
      return { status: 'unreachable', message: e.message };
    }
    const msg = e?.message || String(e);
    return {
      status: 'error',
      message: /401|token/i.test(msg)
        ? 'Token rejected — re-copy the full URL from the serve output (the token changes every time you restart serve).'
        : msg,
    };
  }
}

/**
 * Optional supplemental precision for the unreachable banner. If the browser
 * exposes the loopback/local-network permission and reports it 'denied', we
 * can state Chrome HAS blocked the request rather than "may have". An
 * unsupported browser (query throws) resolves false — the copy stays generic.
 */
export async function probeLoopbackDenied(): Promise<boolean> {
  try {
    let status;
    try {
      status = await navigator.permissions.query({ name: 'loopback-network' } as any);
    } catch {
      status = await navigator.permissions.query({ name: 'local-network-access' } as any);
    }
    return status?.state === 'denied';
  } catch {
    // Permissions API absent or the name unsupported — leave the copy generic.
    return false;
  }
}
