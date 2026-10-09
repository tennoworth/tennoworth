# Changelog

Desktop releases. Versions are `desktop-v<version>` tags.

**Pre-1.0 the minor digit is spent sparingly, because 1.0 has to mean
something.** It is not a counter of how much work happened.

- **patch** - the default, and where nearly everything belongs: bug fixes,
  dependency bumps, internal work, and ordinary user-facing features and UI
  work. A new view or a reworked screen is a patch.
- **minor** - reserved for changes to *what the product is or what it is
  compatible with*: a distribution channel added or removed, a persisted
  database / inventory / export format change, an updater or package-identity
  change, a whole-product overhaul, or anything that breaks compatibility.
  If you cannot name the thing that changed shape, it is a patch.
- **major** - 1.0 only, and only when the compatibility contract below is one
  the maintainer is willing to stand behind: database migrations, export
  formats, updater continuity, package identity, supported operating systems.

**Not every change needs a release at all.** The web app is continuously
deployed from `main` and identifies itself by build commit, not by this
version - a change confined to `frontend/` reaches tennoworth.app on
promotion and needs no desktop release. Cut one when desktop users have a
reason to update.

`bun scripts/release.ts prepare <bump>` opens a deliberately incomplete,
structured section here. Before merging the bump, replace every placeholder:
the release body starts with exactly one contextual emoji in
`# <emoji> TennoWorth Desktop X.Y.Z`, follows with an announcement and a
separate plain-language summary, groups top-level bullets below plain-text
category headings under `## Changelog (N)`, and ends with `## Updating`.
`N` must equal the number of top-level bullets. No other emoji belongs in the
entry. Use `🐧` for Linux/Wayland, `🪟` for Windows, `🔒` for security, `🎨`
for appearance, or another single symbol that honestly describes the release.

`bun scripts/release.ts check` enforces this contract as soon as the desktop
version moves ahead of the latest published tag. The release workflow repeats
the same validation through `bun scripts/release.ts notes --release` before
building either platform. Historical entries are intentionally not backfilled.

Releases up to and including 0.3.8 predate this file, and their notes live on
the GitHub releases themselves.

App summaries are generated from annotated bullets in this file. For releases
newer than 0.7.1, include at least one plain-English bullet in this form:
`- **What the person can do** A short explanation. <!-- app-note {"id":"stable-change-id","kind":"improved"} -->`
Kinds are `improved`, `fixed`, and `action`; optional `platforms` restricts a note
to `windows` or `linux`. Stable IDs deduplicate repeated notes. Optional
`supersedes` explicitly replaces earlier non-action IDs; action notices cannot
be hidden this way. Keep titles under 100 and explanations under 600 characters.
The annotation is omitted from published release bodies.

After editing, run `bun scripts/release.ts app-notes`. Release checks and frontend
builds reject a stale bundle. Historical releases through 0.7.1 keep their original
format; the app explicitly identifies incomplete coverage for older upgrades.

## 0.8.101 - 2026-10-09

# 🟢 TennoWorth Desktop 0.8.101

TennoWorth Desktop 0.8.101 is ready.

Buyers on warframe.market look for sellers who are online or in game, so your status decides whether your listings are seen. You can now set it from TennoWorth, keep it for a while, or let the app follow the game: online in game while Warframe runs, and invisible when it closes. Updates can also be installed straight from Settings.

## Changelog (5)

### Trading

- **Set your warframe.market status from the app** Choose Online, Online in game or Invisible from the status strip, Settings or the tray, and keep it while TennoWorth runs or for 30 minutes to 4 hours. <!-- app-note {"id":"wfm-status-set","kind":"improved"} -->
- **Let your status follow the game** Off by default. When enabled, you show as online in game while Warframe runs and switch to Invisible (or Online, if you prefer) when it closes. A status changed on the website takes precedence until your next game session. <!-- app-note {"id":"wfm-status-follow-game","kind":"improved"} -->
- **Go invisible when you quit** A status the app is keeping up is set to Invisible when you quit TennoWorth or sign out, so you are not left showing online. <!-- app-note {"id":"wfm-status-invisible-on-quit","kind":"improved"} -->

### This app

- **Install updates from Settings** After Check for updates finds a new version, the same button installs it and then offers Restart now. The banner and Settings share one install, so it cannot run twice. <!-- app-note {"id":"settings-install-update","kind":"improved"} -->
- **Find the fields that start a task** The market lookup and the Sell name filter carry a quiet light sweep, and pressing / to jump to the filter briefly highlights it. Reduced-motion settings turn the motion off. <!-- app-note {"id":"field-shimmer","kind":"improved"} -->

## Updating

TennoWorth checks for updates automatically at launch and every 30 minutes while it is open. Downloads for Windows and Linux are available in the assets below.

## 0.8.100 - 2026-10-09

# 🤝 TennoWorth Desktop 0.8.100

TennoWorth Desktop 0.8.100 lets you help show what items really sell for, and reads Warframe's new riven system properly.

You can now choose to share the prices of your platinum trades, so TennoWorth can publish what items actually sell for rather than what people ask. It is off unless you turn it on, sends no names or inventory, and can be undone. The Rivens view understands trait locking and splicing from Update 44, prices comparable rivens by their real buyouts, and shows DE's popularity data for what it is. You can also pick the colour of the app's icon.

## Changelog (11)

### Price sharing

- **Choose to share what your trades sold for** Off unless you enable it in Settings or from a one-time invitation. Only the item, quantity, price and day of platinum trades are sent, never who you traded with, your inventory or your accounts, and an item's price is published once five installs report it in a week. <!-- app-note {"id":"price-sharing-opt-in","kind":"improved"} -->
- **Delete what you shared** Delete recent reports in Settings removes this install's reports from this week and last week; older weeks are already merged into totals that link to no install. <!-- app-note {"id":"price-sharing-erase","kind":"improved"} -->

