<script lang="ts">
  import Select from '../../ui/Select.svelte';
  import { busy } from '../../ui/keyboard-actions';
  import { t } from '../../i18n/i18n.svelte';
  import { humanError } from '../../contracts/errors';
  import { DEFAULT_PRICE_RULE, MAX_PRICE_OFFSET, MIN_PRICE_OFFSET, priceFromRule, type PriceBasis, type PriceRule } from '../../domain/price-rule';
  import { isDefaultRule, type PricingController } from './pricing.svelte';
  import { basisName } from './price-rule-text';

  let { pricing }: { pricing: PricingController } = $props();

  const BASES: PriceBasis[] = ['lowest-ask', 'low5-avg', 'low20-avg', 'avg-48h', 'median-90d'];
  let options = $derived(BASES.map((basis) => ({ value: basis, label: basisName(basis) })));
  let saving = $state(false);
  let error = $state('');
  let offsetError = $state('');

  // A worked example on a round figure says what the rule does more plainly
  // than a description of the arithmetic.
  const EXAMPLE_BASE = 20;
  let example = $derived(t('settings.priceRuleExample', {
    basis: basisName(pricing.rule.basis),
    base: String(EXAMPLE_BASE),
    price: String(priceFromRule(pricing.rule, { lowestAsk: EXAMPLE_BASE, low5Avg: EXAMPLE_BASE, low20Avg: EXAMPLE_BASE, avg48h: EXAMPLE_BASE, median90d: EXAMPLE_BASE })),
  }));

  async function save(next: PriceRule) {
    error = '';
    saving = true;
    try { await pricing.setRule(next); } catch (e) { error = humanError(e); } finally { saving = false; }
  }

  function changeOffset(input: HTMLInputElement) {
    const value = Number(input.value);
    if (!Number.isInteger(value) || value < MIN_PRICE_OFFSET || value > MAX_PRICE_OFFSET) {
      offsetError = t('settings.priceOffsetRange', { min: String(MIN_PRICE_OFFSET), max: String(MAX_PRICE_OFFSET) });
      input.value = String(pricing.rule.offset);
      return;
    }
    offsetError = '';
    if (value !== pricing.rule.offset) void save({ ...pricing.rule, offset: value });
  }
</script>

<div class="ui-setting-row">
  <div class="ui-setting-copy">
    <strong id="price-rule-label">{t('settings.priceRule')}</strong>
    <p>{t('settings.priceRuleHelp')}</p>
    <p class="example" data-testid="price-rule-example">{example}</p>
    {#if offsetError}<p class="ui-notice" data-tone="warn" role="alert">{offsetError}</p>{/if}
    {#if error}<p class="ui-notice" data-tone="bad" role="alert">{t('settings.priceRuleFailed', { error })}</p>{/if}
  </div>
  <div class="ui-setting-control">
    <Select id="price-rule-basis" labelledby="price-rule-label" value={pricing.rule.basis} {options} disabled={saving} onchange={(basis) => void save({ ...pricing.rule, basis })} />
    <label class="offset">
      <span>{t('settings.priceOffset')}</span>
      <input id="price-rule-offset" class="ui-input" type="number" step="1" min={MIN_PRICE_OFFSET} max={MAX_PRICE_OFFSET} value={pricing.rule.offset} use:busy={saving} onchange={(event) => changeOffset(event.currentTarget)} />
      <span class="unit">p</span>
    </label>
    {#if !isDefaultRule(pricing.rule)}
      <button type="button" class="btn xs ghost" use:busy={saving} onclick={() => { offsetError = ''; void save(DEFAULT_PRICE_RULE); }}>{t('settings.priceRuleReset')}</button>
    {/if}
  </div>
</div>

<style>
  .example { color: var(--fg); }
  .offset { display: inline-flex; align-items: center; gap: var(--s2); font-size: var(--text-control); }
  .offset input { width: 5rem; font-family: var(--font-mono); text-align: right; }
  .unit { font-family: var(--font-mono); color: var(--muted); }
</style>
