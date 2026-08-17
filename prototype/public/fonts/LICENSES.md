# Self-hosted fonts

Every face here is licensed under the SIL Open Font License 1.1 and is served
from this origin only — the app makes no third-party requests, and the CSP's
`font-src 'self'` would block them anyway. Files are the latin woff2 subsets
Google Fonts serves to modern browsers (fetched 2026-08-17 from the css2 API
with a Chrome User-Agent, latin `unicode-range` blocks only), renamed but not
re-encoded. Non-latin glyphs (▲▼◆■□, extended Latin) fall through to the
system stacks declared in `src/app.css`.

| File | Family · weights | Used by look | Copyright |
|---|---|---|---|
| `archivo-narrow-var-latin.woff2` | Archivo Narrow, variable 400–700 | yorha (labels/headings) | © Omnibus-Type (Héctor Gatti) |
| `ibm-plex-sans-var-latin.woff2` | IBM Plex Sans, variable 100–700 | yorha, corpus (body) | © IBM Corp. |
| `ibm-plex-mono-{400,500,600}-latin.woff2` | IBM Plex Mono 400/500/600 | yorha, corpus, vitruvian (numerals) | © IBM Corp. |
| `chakra-petch-{500,600,700}-latin.woff2` | Chakra Petch 500/600/700 | corpus (headings/labels) | © Cadson Demak |
| `titillium-web-600-latin.woff2` | Titillium Web 600 | vitruvian (labels/headings) | © Accademia di Belle Arti di Urbino |
| `source-sans-3-var-latin.woff2` | Source Sans 3, variable 200–900 | vitruvian (body) | © Adobe Systems Incorporated |

Full OFL text: https://openfontlicense.org/open-font-license-official-text/

The OFL permits bundling and redistribution with software; it forbids selling
the font files on their own and using the Reserved Font Names for modified
versions. Nothing here is modified.
