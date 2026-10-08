import type { PromptPolicy } from '../ui/prompts';

// Highest priority first: when several are due, a launch asks the first one
// that applies.
export const PRICE_SHARING_PROMPT: PromptPolicy = { id: 'price-sharing-v1', delayDays: 0, minLaunches: 1, gapDays: 14, maxAsks: 3 };
// First after an update has shown its notes, then once a year for as long as
// the app is used. Clicking through retires it.
export const SUPPORT_PROMPT: PromptPolicy = { id: 'support-v1', delayDays: 0, minLaunches: 1, gapDays: 365, maxAsks: Number.POSITIVE_INFINITY, firstOnUpdate: true };
export const PROMPT_ORDER = [PRICE_SHARING_PROMPT, SUPPORT_PROMPT];