### Rivens

- **See which spliced traits a riven can make** Splice options follow DE's recipe table, and spliced traits are marked on your rivens and in listings. <!-- app-note {"id":"riven-splice-options","kind":"improved"} -->
- **Comparable rivens use real buyout prices** Auctions with only a starting bid no longer show as 1p asks, and comparisons open on rolls that share your riven's positive stats. <!-- app-note {"id":"riven-comps-buyout","kind":"fixed"} -->
- **Account for a locked trait** The offer check has a trait is locked switch that doubles the reroll cost, as cycling with a lock does since Update 44. <!-- app-note {"id":"riven-trait-lock","kind":"improved"} -->
- **Riven popularity reads correctly** DE's weekly figure is a 0-100 popularity score, not a sale count, and the offer check no longer hides most of it. <!-- app-note {"id":"riven-popularity-score","kind":"fixed"} -->
- **Report rivens the app cannot fully read yet** Each riven has Copy data, and a riven with data TennoWorth does not understand yet says so and asks for a report. <!-- app-note {"id":"riven-copy-data","kind":"improved"} -->

### Selling

- **List on WFM says what it does** The button no longer shows a batch count of 50 that was only ever the table's cap. <!-- app-note {"id":"list-on-wfm-label","kind":"fixed"} -->

### This app

- **Pick the app icon's colour** Classic blue, ink, rag, or match your colour mode, for the window, taskbar and tray. Your desktop shortcut and the installer keep the classic icon. <!-- app-note {"id":"app-icon-colour","kind":"improved"} -->
- **An occasional note about supporting TennoWorth** After an update, and then at most once a year, a note you can dismiss links to Ko-fi. Every feature stays free. <!-- app-note {"id":"support-note","kind":"improved"} -->

### Linux

- **The Column guide shows its entries again** In the Linux app the guide opened as a bare title bar; its explanations now appear. <!-- app-note {"id":"column-guide-webkitgtk","kind":"fixed","platforms":["linux"]} -->

## Updating

TennoWorth checks for updates automatically at launch and every 30 minutes while it is open. Downloads for Windows and Linux are available in the assets below.

## 0.8.7 - 2026-10-05

# 🔍 TennoWorth Desktop 0.8.7

TennoWorth Desktop 0.8.7 makes the numbers you sell by easier to read and harder to misread.

Baro's arrival notice now tells you what is worth buying and how many ducats your items could earn, and notification history is organized by day. The Sell table explains every column in one guide, its columns say what they measure, and the prices and ages behind repricing advice are stated honestly. Small windows reach the first decision sooner, and more controls work with a keyboard and a screen reader.

## Changelog (11)

### Baro and notifications

- **Baro's arrival notice shows buying value and ducat yield** See worthwhile picks at snapshot prices and how many ducats your held items could yield. <!-- app-note {"id":"baro-arrival-value","kind":"improved"} -->
- **Notification history is easier to scan** Separate timestamps, day groups and organized Baro and daily digest details make the next action easier to find. <!-- app-note {"id":"notification-history-layout","kind":"improved"} -->

### Selling

- **One guide explains every Sell column** The small "?" beside each column header is replaced by a Column guide next to Columns, and every header can now be sorted from the keyboard. <!-- app-note {"id":"sell-column-guide","kind":"improved"} -->
- **Sell columns say what they measure** Score is now Priority, Potential is now Stack value, and Low sell is now Low ask. Stack value counts only the copies you can sell. <!-- app-note {"id":"sell-column-names","kind":"improved"} -->
- **Top picks keep List within reach** List and hide sit beside each pick, so a narrow window no longer has to scroll sideways to act on one. <!-- app-note {"id":"top-picks-actions-in-reach","kind":"improved"} -->
- **Repricing advice states its real basis** My orders shows how old the market snapshot actually is, and a reprice reads as the cut or raise to your ask. Stale-price warnings read correctly. <!-- app-note {"id":"honest-market-age","kind":"fixed"} -->

### Planning

- **Trade Session and relic numbers say what they count** Trade Session shows the trades you have left today, and a relic shows the expected value of cracking every copy you own. <!-- app-note {"id":"trade-relic-copy","kind":"improved"} -->

### Price watches

- **Watch reconnects use less data** Reconnecting the live price watch reuses the item list it already has instead of downloading the whole catalogue again. <!-- app-note {"id":"watch-reconnect-catalogue","kind":"improved"} -->

### Accessibility and layout

- **Unavailable buttons look unavailable** Buttons such as Add watch and Scan game no longer look clickable while they are disabled. <!-- app-note {"id":"disabled-primary-buttons","kind":"fixed"} -->
- **Fields and order actions are named for screen readers** Fields announce their visible labels, and order buttons name the action and the item. <!-- app-note {"id":"accessible-field-names","kind":"fixed"} -->
- **Small windows reach the first decision sooner** In narrow windows, project links and feedback move to the bottom of the page. <!-- app-note {"id":"compact-narrow-shell","kind":"improved"} -->

## Updating

TennoWorth checks for updates automatically at launch and every 30 minutes while it is open. Downloads for Windows and Linux are available in the assets below.

## 0.8.6 - 2026-10-02

# 🧩 TennoWorth Desktop 0.8.6

TennoWorth Desktop 0.8.6 makes the relic overlay read the right thing and keeps your listings and scans safer.

The overlay now reads the game's own window even with warframe.market open in a browser, matches reward names by whole words, and looks the same on Wayland as everywhere else. Automatic listing updates after a trade never guess at a quantity, and "remember on this device" now holds even when a sign-in and an unlock overlap.

