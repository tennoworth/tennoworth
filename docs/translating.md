# Translating TennoWorth

TennoWorth is being translated into every language Warframe supports. Each
language starts as an **unreviewed draft**; players who speak it turn the
drafts into text that reads naturally. You do not need to be a developer, and
reviewing a few lines helps.

The app does not offer a language choice yet. When it does, it will offer
every language whose item names warframe.market provides, drafts included, and
fix wording as players report it. Thai waits until its item names are
available; its drafts can be reviewed in the meantime.

## What to review

Each language has one file: `frontend/src/i18n/locales/<code>.po`.

| Code | Language | Code | Language |
|---|---|---|---|
| `de` | Deutsch | `ru` | Русский |
| `es` | Español | `uk` | Українська |
| `fr` | Français | `tr` | Türkçe |
| `it` | Italiano | `ja` | 日本語 |
| `pt` | Português (Brazil) | `ko` | 한국어 |
| `pl` | Polski | `zh-hans` | 简体中文 |
| `th` | ไทย | `zh-hant` | 繁體中文 |

Every message in the file looks like this:

```po
#. First-run page, the main button. Reads the game's inventory.
#. Max 26 characters.
#, fuzzy
msgctxt "firstRun.scan"
msgid "Scan inventory"
msgstr "Scanner l’Inventaire"
```

- `#.` lines say where the text appears and how much room it has.
- `msgid` is the English. `msgstr` is your language.
- `#, fuzzy` means **not reviewed yet**.

## How to review on GitHub

1. Open your language's file on GitHub and press the pencil icon to edit it.
2. For each message:
   - If the translation is right, delete its `#, fuzzy` line (and a
     `#| msgid` line under it, if there is one).
   - If it is wrong or sounds unnatural, fix the `msgstr` text, then delete
     `#, fuzzy`.
   - If you are unsure, leave `#, fuzzy` in place.
3. Choose **Propose changes**, then **Create pull request**. Say which language
   you reviewed and anything you were unsure about.

Prefer not to edit files? Open a **Translation review** issue instead and
paste the corrections there.

## Rules that keep the app working

A check runs on every change and points at any of these that break.

- **Keep everything in braces exactly as it is.** `{summary}`, `{age}` and
  `{count}` are filled in by the app. Move them anywhere in the sentence, but do
  not translate or rename them.
- **Plurals:** `{count, plural, one {# item} other {# items}}` picks a form by
  number, and `#` is the number. Translate only the text inside the inner
  braces. Use the forms your language has: `one`, `few`, `many`, `other` (Polish,
  Russian and Ukrainian use all four; Japanese, Korean, Chinese and Thai need
  only `other`).
- **Keep tags:** `<em>your</em>` marks the stressed word; keep the `<em>` and
  `</em>` around the matching word in your language.
- **Respect the length notes** where they exist. Short labels sit in a tight bar;
  a longer word may be cut off. Shorter is usually better.
- **Write how the game writes.** Use the words Warframe uses in your language.
  [`glossary.json`](../frontend/src/i18n/glossary.json) lists them: entries
  marked `game` come from the game's own text, and entries marked `verify` are
  drafts to check against your game.
- **Do not translate item names.** They come from warframe.market in your
  language. Keep `TennoWorth`, `warframe.market`, `Warframe` and `Prime` as
  they are.

## For developers

Translatable text lives in `frontend/src/i18n/en.ts`: a key, the English, a note
for translators and, where space is tight, a length limit. Components call
`t('key', { args })`. After changing `en.ts`, run from `frontend/`:

```sh
bun scripts/i18n.ts --write   # add, draft or drop the key in every catalog
bun scripts/i18n.ts           # check every catalog and print each language's progress
```

A changed English message turns its translations back into drafts that keep the
old English (`#| msgid`) for the reviewer. `bun run test` runs the same check.

To see the app in a language in the desktop preview, add `&lang=<code>` to the
preview URL (`/?preview-desktop&lang=fr`). `&lang=pseudo` shows a fake
language: accented and about 40% longer. It finds text that skipped the catalog
and layouts that cannot grow.
