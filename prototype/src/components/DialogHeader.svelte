<script lang="ts">
  // Shared header layout for modal dialogs - close-button styling and title
  // treatment. The parent dialog owns aria-labelledby; titleId keeps the two
  // in sync.
  interface Props {
    titleId: string;
    title: string;
    onclose: () => void;
  }
  let { titleId, title, onclose }: Props = $props();
</script>

<header>
  <h2 id={titleId}>{title}</h2>
  <button class="x" onclick={onclose} aria-label="Close">×</button>
</header>

<style>
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 18px;
    border-bottom: 1px solid var(--border);
    /* No-op outside a flex parent; keeps the header from being squished when
       one is (modal dialogs are flex columns). */
    flex-shrink: 0;
  }
  h2 {
    margin: 0;
    font-size: 13px;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--accent);
    font-weight: 600;
  }
  .x {
    background: transparent;
    border: 1px solid var(--border);
    color: var(--muted);
    font-size: 16px;
    line-height: 1;
    width: 26px;
    height: 26px;
    border-radius: var(--radius-ctl);
    cursor: pointer;
  }
  .x:hover {
    color: var(--fg);
  }
</style>