## Changelog (10)

### Relic overlay

- **The overlay reads the game, not your browser** The overlay used to capture the first window with "warframe" in its title, which could be warframe.market in a browser or a Discord channel. It now picks the game's own window. <!-- app-note {"id":"overlay-captures-game-window","kind":"fixed"} -->
- **Reward names match by whole words** A reward was sometimes matched to a shorter item name hidden inside it, so Limbo could be read as Bo. Names now match by whole words. <!-- app-note {"id":"overlay-whole-word-match","kind":"fixed"} -->
- **The Wayland overlay matches the app's colours** On Linux Wayland, the reward cards used an old colour scheme. They now use the same colours as the overlay everywhere else. <!-- app-note {"id":"wayland-overlay-palette","kind":"fixed","platforms":["linux"]} -->
- **Preview overlay works on Wayland** The Preview overlay button in Settings failed on Wayland with "no primary monitor is available". It now shows the preview on the monitor TennoWorth is on. <!-- app-note {"id":"wayland-overlay-preview","kind":"fixed","platforms":["linux"]} -->

### Selling

- **After a trade, listings are only changed when TennoWorth is sure** When a trade completes, TennoWorth lowers or removes the matching listing. It now skips any listing it cannot fully read, logs why, and never assumes a quantity of one. Listing protection refuses to go ahead when your current orders cannot be read in full. <!-- app-note {"id":"trade-close-never-guesses","kind":"fixed"} -->

### Sign-in and backups

- **"Remember on this device" sticks** Unlocking while a sign-in finished elsewhere in the app could store an out-of-date key or delete the new one, so the next launch asked for your passphrase again. The newest choice now always wins. <!-- app-note {"id":"remember-newest-wins","kind":"fixed"} -->
- **TennoWorth says when it cannot remember you** If your system has no keyring available, for example with KDE Wallet switched off, the sign-in and unlock dialogs now explain that you will be asked for your passphrase each launch and how to turn the keyring on. <!-- app-note {"id":"keyring-unavailable-note","kind":"improved"} -->
- **Backup passphrases count every character once** The 12-character minimum for export passphrases counted some symbols, such as pictographs, as two characters. It now counts characters the same way sign-in does. <!-- app-note {"id":"passphrase-counts-characters","kind":"fixed"} -->

### Reliability

- **A damaged saved inventory no longer blocks scanning** If the saved inventory on disk was corrupt, every later scan failed and the bad copy was never replaced. TennoWorth now treats it as no saved inventory, so the next scan replaces it. <!-- app-note {"id":"corrupt-inventory-survives","kind":"fixed"} -->
- **Ledger and price watches show the latest data** A slow reload could replace newer results in the Ledger and Price watches; it no longer does. <!-- app-note {"id":"stale-panel-results","kind":"fixed"} -->

## Updating

TennoWorth checks for updates automatically at launch and every 30 minutes while it is open. Downloads for Windows and Linux are available in the assets below.

## 0.8.5 - 2026-10-01

# 🔧 TennoWorth Desktop 0.8.5

TennoWorth Desktop 0.8.5 makes two things say what is actually true.

If you share the daily usage count, the terms in Settings now describe exactly what the app sends and how often it tries. The trader calendar also flags old event data on the same day the rest of TennoWorth does, instead of a day later.

## Changelog (2)

### Settings

- **Usage sharing terms match what the app does** "What is sent and what is not" said the app makes one check-in attempt a day with no retries. If a check-in does not get through, the app offers the same daily token again once a minute, at most 10 times that day, and never for a past day. The terms now say so. Nothing about what is sent has changed. <!-- app-note {"id":"usage-terms-retries","kind":"fixed"} -->

### Calendar

- **Old event data is flagged on time** The trader calendar now adds "reward data 7d old" to an event as soon as its reward data reaches seven days, the same age at which TennoWorth's market data warns. It used to stay silent until the eighth day. <!-- app-note {"id":"calendar-stale-seven-days","kind":"fixed"} -->

## Updating

TennoWorth checks for updates automatically at launch and every 30 minutes while it is open. Downloads for Windows and Linux are available in the assets below.

## 0.8.4 - 2026-09-29

# 🎨 TennoWorth Desktop 0.8.4

TennoWorth Desktop 0.8.4 reorganises Settings around what you actually come back to.

Automatic scanning, the relic overlay and notifications now lead the page, under a status strip that shows at a glance whether scanning and the overlay are running and whether you are signed in. Settings you change once, like colour mode, updates and usage sharing, sit together at the bottom.

## Changelog (3)

### Settings

- **See what is running at a glance** A status strip at the top of Settings shows automatic scan, relic overlay and warframe.market sign-in state, and each part links to its section. <!-- app-note {"id":"settings-status-strip","kind":"improved"} -->
- **Everyday settings come first** Automatic scan, the relic overlay and notifications lead the page. Colour mode, updates and usage sharing are grouped in one "This app" panel at the bottom. The overlay's Preview and Scan buttons sit in its title bar, and diagnostics are under Troubleshooting. <!-- app-note {"id":"settings-reordered","kind":"improved"} -->
- **Notification settings opens where you need it** The Notification settings button in the inbox now opens Settings at Notifications, where each alert type is one row with Inbox and Popup switches and its timing underneath. <!-- app-note {"id":"notification-settings-link","kind":"improved"} -->

## Updating

TennoWorth checks for updates automatically at launch and every 30 minutes while it is open. Downloads for Windows and Linux are available in the assets below.

## 0.8.3 - 2026-09-28

# 🔧 TennoWorth Desktop 0.8.3

TennoWorth Desktop 0.8.3 fixes a batch of small but misleading things.

