<script lang="ts">
  // A player's rating of one riven's roll verdict. Each option states the
  // opposite of what the guide said about one of this riven's stats, so a
  // "no" carries something a maintainer can check. Ratings change nothing on
  // their own: the service proposes a change only when enough players who sold
  // rivens for the weapon agree.
  import Select from '../../ui/Select.svelte';
  import { humanError } from '../../contracts/errors';
  import { t } from '../../i18n/i18n.svelte';
  import type { RollSuggestion, RollVote } from '../../contracts/price-reports';
  import type { RivenRowRead, RollCheck } from '../../domain/rivens';

  type RollRead = Extract<RollCheck, { kind: 'read' }>;

  interface Props {
    /** WFM weapon slug. */
    weapon: string;
    read: RollRead;
    stats: RivenRowRead['stats'];
    /** This install's rating of the weapon this week, if any. */
    mine?: RollVote;
    sharing: 'on' | 'off' | 'unavailable';
    statName: (slug: string) => string;
    suggestUrl: string;
    onvote: (vote: RollVote) => Promise<void>;
  }
  let { weapon, read, stats, mine, sharing, statName, suggestUrl, onvote }: Props = $props();

  const uid = $props.id();
  let choosing = $state(false);
  let changing = $state(false);
  let choice = $state('');
  let sending = $state(false);
  let message = $state('');

  // A word joiner keeps the sign on the line with its stat.
  function claim(s: RollSuggestion): string {
    const name = (s.positive ? '+' : '−') + '\u2060' + statName(s.stat);
    if (s.positive) return t(s.good ? 'riven.rate.wanted' : 'riven.rate.addsLittle', { stat: name });
    return t(s.good ? 'riven.rate.harmless' : 'riven.rate.hurts', { stat: name });
  }

  const options = $derived.by(() => {
    const out: { value: string; label: string; suggest: RollSuggestion | null }[] = [];
    const seen = new Set<string>();
    for (const s of stats) {
      if (!s.slug) continue;
      const good = s.positive
        ? !(read.need.includes(s.slug) || read.pick.includes(s.slug))
        : read.harmless !== s.slug;
      const value = (s.positive ? '+' : '-') + s.slug;
      if (seen.has(value)) continue;
      seen.add(value);
      const suggest = { stat: s.slug, positive: s.positive, good };
      out.push({ value, label: claim(suggest), suggest });
    }
    out.push({ value: '', label: t('riven.rate.other'), suggest: null });
    return out;
  });

  function ask() {
    message = '';
    if (sharing !== 'on') {
      message = t('riven.rate.needsSharing');
      return;
    }
    choice = options[0].value;
    choosing = true;
  }

  async function send(agree: boolean) {
    message = '';
    if (sharing !== 'on') {
      message = t('riven.rate.needsSharing');
      return;
    }
    const suggest = agree ? null : (options.find((o) => o.value === choice)?.suggest ?? null);
    sending = true;
    try {
      await onvote({ weapon, verdict: read.verdict, agree, suggest });
      choosing = false;
      changing = false;
      message = t('riven.rate.thanks');
    } catch (e) {
      message = humanError(e);
    } finally {
      sending = false;
    }
  }
</script>

{#if sharing !== 'unavailable'}
  <div class="rate" data-testid="roll-rating">
    {#if mine && !changing}
      <p class="said">
        {mine.agree ? t('riven.rate.saidRight') : mine.suggest ? t('riven.rate.saidOff', { what: claim(mine.suggest) }) : t('riven.rate.saidWrong')}
        <button type="button" class="btn link xs" onclick={() => { changing = true; message = ''; }}>{t('riven.rate.change')}</button>
      </p>
      {#if !mine.agree && !mine.suggest}
        <p class="said"><a href={suggestUrl} target="_blank" rel="noopener noreferrer">{t('riven.rate.tellUs')}</a></p>
      {/if}
    {:else if choosing}
      <span class="q" id="{uid}-what">{t('riven.rate.whatsOff')}</span>
      <Select labelledby="{uid}-what" value={choice} {options} onchange={(next) => (choice = next)} />
      <span class="acts">
        <button type="button" class="btn xs" disabled={sending} onclick={() => send(false)}>{t('riven.rate.send')}</button>
        <button type="button" class="btn ghost xs" disabled={sending} onclick={() => (choosing = false)}>{t('riven.rate.cancel')}</button>
      </span>
    {:else}
      <span class="q">{t('riven.rate.question')}</span>
      <span class="acts">
        <button type="button" class="btn ghost xs" disabled={sending} onclick={() => send(true)}>{t('riven.rate.yes')}</button>
        <button type="button" class="btn ghost xs" disabled={sending} onclick={ask}>{t('riven.rate.no')}</button>
      </span>
    {/if}
    {#if message}<p class="msg" role="status">{message}</p>{/if}
  </div>
{/if}

<style>
  /* Quiet, under the reasons: the verdict is the content, the rating a margin note. */
  .rate { display: flex; flex-wrap: wrap; align-items: center; gap: var(--s1); margin-top: var(--s2); padding-top: var(--s1); border-top: 1px dotted var(--hairline); color: var(--muted); font: var(--text-caption)/1.25rem var(--font-body); }
  .acts { display: inline-flex; flex-wrap: wrap; gap: var(--s1); }
  .rate :global(.ui-select) { width: 100%; }
  .said, .msg { flex-basis: 100%; margin: 0; }
  .said a { color: var(--fg); }
  .msg { color: var(--fg); }
</style>
