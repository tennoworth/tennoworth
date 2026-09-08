# Self-hosted repository layout transition

The source roots change from `prototype/` and `companion/` to `frontend/`
and `rust/`. Public URLs, release asset names, application identity, and data
formats do not change. Local source builds need only the new paths; the
self-host kit needs the coordinated procedure below.

This is an operator procedure, not contributor setup. Production execution
requires explicit authorization. Do not promote the rename while an old
installed `pull-app.sh` can run unattended: it restores live snapshots into
the old source directory after advancing the checkout.

## Before promotion

1. Record the deployed commit, web/scraper asset stamps, binary checksum, and
   enabled timer state. Confirm that the authoritative remote is named `github`
   and points to the intended GitHub repository. If an existing deployment uses
   another name for that same remote, rename it or pass `REMOTE` explicitly.
2. Stop `wfm-app-pull.timer`, `wfm-web-pull.timer`, `wfm-scrape-pull.timer`, and
   `wfm-scrape.timer`. Wait for all four corresponding services to become
   inactive or failed. Do not kill an in-flight scrape or replace a running
   shell script. The puller independently rejects an activating scrape.
3. Make a private backup outside the deployment directory, with `umask 077`.
   Preserve the checkout including its Git state, `bin/`, installed scripts,
   asset stamps, and the deployed Caddy configuration. Include the live
   market/catalog pair, definitions (including operator overrides), history, CSV/checkpoints, and current web bundle.
   Check that the backup can be read before continuing.
4. Install the migration-aware `deploy/pull-app.sh` from the reviewed change
   **before** advancing the deployed checkout. Stage it beside the installed
   script and rename it into place; never truncate a running script. Its
   ordinary mode refuses a source-root transition before clearing live data.

Keep the site on its existing Caddy configuration during preparation. No
contributor needs access to this host or its credentials.

## Transition

After authorized production promotion, wait for the matching `build-web` and
`build-scrape` workflows to succeed. Confirm their source commit before using
the rolling artifacts. Keep all deployment timers stopped.

From the deployment account, run the installed migration-aware puller:

```sh
APP=/srv/wfm/app DEPLOY_ROOT=/srv/wfm REMOTE=github BRANCH=main \
  /srv/wfm/pull-app.sh --migrate-layout
```

The puller backs up logical artifact names outside the checkout, advances only
by fast-forward, and restores to the layout actually present afterward. A
failed fast-forward restores the original layout. Each restored JSON file is
replaced atomically, with the catalog before the market generation anchor.
CSV and history remain production-owned. The previously served web bundle and
old public-data paths, including definitions, remain available while Caddy still uses the old root. Definitions are preserved during this layout transition; ordinary source updates continue applying tracked definition changes.
If restoration fails, the puller reports the retained recovery directory;
recover it before restarting any timers.

Then:

1. Compare checksums of the live market/catalog pair, definitions, history, and CSV with the
   pre-transition copies. They must be unchanged by the source update.
2. Run the newly installed `pull-scrape.sh` and `pull-web.sh`. Verify the
   installed binary and bundle came from the successful matching workflows.
   Do not let a binary built for the old source paths run the new checkout.
3. Install `deploy/Caddyfile`, validate it with `caddy validate --config
   /etc/caddy/Caddyfile --adapter caddyfile`, and reload Caddy. Install any
   changed service/unit copies and run `systemctl daemon-reload`.
4. Check the site, assets, `/market.json`, `/wfstat-catalog.json`,
   `/history.json` when present, and `/definitions.json`. Check CSP and verify
   that the served snapshots are the live copies, not the bundled fallback.
5. Run `/srv/wfm/bin/wfm-scrape build` from the deployment checkout using its existing
   CSV. This performs upstream enrichment; it must complete and publish a
   valid pair before scheduled scraping resumes. Keep the backup until the
   next complete scheduled scrape/build also succeeds.
6. Restore the previously enabled timer state. Inspect the service logs and
   artifact timestamps after the next cycle.

The old `prototype/public` and `prototype/dist` copies are temporary rollback
material. Remove them only after verifying that Caddy, installed scripts,
units, and the running pipeline use `frontend/`, and after checking the old
directory contains no unrelated files. Do not run a blanket `git clean` on a
production checkout.

## Rollback

Stop the four timers again and wait for in-flight services to settle. Save
any live snapshots, definitions, history, CSV, and checkpoints produced since the original
backup into another private directory. Retain the failed deployment separately
for inspection.

Restore the previous checkout, scraper binary, installed scripts, web bundle,
asset stamps, and Caddy configuration as one matching set from the verified
backup. Restore the newest saved live artifacts into that layout's public
folder, catalog first and market last. Data formats are unchanged by this
reorganization. Validate and reload Caddy, verify the old paths and served
snapshot checksums, then restore the prior timer state.

A rollback of the deployed checkout does not authorize force-pushing `main`
or `develop`. Keep automatic pulls paused until the source rollback or
replacement deployment has been reviewed.

## Local regression coverage

`bun test scripts/deploy-layout.test.ts` runs isolated local Git repositories
with a stub service-state query. It covers ordinary updates, explicit layout
migration, preservation of newer live data and the served bundle, refusal
without migration mode, failed-fast-forward recovery, and an activating
scrape. It never contacts a production host or changes system services.
