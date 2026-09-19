# Reddit pain points, September 2026

What Warframe players actually complain about in places where "what should I
sell?" comes up, which of those complaints the app already answers, and what
was **not** established. The research revision is `03fe890`.

This is a findings record, not a marketing plan. Positioning and channel copy
live in the marketing plan; this document only states what the community says
and what it implies for the product.

## Method

Reddit blocks this machine for both HTML and `.json` when logged out (403, or a
page reading "You've been blocked by network security"), so the sweep used three
channels with different reach:

| Channel | Gives | Loses |
|---|---|---|
| Arctic Shift archive API | Comment text, per-year post counts, deep history | Sample, not a census; no scores; slow queries time out under load |
| Logged-in browser session | Post search with scores and comment counts, per-thread comments | `type=comment` is ignored, so it never returns comments from search |
| Reddit public RSS search | Works logged out; post titles, permalinks, bodies, dates | 21-28 entries per query, no scores, per-subreddit rate limiting |

Collection ran 2026-09-18 to 2026-09-19 (UTC).

### Queries

Archive post search, for example:

```
https://arctic-shift.photon-reddit.com/api/posts/search
  ?subreddit=wartrade&query=what+should+i+sell&after=2024-01-01&limit=100
  &fields=title,selftext,score,created_utc,id,num_comments
```

Archive comment search, for example:

```
https://arctic-shift.photon-reddit.com/api/comments/search
  ?subreddit=warframe&body=alecaframe&after=2024-01-01&limit=100
  &fields=body,score,created_utc,id,link_id
```

Archive volume aggregation, by calendar year:

```
https://arctic-shift.photon-reddit.com/api/posts/search/aggregate
  ?subreddit=wartrade&title=price+check&aggregate=created_utc&frequency=year
  &after=2022-01-01
```

Logged-in browser search and thread reads:

```
/r/<sub>/search.json?q=<query>&restrict_sr=1&sort=top&t=year&limit=100
/comments/<id>.json?limit=200&sort=top
```

RSS, which reproduces the post-level sweep without any login:

```
/r/<sub>/search.rss?q=<query>&restrict_sr=1&sort=top&t=year&limit=100
```

### Re-running it

The post-level sweep needs no credentials. Sequentially, about one query every
eight seconds, retrying HTTP 429 after 25-55 s:

```python
import urllib.request, urllib.parse, xml.etree.ElementTree as ET, time
NS = {"a": "http://www.w3.org/2005/Atom"}
UA = {"User-Agent": "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 Chrome/141 Safari/537.36"}

def search(sub, query, sort="top", t="year"):
    url = "https://www.reddit.com/r/%s/search.rss?%s" % (
        sub, urllib.parse.urlencode({"q": query, "restrict_sr": 1, "sort": sort, "t": t, "limit": 100}))
    body = urllib.request.urlopen(urllib.request.Request(url, headers=UA), timeout=40).read()
    return ET.fromstring(body).findall("a:entry", NS)

for sub, query in [("Warframe", "overwolf"), ("wartrade", "price check")]:
    for entry in search(sub, query)[:10]:
        print(entry.findtext("a:published", "", NS)[:10],
              entry.findtext("a:title", "", NS)[:90],
              entry.find("a:link", NS).get("href"))
    time.sleep(8)
```

Coverage loss when re-running logged out: no upvote scores, no comment counts,
no comment-level keyword search, and at most ~25 posts per query. Comment
evidence can only be recovered from the archives or by reading individual
threads.

### Known defects in this evidence

- **Archive counts are not a census.** They are a sample of what the archive
  retained, and they are used here for direction only.
- **The 2025 buckets are internally inconsistent** and are excluded from every
  trend claim. r/wartrade post titles matching `price check`: 1,371 (2022),
  3,351 (2023), 7,652 (2024), 982 (2025). A four-fold drop in one year is an
  artifact until someone checks it against another source.
