# Translating TennoWorth

TennoWorth is being translated into every language Warframe supports. Each
language starts as an **unreviewed draft**; players who speak it turn the
drafts into text that reads naturally. You do not need to be a developer, and
reviewing a few lines helps.

The app does not offer a language choice yet. When it does, it will offer
every language below except Thai, drafts included, and fix wording as players
report it. Thai stays in review for longer: warframe.market has no Thai item
names, so the reward overlay would read Thai rewards in English. Its drafts can
be reviewed in the meantime, and the preview shows them (`&lang=th`).

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
  only `other`). A form such as `=0 {none}` is for that exact number.
- **Choices:** `{kind, select, set {...} other {...}}` picks text by a word the
  app passes. Translate the text inside the inner braces, never the words
  before them.
- **Keep tags:** a message may mark words with `<em>` (stress), `<b>` (bold),
  `<code>` (a value to type), `<kbd>` (a key), `<good>` or `<bad>` (green or
  red). Keep each pair around the matching words in your language, with the
  same tags as the English.
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

### Adding or changing a message

English lives in one module per area: `frontend/src/i18n/en/<area>.ts`, merged
into `EN` by `frontend/src/i18n/en.ts`. Each entry is a key, the English, a note
for translators and, where space is tight, a length limit:

```ts
export const BARO = {
  'baro.verdict.flip': m('buy · flip', 'Verdict tag: buy and resell now.', 14),
} satisfies Record<string, Source>;
```

Keys are `area.name` and typed, so `t('baro.verdict.flip')` and
``t(`baro.verdict.${verdict}`)`` fail to compile when a key is missing. A new
area gets its own module, spread into `EN`. The note says where the text
appears; a translator sees nothing else.

After changing English, run from `frontend/`:

```sh
bun scripts/i18n.ts --write   # add, draft or drop the key in every catalog
bun scripts/i18n.ts           # check every catalog and print each language's progress
```

A changed English message turns its translations back into drafts that keep the
old English (`#| msgid`) for the reviewer. `bun run test` runs the same check.

### Message syntax

Messages use a small part of ICU MessageFormat: `{name}`, `{n, number}`,
`{n, plural, =0 {...} one {...} few {...} many {...} other {...}}` with `#` for
the number, and `{x, select, a {...} other {...}}`. A plain `{x}` given a
number is formatted for the language, with grouping, so pass a year, an id or
an already formatted value as a string.

Rich text uses six tags: `<em>`, `<b>`, `<code>`, `<kbd>`, `<good>` and
`<bad>`. Render such a message with `<Rich text={t('key')} />`
(`frontend/src/ui/Rich.svelte`): the tags become elements and the rest stays
text, never HTML. The elements belong to `Rich`, so a parent styles them with
`:global(b)` and similar. A translation must carry the same tags as the English.

### Rules for code

- Call `t()` at render time: in markup, a `$derived`, or a function. A
  module-level constant is computed once, in English, before the language
  loads.
- Do not name a local variable `t`; it hides the translator.
- Format dates and numbers with `toLocale*(formatLocale())`. In English it is
  the system's locale; otherwise the app's language.
- `domain/` does not import `i18n` (the architecture gate refuses it). It
  returns codes and data, and the view words them.
- Never compare displayed text in logic. Branch on a kind or code, then
  translate it.

### How the language loads

The desktop loads its language before it mounts (`startLanguage` in
`frontend/src/i18n/startup.ts`), so nothing shows in English first. The reward
overlay window cannot read settings; it reads the language the main window
saved under `tennoworth.app-language` in local storage
(`startSideWindowLanguage`). The hosted website stays English.

### Previewing

To see the app in a language in the desktop preview, add `&lang=<code>` to the
preview URL (`/?preview-desktop&lang=fr`). `&lang=pseudo` shows a fake
language: accented and about 40% longer. It finds text that skipped the catalog
and layouts that cannot grow.

### Still in English

These do not go through the catalogs yet:

- text made by the native Rust code: Trade Session reasons and errors,
  warframe.market access messages, whisper text, and the in-game notice card;
- the bundled release notes;
- the public website and the private dashboard;
- feedback reports sent to GitHub, which stay English on purpose so maintainers
  can read them.