The Baro countdown now keeps counting while the app stays open, the vault advice and the "vaulting soon" tag finally agree, and automatic scans that find nothing new no longer pile up copies of your inventory or throw away a batch you are reviewing. Signing out now always sticks.

## Changelog (6)

### Baro and vaulting

- **The Baro countdown keeps counting** The arrival and departure countdowns now update every minute and flip to "here" on time, instead of freezing until the next market refresh. <!-- app-note {"id":"baro-countdown-ticks","kind":"fixed"} -->
- **Vault advice matches the "vaulting soon" tag** Hold advice before a vault now uses the same 60-day window as the "vaulting soon" tag and the Vaulted filter, so every held item shows up there. Items further out are no longer marked hold. <!-- app-note {"id":"vault-soon-60-days","kind":"improved"} -->

### Inventory and scans

- **Repeated scans no longer pile up** A scan that finds exactly the same inventory as the last one updates its time instead of saving another full copy, so a long session with automatic scans no longer grows the local database. <!-- app-note {"id":"identical-scans-reused","kind":"improved"} -->
- **A quiet scan leaves your review alone** An automatic scan that finds nothing new no longer forces you to prepare a listing batch again. <!-- app-note {"id":"identical-scan-keeps-review","kind":"fixed"} -->
- **Imports no longer show old Rivens** Importing an inventory file clears the Rivens from your previous scan instead of listing them beside the imported items. <!-- app-note {"id":"import-clears-rivens","kind":"fixed"} -->

### Sign-in

- **Signing out always sticks** Signing out while a sign-in was still finishing could bring the saved login back on the next launch. It now stays signed out, and a warframe.market outage during sign-in reports the outage rather than a confusing parse error. <!-- app-note {"id":"sign-out-sticks","kind":"fixed"} -->

## Updating

TennoWorth checks for updates automatically at launch and every 30 minutes while it is open. Downloads for Windows and Linux are available in the assets below.

## 0.8.2 - 2026-09-28

# 🐧 TennoWorth Desktop 0.8.2

TennoWorth Desktop 0.8.2 makes automatic scanning work on Linux.

Automatic scanning, added in 0.7.107, never started while Warframe ran under Proton. Settings kept promising a scan a minute away, and the inventory stayed on your last manual scan. The app now finds the game reliably, so scans run on the cadence you chose and the inventory updates on its own.

## Changelog (2)

### Automatic scan

- **Automatic scans run on Linux** With the game running under Proton, the app now recognises it as the same game from one check to the next, so the scheduled scan actually happens and the inventory header shows the new scan time. <!-- app-note {"id":"auto-scan-linux-detection","kind":"fixed","platforms":["linux"]} -->
- **Settings shows the next scan too** After an automatic scan, the status line shows when the last scan ran and when the next one is due. <!-- app-note {"id":"auto-scan-status-next","kind":"improved"} -->

## Updating

TennoWorth checks for updates automatically at launch and every 30 minutes while it is open. Downloads for Windows and Linux are available in the assets below.

## 0.8.1 - 2026-09-27

# 🐞 TennoWorth Desktop 0.8.1

TennoWorth Desktop 0.8.1 makes bug reports say what actually went wrong.

A report sent from the app now describes a failed scan accurately. The most common scan failure, a game session the app cannot find in memory, used to be reported as a connection problem. You can report a failed warframe.market sign-in straight from the sign-in window, and before a report opens on GitHub, the app shows you which problems it includes. Reports still carry only fixed categories, never your error text, account details or inventory.

## Changelog (4)

### Bug reports

- **Scan problems are reported accurately** A report now names the real cause of a failed scan, such as Warframe not running, the game session not found in memory, or the inventory request being refused along with its HTTP status. It no longer reports a connection problem for all of them. <!-- app-note {"id":"scan-report-categories","kind":"fixed"} -->
- **Report a sign-in problem from the sign-in window** When signing in to warframe.market or unlocking your login fails, the window offers Report a bug. The report records the kind of failure, not its message. A wrong passphrase is not treated as a bug. <!-- app-note {"id":"signin-bug-report","kind":"improved"} -->
- **See what a report includes before sending it** The feedback window lists the problems a bug report will carry, such as a failed scan, automatic scan or update, in plain words. <!-- app-note {"id":"feedback-included-problems","kind":"improved"} -->
- **The app version is easy to find** The bottom of the sidebar now shows the version next to the build, and bug reports fill it in even when the update check has not answered. <!-- app-note {"id":"sidebar-version","kind":"improved"} -->

## Updating

TennoWorth checks for updates automatically at launch and every 30 minutes while it is open. Downloads for Windows and Linux are available in the assets below.

## 0.8.0 - 2026-09-27

# 🔑 TennoWorth Desktop 0.8.0

TennoWorth Desktop 0.8.0 signs you in to warframe.market again.

warframe.market now puts a browser check in front of its sign-in page, which stopped the app's login from working. The app now opens warframe.market's own sign-in page instead, so your password goes only to warframe.market. This release is also signed with a new update key, so existing installs cannot update to it automatically: download it once below and install it over your current version. Your data and settings are kept, and later releases update automatically again.

## Changelog (3)

### Account

- **Sign in to warframe.market again** Signing in opens warframe.market's own page in a private window, and the app never sees your password. On Linux systems where that window cannot display the page, the app asks you to sign in with your browser and paste the session token instead. Saved logins keep working. <!-- app-note {"id":"wfm-page-signin","kind":"fixed"} -->

### Updates

- **A rejected update says what to do** If an update ever fails its signature check, the app now tells you to download the latest release once, instead of showing the updater's raw error. <!-- app-note {"id":"update-signature-guidance","kind":"improved"} -->