- **Permalinks were not individually re-opened.** Archive-derived links are
  built from `link_id` plus comment id; the logged-in and RSS links are
  Reddit's own.
- **Complementary quotes are one-sided.** Thread comment pulls take the top few
  by score from `sort=top`, which surfaces the confident end of the
  conversation, not the typical one.

## Findings

Confidence labels describe the evidence, not the severity: *very high* means
multiple independent threads plus a volume signal; *high* means several threads
or one large thread; *medium* means one thread or a handful of comments.

### F1 - Riven pricing is the trading community's default question, and its default answer is "nobody knows" (very high)

r/WarframeRiven operates as a price-request queue. Representative posts:
"I just unveiled this" - *"i have no idea how much is it and how much i can get
for it"* (981) [1v1x7e8](https://www.reddit.com/r/WarframeRiven/comments/1v1x7e8/i_just_unveiled_this/);
"Havent messed with rivens before, how is this" - *"How much could this be
worth and where could i sell it"* (875) [1q8jr27](https://www.reddit.com/r/WarframeRiven/comments/1q8jr27/havent_messed_with_rivens_before_how_is_this/),
whose answers ran "7k, possibly 9k if you find the right buyer" and later
"Update. It sold for 13k"; "how much would this burston riven go?" - *"no idea
how much i should ask in trading/wfm"* (549) [1pnpzjw](https://www.reddit.com/r/WarframeRiven/comments/1pnpzjw/how_much_would_this_burston_riven_go/).

