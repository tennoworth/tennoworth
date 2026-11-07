<script lang="ts">
  import { t } from '../../i18n/i18n.svelte';
  import Rich from '../../ui/Rich.svelte';
</script>

  <!-- One Questions panel: "Is this safe?" used to be asked in the hand-off,
       the FAQ and Trust & safety. Trust answers sit in their own column, with
       the data-location question beside them; #faq and #trust both land here. -->
  <section data-shell class="faq questions" id="faq">
    <h2 data-shell>{t('faq.title')}</h2>
    <p data-shell class="trust-lede"><Rich text={t('faq.lede')} /></p>
    <div data-shell class="qcol" id="trust">
      <h3 data-shell>{t('faq.trust')}</h3>
      <details data-shell>
        <summary data-shell>{t('faq.safe.q')}</summary>
        <p data-shell><Rich text={t('faq.safe.a1')} /></p>
        <p data-shell><Rich text={t('faq.safe.a2')} /><a data-shell href="#trust">{t('faq.safe.link')}</a>{t('faq.safe.after')}</p>
      </details>
      <details data-shell>
        <summary data-shell>{t('faq.reads.q')}</summary>
        <p data-shell><Rich text={t('faq.reads.a1')} /></p>
        <p data-shell><Rich text={t('faq.reads.a2')} /></p>
        <p data-shell><Rich text={t('faq.reads.a3')} /></p>
      </details>
      <details data-shell>
        <summary data-shell>{t('faq.memory.q')}</summary>
        <p data-shell><Rich text={t('faq.memory.a')} /></p>
      </details>
      <details data-shell>
        <summary data-shell>{t('faq.wfm.q')}</summary>
        <p data-shell><Rich text={t('faq.wfm.a')} /></p>
      </details>
      <details data-shell>
        <summary data-shell>{t('faq.local.q')}</summary>
        <p data-shell><Rich text={t('faq.local.a1')} /></p>
        <p data-shell><Rich text={t('faq.local.a2')} /></p>
      </details>
      <details data-shell>
        <summary data-shell>{t('faq.verify.q')}</summary>
        <p data-shell><Rich text={t('faq.verify.a')} /><a data-shell href="https://github.com/tennoworth/tennoworth/blob/main/SECURITY.md" target="_blank" rel="noopener noreferrer">SECURITY.md</a>{t('faq.verify.after')}</p>
      </details>
      <details data-shell>
        <summary data-shell>{t('faq.risk.q')}</summary>
        <p data-shell><Rich text={t('faq.risk.a1')} /></p>
        <p data-shell><Rich text={t('faq.risk.a2')} /></p>
      </details>
      <details data-shell>
        <summary data-shell>{t('faq.data.q')}</summary>
        <p data-shell><Rich text={t('faq.data.a1')} /></p>
        <p data-shell><Rich text={t('faq.data.a2')} /></p>
      </details>
    </div>
    <div data-shell class="qcol">
      <h3 data-shell>{t('faq.using')}</h3>
      <details data-shell>
        <summary data-shell>{t('faq.count.q')}</summary>
        <p data-shell>{t('faq.count.a1')}<a data-shell href="/#community-usage">{t('faq.count.chart')}</a>.</p>
        <p data-shell>{t('faq.count.a2')}<a data-shell href="https://github.com/tennoworth/tennoworth/security/policy">{t('faq.count.policy')}</a>{t('faq.count.a2after')}</p>
      </details>
      <details data-shell>
        <summary data-shell>{t('faq.prices.q')}</summary>
        <p data-shell><Rich text={t('faq.prices.a')} /></p>
      </details>
      <details data-shell>
        <summary data-shell>{t('faq.list.q')}</summary>
        <p data-shell><Rich text={t('faq.list.a1')} /></p>
        <p data-shell><Rich text={t('faq.list.a2')} /></p>
      </details>
      <details data-shell>
        <summary data-shell>{t('faq.next.q')}</summary>
        <p data-shell><Rich text={t('faq.next.a1')} /></p>
        <p data-shell><Rich text={t('faq.next.a2')} /></p>
      </details>
      <details data-shell>
        <summary data-shell>{t('faq.sync.q')}</summary>
        <p data-shell><Rich text={t('faq.sync.a')} /></p>
      </details>
      <details data-shell>
        <summary data-shell>{t('faq.rivens.q')}</summary>
        <p data-shell><Rich text={t('faq.rivens.a')} /></p>
      </details>
      <details data-shell>
        <summary data-shell>{t('faq.support.q')}</summary>
        <p data-shell>{t('faq.support.a')}<a data-shell href="https://ko-fi.com/prowly" target="_blank" rel="noopener">ko-fi.com/prowly</a>{t('faq.support.after')}</p>
      </details>
    </div>
  </section>