### Selling

- **A listed set is no longer called unowned while prices load** Before the market data finished loading, a listed prime set could briefly read as not in your inventory and be offered for deletion. The app now waits for the market data before judging it. <!-- app-note {"id":"set-ownership-while-loading","kind":"fixed"} -->

## Updating

Versions before 0.8.0 cannot update to this release automatically. Download it for Windows or Linux from the assets below and install it over your current version; your data and settings are kept. From 0.8.0 on, TennoWorth checks for updates automatically again.

## 0.7.107 - 2026-09-25

# 🔄 TennoWorth Desktop 0.7.107

TennoWorth Desktop 0.7.107 can keep your inventory current on its own.

Turn on automatic scanning and the app scans every 15, 30 or 60 minutes while Warframe is running, and never while it is closed. The app also has a quieter, more legible look, a Sell table that starts with the columns that decide a sale, and items from the latest Prime Access that now resolve on day one. Order changes are only reported once warframe.market confirms them, and a listed prime set is no longer offered for one-click deletion as "not owned".

## Changelog (9)

### Scanning

- **Scan automatically while the game is running** An opt-in setting scans every 15, 30 or 60 minutes, only while Warframe is running. A freshly launched game is given time to finish logging in first, failures are shown in Settings instead of as notifications, and nothing changes for anyone who leaves it off. <!-- app-note {"id":"auto-scan","kind":"improved"} -->
- **New Prime Access items resolve on day one** Items warframe.market lists before the community item data catches up - such as Citrine, Steflos and Corufell Prime - now resolve in inventory, with their set breakdowns and relic rewards. <!-- app-note {"id":"day-one-items","kind":"fixed"} -->

### Selling

- **Sell starts with the columns that decide a sale** The default preset shows ownership, score, price, trend, volume, advice and potential without scrolling sideways, and a Columns menu adds or removes any column per preset. The first run leads with the scan button. <!-- app-note {"id":"sell-columns","kind":"improved"} -->
- **Order changes are reported only when confirmed** A repricing, quantity change or visibility change that warframe.market did not confirm is now reported as unconfirmed instead of as done, and an interrupted batch whose outcome is unknown can always be reviewed or discarded. <!-- app-note {"id":"confirmed-order-changes","kind":"fixed"} -->
- **A listed prime set is no longer called unowned** A scan never reports a set itself, so the orders panel read a listed set as not owned and offered to delete it. It no longer does, and deleting a listing from the health fixes now asks for a second click. <!-- app-note {"id":"listed-set-ownership","kind":"fixed"} -->

### Trades

- **A trade the ledger could not save is retried** A confirmed trade that failed to record was lost for good. The log reader now holds its place until the trade is saved and offers it again, without recording it twice. <!-- app-note {"id":"trade-ledger-retry","kind":"fixed"} -->

### Account

- **Logging out always wins** A logout that landed while the app was still unlocking or signing in could be undone, leaving the app signed in. Logout now takes precedence, and a new sign-in is no longer lost when a silent unlock fails. <!-- app-note {"id":"logout-precedence","kind":"fixed"} -->

### Appearance

- **A quieter, more readable interface** Each view leads with its title, prices and trend lines are drawn to be read first, status tags share one style, light-mode text is darker, and stale market data is flagged as a caution rather than an error. <!-- app-note {"id":"interface-refresh","kind":"improved"} -->

### Market data

- **Prices never step backwards to an older snapshot** The app refuses a downloaded market snapshot older than the one it already holds, so the tray and the dashboard stay on the same, newest prices. <!-- app-note {"id":"no-older-snapshot","kind":"fixed"} -->

## Updating

TennoWorth checks for updates automatically at launch and every 30 minutes while it is open. Downloads for Windows and Linux are available in the assets below.

## 0.7.106 - 2026-09-20

# 🧩 TennoWorth Desktop 0.7.106

TennoWorth Desktop 0.7.106 counts the prime parts your inventory always had.

The game files an unbuilt prime part under a Blueprint path that TennoWorth did not look up, so those parts were missing from the sell view, from set planning and from the Baro ducat total - one real inventory was short nine parts and 590 ducats. They resolve now. The riven comparables drawer also explains the sample it is showing and how old each listing is, a re-checked quote stops reporting itself as fresher than it is, and the inventory-ready popup that fired after every scan is gone.

## Changelog (4)

### Inventory

- **Prime parts are no longer missing from what you own** Unbuilt prime parts are filed under a Blueprint path that TennoWorth did not recognise, so they were dropped from the owned inventory - one real inventory was short nine parts and 590 ducats. They now resolve, and they count again in the sell view, in set planning and in the Baro ducat total. <!-- app-note {"id":"blueprint-prime-parts","kind":"fixed"} -->

### Rivens

- **The comparables drawer accounts for its sample** It reports how many asks are in the sample, how many carry a listing time, the newest-to-oldest span, and the seller-status counts; each auction says how long ago it was listed; and the DE band note says plainly that its number is a sold count. A single Refresh replaces keeping the first fetch for the life of the view. <!-- app-note {"id":"riven-comps-sample","kind":"improved"} -->

### Selling

- **A re-checked quote stops looking fresher than it is** A cached buyer read was stamped with the clock at the moment it was served instead of when the payload was decoded, so re-checking inside the cache window moved the check time forward while the data stayed the same. The displayed check time now reflects when the quote was actually observed. <!-- app-note {"id":"live-quote-observed-at","kind":"fixed"} -->

### Scanning

- **No more inventory-ready popup after every scan** The post-scan pass still rebuilds the tray and records its summary, but it no longer writes an inbox entry or fires a desktop notice that duplicated the daily sell digest. The retired notification category is gone from settings, and stored preferences and inbox rows are cleaned up when they are read. <!-- app-note {"id":"drop-scan-notification","kind":"improved"} -->

