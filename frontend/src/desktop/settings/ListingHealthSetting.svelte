<script lang="ts">
  import { busy } from '../../ui/keyboard-actions';
  import { t } from '../../i18n/i18n.svelte';
  import { humanError } from '../../contracts/errors';
  import { HEALTH_KINDS, MAX_HEALTH_TOLERANCE, type HealthKind } from '../../domain/listing-health';
  import type { PricingController } from './pricing.svelte';

  let { pricing }: { pricing: PricingController } = $props();

  let saving = $state(false);
  let error = $state('');
  let toleranceError = $state('');

  async function save(next: { tolerance: number; muted: HealthKind[] }): Promise<boolean> {
    error = '';
    saving = true;
    try {
      await pricing.setHealth(next);
      return true;
    } catch (e) {
      error = humanError(e);
      return false;
    } finally {
      saving = false;
    }
  }

  function changeTolerance(input: HTMLInputElement) {
    const value = Number(input.value);
    const current = pricing.healthPrefs.tolerance;
    if (input.value.trim() === '' || !Number.isInteger(value) || value < 0 || value > MAX_HEALTH_TOLERANCE) {
      toleranceError = t('settings.priceOffsetRange', { min: '0', max: String(MAX_HEALTH_TOLERANCE) });
      input.value = String(current);
      return;
    }
    toleranceError = '';
    if (value !== current) void save({ tolerance: value, muted: pricing.healthPrefs.muted }).then((ok) => { if (!ok) input.value = String(pricing.healthPrefs.tolerance); });
  }

  async function toggle(kind: HealthKind, input: HTMLInputElement) {
    const muted = pricing.healthPrefs.muted.filter((k) => k !== kind);
    if (!input.checked) muted.push(kind);
    if (!(await save({ tolerance: pricing.healthPrefs.tolerance, muted }))) input.checked = !pricing.healthPrefs.muted.includes(kind);
  }
</script>

<div class="ui-setting-row">
  <div class="ui-setting-copy">
    <label for="health-tolerance">{t('settings.healthTolerance')}</label>
    <p>{t('settings.healthToleranceHelp')}</p>
    {#if toleranceError}<p class="ui-notice" data-tone="warn" role="alert">{toleranceError}</p>{/if}
  </div>
  <div class="ui-setting-control">
    <span class="tolerance">
      <input id="health-tolerance" class="ui-input" type="number" step="1" min="0" max={MAX_HEALTH_TOLERANCE} value={pricing.healthPrefs.tolerance} use:busy={saving} onchange={(event) => changeTolerance(event.currentTarget)} />
      <span class="unit">p</span>
    </span>
  </div>
</div>
<div class="ui-setting-row">
  <div class="ui-setting-copy">
    <strong id="health-checks-label">{t('settings.healthChecks')}</strong>
    <p>{t('settings.healthChecksHelp')}</p>
    {#if error}<p class="ui-notice" data-tone="bad" role="alert">{t('settings.healthFailed', { error })}</p>{/if}
  </div>
  <fieldset class="ui-setting-control checks" aria-labelledby="health-checks-label">
    {#each HEALTH_KINDS as kind (kind)}
      <label class="ui-setting-check">
        <input type="checkbox" checked={!pricing.healthPrefs.muted.includes(kind)} use:busy={saving} onchange={(event) => void toggle(kind, event.currentTarget)} />
        <span>{t(`settings.health.${kind}`)}</span>
      </label>
    {/each}
  </fieldset>
</div>

<style>
  .tolerance { display: inline-flex; align-items: center; gap: var(--s2); font-size: var(--text-control); }
  .tolerance input { width: 5rem; font-family: var(--font-mono); text-align: right; }
  .unit { font-family: var(--font-mono); color: var(--muted); }
  .checks { flex-direction: column; align-items: flex-start; gap: 0; margin: 0; padding: 0; border: 0; }
</style>
