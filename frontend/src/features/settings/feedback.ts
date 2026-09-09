import { diagnosticError } from '../../contracts/errors';
import type { UpdateStatus } from '../../contracts/update';

const issueUrl = 'https://github.com/tennoworth/tennoworth/issues/new';
// Leave room for browser/OS URL handling; larger snapshots travel as files.
const maxIssueUrlLength = 6000;

export interface FeedbackState {
  capturedAt: string;
  build: string;
  platform: string | null;
  view: string;
  phase: string;
  scanning: boolean;
  scanError: unknown;
  marketLoaded: boolean;
  marketError: unknown;
  theme: string;
  width: number;
  height: number;
}

export function feedbackSnapshot(state: FeedbackState, update: UpdateStatus | null, operation: { operation: string; status: string; error: string | null } | null) {
  return {
    schema: 1,
    capturedAt: state.capturedAt,
    app: { version: update?.current_version ?? null, build: state.build, platform: state.platform },
    screen: state.view,
    inventory: { phase: state.phase, scanning: state.scanning, error: diagnosticError(state.scanError) },
    market: { loaded: state.marketLoaded, error: diagnosticError(state.marketError) },
    update: update ? { checked: update.checked, available: update.available, support: update.support, version: update.version, lastOperation: operation } : { status: 'unavailable', lastOperation: operation },
    appearance: { theme: state.theme, width: state.width, height: state.height },
  };
}

export function feedbackLink(snapshot: ReturnType<typeof feedbackSnapshot>, includeState: boolean) {
  const params = new URLSearchParams({ template: 'bug-report.yml', version: `${snapshot.app.version ?? 'Unknown version'} (build ${snapshot.app.build})` });
  if (snapshot.app.platform === 'windows') params.set('operating-system', 'Windows');
  if (snapshot.app.platform === 'linux') params.set('operating-system', 'Linux');
  if (includeState) params.set('attachments', `App-state snapshot (error categories only):\n\n\`\`\`json\n${JSON.stringify(snapshot, null, 2)}\n\`\`\``);
  const url = `${issueUrl}?${params}`;
  if (url.length <= maxIssueUrlLength) return { url, needsAttachment: false };
  params.delete('attachments');
  return { url: `${issueUrl}?${params}`, needsAttachment: includeState };
}