## Updating

TennoWorth checks for updates automatically at launch and every 30 minutes while it is open. Downloads for Windows and Linux are available in the assets below.

## 0.7.105 - 2026-09-15

# 📋 TennoWorth Desktop 0.7.105

TennoWorth Desktop 0.7.105 turns Routines into a checklist you can actually keep.

Track the daily and weekly habits that compound, and add the monthly goals you are working toward - each with its own tick. Put them in the order you want with the buttons or by dragging, and the list remembers where you left it. This release also updates the TLS library the app uses for market connections.

## Changelog (5)

### Routines

- **Tick off what you have already done** Routines is a real checklist now: mark a daily or weekly task as you finish it, and the tick stays until that period resets. <!-- app-note {"id":"routine-checklist","kind":"improved"} -->
- **Keep a list of monthly goals** Add as many goals as you are genuinely working on, rename one in place, or remove it. Each goal carries its own tick, and the ticks reset with the month while the goals stay. <!-- app-note {"id":"monthly-goals","kind":"improved"} -->
- **Put the list in the order you want** Move a goal with the Up and Down buttons, or drag it by the handle to any position. The order is kept between sessions. <!-- app-note {"id":"goal-reordering","kind":"improved"} -->

### Scanning

- **Steadier inventory scanning** Capture rejects values that do not match the shape the game produces, keeps session details out of error messages, and bounds its own resource use. <!-- app-note {"id":"capture-hardening","kind":"fixed"} -->

### Security

- **Updated the library that secures market connections** TennoWorth now uses a rustls release that rejects TLS 1.3 handshake messages sent at the wrong encryption level. <!-- app-note {"id":"rustls-2026-0285","kind":"fixed"} -->

## Updating

TennoWorth checks for updates automatically at launch and every 30 minutes while it is open. Downloads for Windows and Linux are available in the assets below.

## 0.7.104 - 2026-09-11

# 🧭 TennoWorth Desktop 0.7.104

TennoWorth Desktop 0.7.104 makes inventory advice useful sooner and safer to act on.

See protected opportunities before connecting WFM, recover without losing sight of your last good scan, and review exactly what changed after an update. Daily installation counting remains off unless you choose to enable it.

## Changelog (5)

### Selling

- **Get protected estimates before connecting WFM** Scan or restore inventory to see estimated selling and ducat opportunities that honor your keep rules. The new What I’m keeping summary makes quantities and item rules easier to review. <!-- app-note {"id":"protected-estimates","kind":"improved"} -->
- **Post only from the inventory you reviewed** Listing changes and recovered batches now require the matching current game scan. If your inventory changed, TennoWorth keeps your edits and tells you whether to scan or recheck quantities. <!-- app-note {"id":"listing-scan-identity","kind":"fixed"} -->

### Recovery

- **Keep your last good inventory after a scan problem** Failed scans and scans with no tradeable items leave the last successful inventory visible with its real age. Retry, update, Settings, and reporting actions remain available. <!-- app-note {"id":"inventory-recovery","kind":"fixed"} -->

### Privacy

- **Choose whether to share a daily installation count** Sharing is off by default. If enabled in Settings, TennoWorth sends a rotating daily token without account, inventory, hardware, version, or activity details. <!-- app-note {"id":"daily-installation-count","kind":"improved"} -->

### Updates

- **Read what changed after an update** TennoWorth brings together the bundled notes for every release you skipped and lets you reopen them from Settings → Updates → What’s new. <!-- app-note {"id":"installed-update-notes","kind":"improved"} -->

## Updating

TennoWorth checks for updates automatically at launch and every 30 minutes while it is open. Downloads for Windows and Linux are available in the assets below.

## 0.7.103 - 2026-09-08

# 🔧 TennoWorth Desktop 0.7.103

TennoWorth Desktop 0.7.103 fixes inventory scanning and update downloads.

Scan your inventory without game metadata blocking the result, and update directly from older supported versions without a cached download disappearing after another release.

## Changelog (2)

### Inventory

- **More reliable inventory scans** Fixed a scan error caused by game data that was not needed to read your inventory. <!-- app-note {"id":"scan-metadata","kind":"fixed"} -->

### Updates

- **Updates work when you have missed a release** Older update links keep working after a newer version comes out. <!-- app-note {"id":"update-downloads","kind":"fixed"} -->

## Updating

TennoWorth checks for updates automatically. Downloads are available below.

## 0.7.102 - 2026-09-08

# 💱 TennoWorth Desktop 0.7.102

TennoWorth Desktop 0.7.102 is ready.

Plan sales around what you want to keep, compare complete sets with individual parts, and see how much of your stack visible buyers can cover.

## Changelog (4)

### Selling

- **Choose what you want to keep** Set aside copies or save the parts for a set you are building. Your selling and ducat suggestions take those choices into account. <!-- app-note {"id":"keep-plan","kind":"improved"} -->
- **Plan sales for complete sets** Add complete sets to Trade Session without counting their parts again in another sale. <!-- app-note {"id":"complete-sets","kind":"improved"} -->
- **Find buyers for larger stacks** See how much of your stack the listed buyers want, and compare their offers with your asking price. <!-- app-note {"id":"buyer-stacks","kind":"improved"} -->

### Reliability

- **Your latest inventory stays in view** An older calculation finishing late will no longer replace your newer scan, import, or plan. <!-- app-note {"id":"newest-inventory","kind":"fixed"} -->

## Updating

TennoWorth checks for updates automatically. Downloads are available below.

## 0.7.101 - 2026-09-08

# 🐧 TennoWorth Desktop 0.7.101

