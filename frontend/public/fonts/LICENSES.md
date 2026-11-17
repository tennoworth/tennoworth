# Self-hosted fonts

Every face here is licensed under the SIL Open Font License 1.1 and is served
from this origin only - the app makes no third-party requests, and the CSP's
`font-src 'self'` would block them anyway. Plex Sans and Mono are the Latin woff2 subsets Google Fonts serves to modern
browsers (fetched 2026-08-17). Plex Sans Condensed weights 400–700 were fetched
from the Google Fonts repository on 2026-10-20 and subset to Latin, punctuation
and UI symbols with fonttools; glyph outlines are unchanged. The bundled
`IBM-Plex-OFL.txt` contains their copyright and license notice. Plex Math
supplies the geometric shapes the UI writes as text, which the other faces
lack; it was fetched from `@ibm/plex-math@1.1.0`
(`fonts/complete/woff2/IBMPlexMath-Regular.woff2`) on 2026-11-17 and subset
with fonttools:

    pyftsubset IBMPlexMath-Regular.woff2 --flavor=woff2 --no-hinting \
      --layout-features='' --output-file=ibm-plex-math-shapes.woff2 \
      --unicodes=U+25B2-25B4,U+25B6,U+25B8,U+25BA,U+25BC,U+25BE,U+25C0,U+25C2,U+25C8

Add a code point to both that list and the `unicode-range` in `src/app.css`.
Other non-latin glyphs (◆■□, extended Latin) fall through to the system
stacks declared in `src/app.css`.

| File | Family · weights | Used for | Copyright |
|---|---|---|---|
| `ibm-plex-sans-condensed-{400,500,600,700}-latin.woff2` | IBM Plex Sans Condensed 400–700 | labels / headings (`--font-ui`) | © IBM Corp. |
| `ibm-plex-sans-var-latin.woff2` | IBM Plex Sans, variable 100–700 | body copy (`--font-body`) | © IBM Corp. |
| `ibm-plex-mono-{400,500,600}-latin.woff2` | IBM Plex Mono 400/500/600 | numerals / code (`--font-mono`) | © IBM Corp. |
| `ibm-plex-math-shapes.woff2` | IBM Plex Math Regular, shapes only | carets and markers, first in every stack | © IBM Corp. |

The Chakra Petch, Titillium Web and Source Sans 3 subsets were dropped in
2026-08 with the corpus and vitruvian looks; yorha is the only theme and it
uses the faces above in both light and dark.

Full OFL text: https://openfontlicense.org/open-font-license-official-text/

The OFL permits bundling and redistribution with software; it forbids selling
the font files on their own and using the Reserved Font Names for modified
versions. The condensed and math files are subsets; the remaining files are unmodified.