The listed prices are treated as unreliable in both directions: *"Wfm has
identical ones for absolutely insane amounts and idk if thats realistic"*
[1pottq4](https://www.reddit.com/r/wartrade/comments/1pottq4/qpc_fair_price/);
*"The only similar one at WF Market sits at 10K plat, and I know that's a
scam"* [1vdudqp](https://www.reddit.com/r/wartrade/comments/1vdudqp/wtspcriven_corufell_godroll/).

Implied gap: a posted ask is not evidence that anything sells at it, and
nothing in the current data says how thin the asking side is.

### F2 - An announced change moves prices before anything ships (high)

*"Albrecht Entrati strikes the market again - Arcane Hot Shot lost more than
half its value since the announcement that it would be added to Arcane
Dissolution on Deimos"* (6,309) [1o25qng](https://www.reddit.com/r/Warframe/comments/1o25qng/albrecht_entrati_strikes_the_market_again/).

The live riven changes produced the same behaviour: *"DO NOT BUY GROLL RIVENS
NOW, BIG CHANGES COMING PRICES GONNA GO DOWN MASSIVELY"*
[1vxsvga](https://www.reddit.com/r/wartrade/comments/1vxsvga/);
*"it's best to sell this thing now for a guaranteed 600-800 plat rather than
wait for the update which will most likely makes this way cheaper"*
[1vyk9o7](https://www.reddit.com/r/wartrade/comments/1vyk9o7/).
Devshorts 115 and 116 covered riven system changes.

Implied gap: the hold-or-sell advisor reasons over vault, Resurgence and Baro
calendars plus a year of medians. It has no notion of an announced change, and
it currently produces no verdict at all for riven rows.

### F3 - Reaching a paying buyer is the friction, not pricing (high)

*"whats a reasonable and fast selling price? Just got this recently and it
seems hard to find a person to buy this although it's a good secondary"*
[1ox0ax5](https://www.reddit.com/r/wartrade/comments/1ox0ax5/prcwtspc_whats_a_reasonable_and_fast_selling_price/);
*"clanmates told me it's worth AL 2k but i'm having no luck selling this, all
the ppl in trade chat claim it's too high of a price"*
[1pplxhm](https://www.reddit.com/r/WarframeRiven/comments/1pplxhm/riven_pricing/).
Bulk divestments are common: *"Bulk rivens, want everything not crossed out
gone. Make an offer, bulk trades will be cheaper"*
[1poeag6](https://www.reddit.com/r/wartrade/comments/1poeag6/wts_pc_riven_bulk_rivens/).

The app compares a stack against visible buy orders and reports covered units,
bid value and the gap against the ask. Contact itself is deliberately left to
warframe.market (see K2/K3).

### F4 - Visible prices are distrusted, and mispricing cuts both ways (high)

*"Got scammed out of a Legendary Core as a new player... A player offered me
20p for it and I thought 'hey, free plat'. Turns out it goes for around
100-300p on warframe.market"* (3,804) [1sonq6u](https://www.reddit.com/r/Warframe/comments/1sonq6u/got_scammed_out_of_a_legendary_core_as_a_new/);
*"i have no clue how to price it so i don't get scammed or accidentally scam
someone else"* [1tbsr9r](https://www.reddit.com/r/wartrade/comments/1tbsr9r/q_pc/);
*"Is my first time selling a lich, i dont wanna be scammed for someone
experienced in the chat"* [1so7jqv](https://www.reddit.com/r/wartrade/comments/1so7jqv/).

### F5 - "I have a ton of parts and I don't know what to do with them" (very high)

The clearest statement of the product's own question, from the main subreddit:
*"How do you manage your inventory to sell prime parts? I often crack a bunch
of relics, picking what's worth the most. But then, I'm finding myself with a
ton of parts and I don't really know how to organize so I can track which sets
I have and what is worth."* [1viycih](https://www.reddit.com/r/Warframe/comments/1viycih/how_do_you_manage_your_inventory_to_sell_prime/)
(2026-08-08).

Also: *"I'm a total newbie... but trading? No clue. I grind stuff but have
zero idea what's valuable or what prices to set"* (171) [1r849l6](https://www.reddit.com/r/Warframe/comments/1r849l6/new_player_terrified_of_alecframe_even_though_it/).

### F6 - The no-Overwolf, Linux and ban-fear wedge is real, and now crowded (very high)

Users leave the incumbent and then need something else:
*"I have removed Aleca from my PC in fear of getting banned by DE for using
Overwolf, though now i need something to help scratch Aleca's itch that isn't
Bannable. I mostly used Aleca to see what parts were missing"*
[1v09qnr](https://www.reddit.com/r/Warframe/comments/1v09qnr/uninstalling_alecaframe/) (2026-07-18);
*"Are there any non-Overwolf alternatives to Alecaframe?... I would like to
avoid Overwolf (both because of its bloatware properties and because of its
political affiliations)"* [1py0897](https://www.reddit.com/r/Warframe/comments/1py0897/are_there_any_nonoverwolf_alternatives_to/);
*"Overwolf on Linux?"* [1pfk80g](https://www.reddit.com/r/Warframe/comments/1pfk80g/overwolf_on_linux/);
*"Linux alternative to Alecaframe?"* [1uda3pe](https://www.reddit.com/r/Warframe/comments/1uda3pe/linux_alternative_to_alecaframe/).

The 1,350-point warning thread [1rp21jq](https://www.reddit.com/r/Warframe/comments/1rp21jq/quick_warning_to_all_of_you_using_alecaframe_or/)
carries the same tension in its comments: *"keeping track of my relics and
prime parts is a royal pain without it, i simply would avoid opening relics
even more than i do if i didn't had it"*, *"it's in a grey area that a lot of
people, myself included, just don't feel comfortable in"*, and *"DE has
refused to say alecaframe is a blessed app"*.

The niche is no longer empty. Present in the same conversations: TennoScope (a
FOSS Rust Linux advisor, pitched in [1uda3pe](https://www.reddit.com/r/Warframe/comments/1uda3pe/linux_alternative_to_alecaframe/)),
TennoHub [1u3clpe](https://www.reddit.com/r/Warframe/comments/1u3clpe/working_on_a_nooverwolf_warframe_companion_app/),
WFHub (macOS, EE.log plus OCR reward overlay, market/farm/riven advisors,
inventory scan) [1sjn04j](https://www.reddit.com/r/Warframe/comments/1sjn04j/tool_wfhub_macos_desktop_app_for_warframe/),
VoidStonks [1s5fcya](https://www.reddit.com/r/Warframe/comments/1s5fcya/voidstonks_warframe_farm_market_optimization_tool/),
warframelive [1sbdv8p](https://www.reddit.com/r/Warframe/comments/1sbdv8p/yet_another_tracker_warframelive_is_live/),
Cephalon Kronos, a Baro notification app [1rx84ko](https://www.reddit.com/r/Warframe/comments/1rx84ko/i_spent_3_years_building_an_app_that_sends_you/),
and riven tools including morrowshore and wfhelper [1v1ruto](https://www.reddit.com/r/Warframe/comments/1v1ruto/any_good_ways_to_rate_a_riven_besides_aleca/).
"Does not need Overwolf" is a baseline, not a differentiator.

### F7 - The community has already written the feature list (high)

From the comments of the no-Overwolf app thread
[1u3clpe](https://www.reddit.com/r/Warframe/comments/1u3clpe/working_on_a_nooverwolf_warframe_companion_app/):

- *"recommend me what relic reward to pick, if I already have it or other items
  in the set, tell me the average market price for what I get when cracking it,
  notify me when there's a trade request in chat, sync with warframe.market"*
- *"Warframe.market price check on prime parts found in relics... mastery
  tracker and refined inventory search"*
- *"The BIGGEST reason why i use the overwolf program is their integration with
  warframe.market. Directly on the app i can list items to the market... But
  most importantly: when I trade, this program auto marks the listing on
  wf.market as complete"* - already shipped here, from `EE.log` trade detection
- *"Very worrisome that most people in this thread are fine with an app that
  reads your screen not being open source"*

Requests not covered today: a trade-request notification from chat, and
mastery-aware reward advice.

### F8 - warframe.market is treated as critical infrastructure (high)

*"wf market being a third party website is crazy... wf market has been down
for a day now and will possibly be for much longer due to maintenance. the
trade community is in shambles, trade chat is filled with people"* (991)
[1oyy74t](https://www.reddit.com/r/Warframe/comments/1oyy74t/wf_market_being_a_third_party_website_is_crazy/).
Top comments ask for an official site or an in-game auction house.

This settles the relationship question: warframe.market is upstream, not a
displacement target. It also makes a legible offline/degraded state a product
requirement rather than a nicety.

### F9 - The API terms bound what may be published (high, and load-bearing)

See constraints below. This was not previously recorded anywhere in the
repository, and it constrains every future tranche that touches market data.

## Constraints

Quoted from warframe.market's own localization strings
(`42bytes-team/wfm-localization/locales/en.json`), which is what the site
renders as its terms.

**K1 - API data and public applications.**
`app.tos.api.dataUsage`: *"API data, unless for personal use or educational
purposes, may only be used to provide additional features not available on the
Site. Site staff should be contacted prior to making applications using the API
public."*
Also `app.tos.api.block` (*"Applications using the API may be blocked at the
discretion of the Site developers"*), `app.tos.api.rateLimit` (*"Requests may
not be made at a rate faster than 3 per second for listings or 10 per minute
for contracts"*), and `app.tos.api.no_support`.

The "additional features not available on the Site" test is the same bar the
project already sets for itself: reading the player's inventory and deciding
what to sell is not something the Site does. The staff-contact requirement is
the one open question - see "Open items".

**K2 - Message generation.**
`app.tos.trading.copyPaste`: *"The Site provides a generated message
("copy/paste message") that should be copied into the game's chat as a whisper
message. This copy/paste message must be used as-is without modification at the
start of contact and signifies the beginning of protection of both parties
within these Terms."* It continues: *"If the provided copy/paste message cannot
be used for reasons such as the in-game paste function not working or being
unable to copy text on the platform used to play Warframe, a custom message may
be used in its place as long as the price and item are correct and the Site is
mentioned in either the contacting message or any subsequent message that is
within 2 minutes afterward and before the Lister responds. Interactions where
the Site is not mentioned at such a time are not covered by these Terms."*
`app.tos.api.copyPaste`: *"Applications that provide a copy/paste message must
provide the message in the same format as seen on the Site and include the
application name."*

So two things are true at once: an application may provide a message if it
matches the Site's format and names the application, **and** the Site's own
message is what carries the described protection when used unmodified at the
start of contact. Appending an application name to the Site's verbatim message
satisfies neither: it is a modification of the protected message and it is not
a purpose-built site-format message. It is also untestable in practice, because
reproducing the Site's generated string byte-for-byte from local data would
drift silently.

**K3 - Product policy (stricter than K2).** TennoWorth generates no contact
messages. The buyer comparison panel stays decision support, and contact is
handed off to warframe.market, where the Site's own copy button produces the
Site's own message.

**K4 - Existing request etiquette is unchanged.** Descriptive user agent, 3
requests per second, 10 per minute on auctions, and a 429/509 response pausing
the process (`docs/wfm-access.md`).

## Coverage today

Revision `03fe890`.

| Finding | Ships today | Gap |
|---|---|---|
| F1 riven pricing | DE weekly band, disposition, live comps with owner status, reroll cost, offer math | The comps sample is the 20 cheapest eligible auctions for a weapon; nothing describes how stale or thin that sample is |
| F2 announced change | Hold-or-sell advisor on prime calendars plus a year of medians | No announced-change input; the advisor produces no verdict for riven rows |
| F3 buyer friction | Buyer coverage panel, listing health, price watches, trade session | Contact is deliberately out of scope (K3) |
| F4 mispricing | Pricing from traded medians, volume and DE usage rather than trade chat | No beginner-facing "this number is far off" framing |
| F5 inventory question | Sell ranking, presets, sets, ducats, trade session | This is the product's existing promise, not new work |
| F6 wedge | Windows and Linux as equals, no Overwolf, MIT, read-only memory scan, no telemetry, `EE.log` auto-close | Undifferentiated on features against TennoScope, TennoHub and WFHub; the trust stance is underused in the product surface |
| F7 feature list | Relic overlay, WFM listing management, `EE.log` auto-close, inventory filters | Chat trade-request notification; mastery-aware reward advice |
| F8 WFM outage | Bundled snapshot, ETag refresh, staleness banner | No legible app-wide state for "warframe.market itself is unreachable" |
| F9 API terms | Described user agent, pacing, pending-plan recovery | Staff contact has no public record; `dataUsage` is now recorded here |

## What this did not establish

- **Real buyer behaviour.** No data on how often a contacted buyer responds or
  completes, and no evidence that contacting buy orders instead of posting asks
  changes outcomes.
- **Competitor adoption.** TennoScope, TennoHub, WFHub, VoidStonks and the rest
  are visible in conversations; nothing here measures whether anyone uses them.
- **The 2025 archive buckets**, which are internally inconsistent and were
  therefore excluded from trend claims.
- **Whether the complaints dominate.** A recurring complaint is not a majority
  preference. The archive counts cannot settle that, and no survey exists.
- **Anything about console or mobile players**, whose inventories this product
  cannot read at all - the sweep covered a PC-centric conversation.

## Open items

1. **K1 staff contact.** The terms ask for contact before an API application is
   public. The draft lives in a maintainer-only document, so nothing in the
   public tree evidences that it happened.
2. **Riven liquidity wording.** `created`/`updated` on an auction are not
   time-to-sale, and `updated` is not proof a seller is available. Any future
   readout must say which sample it describes and how old it is.
