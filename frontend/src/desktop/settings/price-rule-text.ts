// A price rule in words ("90-day median +1p"), for Settings and the My orders
// explanations. Kept out of domain/, which does not import i18n.
import { t } from '../../i18n/i18n.svelte';
import type { PriceBasis, PriceRule } from '../../domain/price-rule';

export function basisName(basis: PriceBasis): string {
  return t(`settings.basis.${basis}`);
}

export function ruleText(rule: PriceRule): string {
  const basis = basisName(rule.basis);
  if (rule.offset === 0) return basis;
  return t(rule.offset > 0 ? 'settings.ruleUp' : 'settings.ruleDown', { basis, offset: String(Math.abs(rule.offset)) });
}
