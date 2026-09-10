# Website SEO review — 2026-09-10

The live homepage, response headers, robots.txt, sitemap.xml, and a nonexistent
path were inspected. The tracked website source is now in `frontend/`.

## Findings and changes

| Finding | Change |
| --- | --- |
| Homepage metadata focused on desktop inventory and omitted browser price lookup | Describe both Warframe market prices and the Windows/Linux inventory tool; align search and social titles |
| Social description claimed “Nothing leaves your machine,” despite optional listing operations | Limit the claim to inventory staying local |
| No structured site or application information | Add WebSite and SoftwareApplication JSON-LD with factual platform, license, and free-download information |
| Sitemap date was fixed at 2026-06-14 | Remove the unreliable date and optional change-frequency/priority hints; retain the canonical homepage |
| Nonexistent URL returned HTTP 200 and homepage HTML | Remove the Caddy SPA fallback; the site uses fragments, with no path-based router |
| No-JavaScript fallback lacked download links and sat below an empty viewport in the built site | Add download/source/security links, describe browser and desktop capabilities, hide the unused mount container, and reuse theme tokens |

Existing canonical and Open Graph URLs use HTTPS on tennoworth.app. Robots.txt
allows crawling and references the sitemap. The existing social PNG is 1200 × 630;
image type and alternative text are now explicit.

## Verification and rollout

- Production build and CSP synchronization check passed; `git diff --check` passed.
- Rechecked on the isolated PR branch based on current `develop`: frontend
  type checking, all 959 frontend tests, knip, hosted/desktop builds, all 529
  Rust tests (two skipped), doctests, clippy, cargo shear, and cargo audit passed.
- The real Linux desktop smoke probe passed in isolated Xvfb/D-Bus with no
  console errors or CSP violations and the desktop scan control present.
- Chromium checked the production build with JavaScript enabled and disabled,
  light/dark appearance, and 320×740, 900×480, and 1440×900 viewports. Metadata
  and JSON-LD remained present, one visible H1 rendered, and no document overflow
  or page exceptions occurred. The fallback heading now appears above the fold.
- Changes are local, not deployed. Apply the catch-all change to the actual
  server Caddy configuration and validate it before reloading; deploying the
  web archive alone does not install this repository's Caddyfile. Caddy was
  unavailable locally, so its runtime behavior remains a deployment check.
- After deployment, verify `/`, `/robots.txt`, `/sitemap.xml`, and `/og.png`
  return 200; a nonexistent page and missing asset return 404; removed package
  endpoints still return 410. Confirm HTTP and any www hostname redirect to
  the canonical HTTPS hostname.

## Prioritized follow-up

1. Prerender the hosted introduction, desktop description, and FAQ from shared
   content. The initial HTML still contains a no-JavaScript fallback rather
   than the complete rendered landing page. The interactive market can remain
   client-rendered. This is an architectural follow-up, not part of this patch.
2. Build useful, independently addressable pages for topics such as choosing
   what to sell, set-versus-parts value, and the Windows/Linux desktop tool.
   Each needs substantive content, internal links, its own metadata and
   canonical URL, and a sitemap entry. Do not create thin keyword pages or
   sitemap entries for fragment-only views.
3. Review visible FAQ accuracy before expanding content: its Riven answer
   says the feature is unsupported while the showcase includes Rivens, and
   its freshness explanation uses a six-hour threshold while HostedShell
   uses three hours.
4. Use Search Console URL Inspection and submit the sitemap after deployment.
   No Search Console data was available for this review, so indexing coverage,
   queries, rankings, and field Core Web Vitals have not been verified.
5. Refresh the social card to match the current visual identity when updating
   marketing assets. Its dimensions and format are already suitable.

Structured data describes the product; it does not guarantee a rich result.
No ratings or reviews were invented. Google's software-app rich results require
a genuine rating or review in addition to other required properties.

References: [Google SEO Starter Guide](https://developers.google.com/search/docs/fundamentals/seo-starter-guide),
[software application structured data](https://developers.google.com/search/docs/appearance/structured-data/software-app),
and [Caddy file_server behavior](https://caddyserver.com/docs/caddyfile/directives/file_server).
