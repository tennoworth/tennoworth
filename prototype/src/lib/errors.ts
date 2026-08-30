// The one place that turns a caught `unknown` into a displayable string.
// Used for plain fetch/parse/network failures across the app. NOT for
// AssistantError - assistant.ts's assistantErrorMessage() understands its
// typed error codes and gives more specific copy; this is the fallback for
// everything else.
export function humanError(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