TennoWorth Desktop 0.7.101 is a small Linux hotfix.

Feedback forms and other external links now open your browser from the AppImage on systems affected by bundled library conflicts.

## Changelog (1)

### Linux

- **Links open your browser again** Report a bug, suggest an improvement, or open an item on warframe.market from the Linux AppImage. <!-- app-note {"id":"appimage-links","kind":"fixed","platforms":["linux"]} -->

## Updating

TennoWorth checks for updates automatically at launch and every 30 minutes while it is open. Downloads for Windows and Linux are available in the assets below.

## 0.7.1 - 2026-09-08

# 🎨 TennoWorth Desktop 0.7.1

TennoWorth Desktop 0.7.1 is ready.

This update makes Settings and the trading dashboard easier to scan, with consistent spacing, clearer labels, and more room for item names and actions. It also fixes the enlarged, blurred background pattern seen in WebKit.

## Changelog (6)

### Interface

- Align Settings and notification preferences in one column, with separate detection, reward-card, and diagnostics groups.
- Use consistent page headings, section spacing, and labelled controls across the trading and management views.
- Label Top picks facts and give item names, riven comparisons, and table actions enough room to remain readable.
- Separate watch creation from monitoring and ledger totals from listing automation and trade history.

### Fixes

- Keep the background grid and diagonal hatching sharp and evenly spaced on WebKit surfaces.

### Market data

- Refresh the bundled market snapshot and item catalog used when a live refresh is unavailable.

## Updating

TennoWorth checks for updates automatically. Downloads are available below.

## 0.7.0 - 2026-09-08

# 🎯 TennoWorth Desktop 0.7.0

TennoWorth Desktop 0.7.0 is ready.

Plan your next trading session, keep track of completed sales, and choose which market opportunities deserve an alert. This release brings Trade Session and a persistent notification inbox to Windows and Linux, alongside clearer layouts and safer listing review.

## Changelog (10)

### Trading

- Plan a Trade Session with Fast Cash, Plat per Trade, Clear Inventory, or Max Value, using a trade budget and optional platinum target.
- Review existing and proposed listings together, preserve edits during refresh, and recheck quantities, allowance, and changed orders before submission.
- Compare bulk orders using per-item prices and submit the reviewed lot total, with consistent pricing across live quotes and streamed price alerts.
- Protect global and per-item reserves in session planning and sell digests, and require a fresh scan before recommending items given away since the last scan.

### Notifications

- Keep completed trades, price-watch matches, Baro reminders, relevant calendar events, and daily sell opportunities in a persistent inbox.
- Select which notification categories are enabled and which may show desktop popups; preferences survive restarts.
- See listing follow-up when a completed sale needs attention, with replay protection that prevents duplicate trade handling.

### Desktop

- Use clearer light and dark layouts with responsive tables, readable long names, and review controls that remain reachable in narrow or short windows.
- Open bug reports and improvement suggestions from the app, and retain partially recognized relic rewards in their correct positions.

### Windows

- Run release and snapshot verification from Windows checkout paths, including paths containing spaces and URL-sensitive characters.

## Updating

TennoWorth checks for updates automatically. Use the in-app updater or download the Windows installer or Linux AppImage below.

Finish or discard pending Trade Session listing batches before downgrading; older versions do not understand their explicit lot sizes.

## 0.6.6 - 2026-09-03

- Relic reward recognition on Linux now follows the reward row's
  screen-height-scaled geometry and uses OCR-friendly grayscale crops, fixing
  reward screens that opened correctly but returned no recognized choices.
- Recognition now combines stable partial results across capture retries, so
  one temporarily unreadable reward slot no longer discards the other choices.

## 0.6.5 - 2026-08-30

- On Wayland, relic reward results now use a native layer-shell overlay that
  stays above borderless-fullscreen Warframe without taking focus or
  intercepting clicks. The existing desktop window remains available as a
  compatibility fallback.
- Overlay placement now maps XWayland capture geometry onto Wayland outputs,
  including mixed-DPI and fractional-scale layouts. Settings reports capture
  and presentation backends separately for clearer diagnostics.

## 0.6.4 - 2026-08-29

- TennoWorth now uses a cleaner transparent application icon with corrected
  honeycomb geometry.

## 0.6.3 - 2026-08-28

**Desktop recovery and account controls are more dependable.**

- Trade detection no longer guesses which warframe.market listing to adjust
  when EE.log omits a mod rank or item subtype, or when multiple listings share
  an item slug. Unambiguous untiered sales continue to reduce or close their
  matching listing automatically.
- Desktop event subscriptions now detach when panels, dialogs, update banners,
  or the relic overlay unmount, preventing duplicate notifications and progress
  updates after repeatedly opening those surfaces.
- The desktop market snapshot now refreshes after network reconnection and
  every 30 minutes, allowing offline launches to recover without an app restart.
- Settings now shows the warframe.market session state and provides a confirmed
  logout that removes saved credentials and interrupted listing batches from
  the device.

**Updates and inventory restores fail more safely.**

- Manual update checks now explain when a Linux install cannot self-update and
  needs the AppImage, instead of incorrectly reporting that install as current.
- Restoring an encrypted inventory backup now uses an in-app review step and
  cannot decrypt or replace a non-empty inventory before explicit confirmation.

**Small interface fixes round out the release.**

- Relic-overlay document styles are now isolated from the hosted site, restoring
  normal page scrolling while preserving the transparent overlay surface.
- GitHub and Ko-fi links are now available from the hosted status bar and the
  desktop sidebar.

## 0.6.2 - 2026-08-28

- Settings now includes a manual **Check for updates** action on Windows and
  Linux AppImage builds. TennoWorth also checks the signed updater manifest
  every 30 minutes while running; downloads and installation still require
  explicit confirmation.

