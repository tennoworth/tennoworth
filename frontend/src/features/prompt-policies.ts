import type { PromptPolicy } from '../ui/prompts';

// Highest priority first: when several are due, a launch asks the first one
// that applies.
export const PRICE_SHARING_PROMPT: PromptPolicy = { id: 'price-sharing-v1', delayDays: 0, minLaunches: 1, gapDays: 14, maxAsks: 3 };
// Only for people already getting value.
export const SUPPORT_PROMPT: PromptPolicy = { id: 'support-v1', delayDays: 14, minLaunches: 5, gapDays: 60, maxAsks: 3 };
export const PROMPT_ORDER = [PRICE_SHARING_PROMPT, SUPPORT_PROMPT];
