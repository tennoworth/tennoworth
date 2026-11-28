# Attributions

TennoWorth builds on other people's work. This file is the permanent record of
it: an entry stays when the work it credits has since been changed or
replaced, so the record of where something started is never lost.

## Riven roll guide: 44bananas

The Rivens view judges whether a riven's stats are a combination buyers look
for on its weapon. That roll guide
([`roll-guide.json`](rust/market-domain/src/rivens/roll-guide.json)) started
from **"good rolls for selling"**, a guide put together by **44bananas** for
Warframe update 44.1.2. It covers 419 weapons: the positive stats buyers want,
the negatives that do no harm, and notes on the builds that change either.

TennoWorth converted it once, on 2026-11-28, and has kept its own copy since.
The conversion corrected a few slips:

- Sporelacer's positive stats ended in a stray separator.
- The melee tab's Vinquibus is warframe.market's `vinquibus_melee`, not the gun.
- Recoil was written both `REC` and `RECOIL`; it is `REC` throughout.
- `ELEMENT` became the four base elements, Heat, Cold, Electricity and Toxin.

Entries whose `source` is `44bananas` are still as converted. Players change
the guide from here on, through pull requests and suggestions (see
[CONTRIBUTING.md](CONTRIBUTING.md#riven-roll-guide)), and a changed entry names
its new source.

## Item pictures, data and fonts

- Item pictures come from the Warframe Wiki, © Digital Extremes. The app
  credits them in Settings → Appearance → Item lists and in the site footer.
- Market data comes from warframe.market, and game data from Digital Extremes
  and warframestat.us. TennoWorth is a fan project and is not affiliated with
  any of them.
- The IBM Plex fonts are © IBM Corp. under the SIL Open Font License 1.1; see
  [`frontend/public/fonts/LICENSES.md`](frontend/public/fonts/LICENSES.md).