## 0.6.1 - 2026-08-28

**Windows is now one installer, and the release carries one checksum file.**

- **The `.msi` is retired.** Every release shipped two Windows installers that
  did the same job; the NSIS `.exe` is the one the in-app updater uses, and it
  is the smaller of the two. Nothing is lost by dropping the other. If you
  installed from the MSI, you keep receiving updates - the updater falls back
  to the generic Windows entry on its own, and they arrive as the `.exe`
  installer.
- **Windows `.sha256` sidecars are gone.** `SHA256SUMS` on the release lists
  the same hash. Verify with `sha256sum --ignore-missing -c SHA256SUMS` - the
  `--ignore-missing` is what lets you check one downloaded file against a list
  naming all of them. The AppImage keeps its own `.sha256` sidecar, because it
  is also served unversioned from the rolling updater release.
- Together these take a release from twelve assets down to seven, of which two
  are the source archives GitHub attaches on its own.

**Relic reward recognition is available as an opt-in local overlay.**

- TennoWorth can watch `EE.log` for reward-screen timing, capture the Warframe
  window, recognize one to four English reward names locally with Tesseract,
  and place market and owned-count context over the choices. A manual
  `Ctrl+Shift+O` fallback is available when the log marker is missing or late.
- Recognition now reports capture, OCR, layout, and catalog-match failures
  separately. Uncertain matches may be shown, but are never marked as the best
  pick. Cached results appear first while optional live prices refresh.
- Optional diagnostics retain only the newest ten local runs and can include
  the captured reward area, name crops, OCR output, layout, results, and stage
  timings. Nothing is uploaded automatically, and diagnostics stay off by
  default because captures may contain player or game information.
- The initial supported capture paths are native window capture on Windows and
  X11, including XWayland sessions. Native Wayland capture and exclusive
  fullscreen are not supported yet.

**Inventory recommendations and desktop behavior are more accurate.**

- Set recipes preserve required duplicate components, fixing recommendations
  such as Kogake Prime needing two boots and two gauntlets. Set economics now
  distinguish current asks from instant-sale bids and explain the comparison.
- Desktop links open through the operating system with an HTTPS host allowlist.
- Riven cards no longer present unsupported values; comparisons are limited to
  the stat information supplied by the source data.
- The desktop workspace scrolls independently again without breaking the
  document-scrolling layout used on narrow screens.

## Unreleased

*Fold this into the next version's section when you run `release.ts prepare`.*

## 0.6.0 - 2026-08-22

**Linux is now distributed as an AppImage, and only as an AppImage.** If you
installed from the apt or dnf repository, or from the AUR, read this.

- **The `.deb` and `.rpm` builds are gone, and the two AUR packages are frozen
  at 0.5.0.** The AppImage is the only Linux build that can update itself -
  Tauri's updater never supported the others - so every Linux user was
  choosing between a package that goes stale silently and one that doesn't.
  One channel that works beats four that half-work.
- **Nothing you installed will break.** The apt and dnf repositories stay
  online and keep serving 0.5.0; the AUR packages still build 0.5.0. They
  simply stop receiving new versions. **To keep getting updates, download
  `TennoWorth-x86_64.AppImage` from this release** - it self-updates from
  then on.

  **Update, 2026-09-01:** the frozen apt and dnf archives have now been
  removed. Their original 0.5.0 package files remain attached to the GitHub
  release for archival use. The two AUR package names remain held by the
  project maintainer, but are still frozen and unsupported.
- **The memory-scan permission hint was wrong for AppImage users** and now
  isn't. `setcap` cannot work there: the AppImage runs from a temporary mount
  that ignores file capabilities, and the path changes every launch. The app
  now tells you to allow same-user ptrace instead, with the line to make it
  survive a reboot. It also no longer suggests a command the kernel refuses
  outright when `ptrace_scope` is 3.

## 0.5.0 - 2026-08-21

A visual overhaul: TennoWorth now has one theme in two modes, and the theme
control has moved somewhere sensible.

- **One theme, light and dark.** The paper-and-ink look is now the whole
  design - square corners, dotted rules, no glow, and an active state that
  inverts rather than changing colour. Its dark mode is new: a deep warm
  brown-black that keeps the same character with the lights off.
- **Corpus, Vitruvian and Baseline are gone.** Four half-finished looks made
  every screen a compromise; one look done properly is better. If you were on
  one of them, you'll land on the new theme automatically.
- **Theme selection moved to Settings → Appearance**, along with a new
  Settings view. Light, Dark, or System - System follows your OS and changes
  with it. It used to sit in the sidebar and on the landing page, which is not
  where a preference belongs.
- **The app is lighter.** Dropping the retired looks removed three token sets,
  their structural rules, and five bundled fonts.

## 0.4.0 - 2026-08-21

The app itself is unchanged since 0.3.8 - this release is about how releases
are built and how you can verify them.

- **Windows installers now ship `.sha256` files.** `SECURITY.md` has told you
  to check them for a while; until now they only existed for the Linux
  artifacts. The `.exe` and `.msi` both have one, in the same `sha256sum`
  format as everything else.
- **Every release carries a `SHA256SUMS`** listing all of its assets, so one
  `sha256sum -c SHA256SUMS` covers the lot.
- **Releases are now published in one step.** A release used to become public
  while parts of it were still uploading, and could go out with an asset
  missing. Now everything is built, verified and attached to a draft first,
  and made public once - so a release you can see is a release that is
  complete.
- **The verification instructions in `SECURITY.md` were corrected**, including
  a stale claim that builds were reproducible. They are publicly auditable CI
  builds; the docs now say so.
