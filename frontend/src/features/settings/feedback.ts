import { diagnosticCode, diagnosticError, diagnosticHttpStatus } from '../../contracts/errors';
import type { AutoScanStatus } from '../../contracts/desktop';
import type { UpdateStatus } from '../../contracts/update';

const issueUrl = 'https://github.com/tennoworth/tennoworth/issues/new';
export const improvementUrl = `${issueUrl}?template=improvement.yml`;
// Leave room for browser/OS URL handling; larger snapshots travel as files.
const maxIssueUrlLength = 6000;

export type WfmSession = 'logged_out' | 'locked' | 'unlocked' | 'unknown';

export interface FeedbackState {
  capturedAt: string;
  build: string;
  /** From the native health check, so the report has a version even when the updater never answers. */
  appVersion: string | null;
  platform: string | null;
  view: string;
  phase: string;
  scanning: boolean;
  scanError: unknown;
  autoScan: AutoScanStatus | null;
  wfmSession: WfmSession;
  wfmError: unknown;
  marketLoaded: boolean;
  marketError: unknown;
  theme: string;
  width: number;
  height: number;
}

type Operation = { operation: string; status: string; error: string | null };

export function feedbackSnapshot(state: FeedbackState, update: UpdateStatus | null, operation: Operation | null) {
  return {
    schema: 2,
    capturedAt: state.capturedAt,
    app: { version: update?.current_version ?? state.appVersion, build: state.build, platform: state.platform },
    surface: 'desktop',
    screen: state.view,
    inventory: {
      phase: state.phase,
      scanning: state.scanning,
      error: diagnosticError(state.scanError),
      httpStatus: diagnosticHttpStatus(state.scanError),
    },
    autoScan: state.autoScan
      ? {
        enabled: state.autoScan.enabled,
        gameRunning: state.autoScan.gameRunning,
        held: state.autoScan.held,
        error: diagnosticError(state.autoScan.lastError),
        httpStatus: diagnosticHttpStatus(state.autoScan.lastError),
      }
      : null,
    wfm: {
      session: state.wfmSession,
      error: diagnosticCode(state.wfmError) ?? diagnosticError(state.wfmError),
      httpStatus: diagnosticHttpStatus(state.wfmError),
    },
    market: { loaded: state.marketLoaded, error: diagnosticError(state.marketError) },
    update: update ? { checked: update.checked, available: update.available, support: update.support, version: update.version, lastOperation: operation } : { status: 'unavailable', lastOperation: operation },
    appearance: { theme: state.theme, width: state.width, height: state.height },
  };
}

export type FeedbackSnapshot = ReturnType<typeof feedbackSnapshot>;

const categoryText: Record<string, string> = {
  game_not_running: 'Warframe was not running',
  credentials_not_found: 'the game session was not found in memory',
  scan_busy: 'a scan was already running',
  endpoint_rejected: 'the inventory request was refused',
  permission_denied: 'permission was denied',
  numeric_out_of_range: 'a number was out of range',
  not_found: 'something was not found',
  timeout: 'the request timed out',
  signature_error: 'a signature check failed',
  connection_error: 'the connection failed',
  unclassified_error: 'an unrecognized error',
  bad_passphrase: 'the passphrase was wrong',
  signin_window_failed: 'the sign-in window failed',
  wfm: 'warframe.market returned an error',
  internal: 'an internal error',
};

export interface FeedbackProblem {
  area: string;
  summary: string;
}

/** The failures a report will carry, in words, so the reporter can see what it is about before leaving the app. */
export function feedbackProblems(snapshot: FeedbackSnapshot): FeedbackProblem[] {
  const describe = (category: string, status: number | null) => {
    const text = categoryText[category] ?? category.replaceAll('_', ' ');
    return status ? `${text} (HTTP ${status})` : text;
  };
  const problems: FeedbackProblem[] = [];
  if (snapshot.inventory.error) problems.push({ area: 'Scan', summary: describe(snapshot.inventory.error, snapshot.inventory.httpStatus) });
  if (snapshot.autoScan?.error) problems.push({ area: 'Automatic scan', summary: describe(snapshot.autoScan.error, snapshot.autoScan.httpStatus) });
  if (snapshot.wfm.error) problems.push({ area: 'warframe.market sign-in', summary: describe(snapshot.wfm.error, snapshot.wfm.httpStatus) });
  if (snapshot.market.error) problems.push({ area: 'Market data', summary: describe(snapshot.market.error, null) });
  const failed = snapshot.update.lastOperation;
  if (failed?.status === 'failed') problems.push({ area: 'Update', summary: describe(failed.error ?? 'unclassified_error', null) });
  return problems;
}

/** Prefill values for bug-report.yml; each key is a field id in that form. */
export function bugReportUrl(fields: { surface: string; version: string; os?: string | null; attachments?: string }) {
  const params = new URLSearchParams({ template: 'bug-report.yml', surface: fields.surface, version: fields.version });
  if (fields.os) params.set('operating-system', fields.os);
  if (fields.attachments) params.set('attachments', fields.attachments);
  return `${issueUrl}?${params}`;
}

export function feedbackLink(snapshot: FeedbackSnapshot, includeState: boolean) {
  const os = snapshot.app.platform === 'windows' ? 'Windows' : snapshot.app.platform === 'linux' ? 'Linux' : null;
  const fields = { surface: 'Desktop app', version: `${snapshot.app.version ?? 'Unknown version'} (build ${snapshot.app.build})`, os };
  if (includeState) {
    const url = bugReportUrl({ ...fields, attachments: `App-state snapshot (error categories only):\n\n\`\`\`json\n${JSON.stringify(snapshot, null, 2)}\n\`\`\`` });
    if (url.length <= maxIssueUrlLength) return { url, needsAttachment: false };
  }
  return { url: bugReportUrl(fields), needsAttachment: includeState };
}
