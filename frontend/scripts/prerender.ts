import { readFile, writeFile, mkdir, rm } from 'node:fs/promises';
import { resolve } from 'node:path';
import { build } from 'vite';
import { pathToFileURL } from 'node:url';
import { pages, pilotItems, itemPath } from '../src/public-site/pages';
import { activityCandidates, decisionAnswer, decisionText, ducatParts, publicData } from '../src/public-site/model';
import type { Market } from '../src/contracts/data';

const root = resolve(import.meta.dirname, '..');
const renderDir = resolve(root, 'target/public-render');
await build({ root, logLevel: 'warn', build: { ssr: resolve(root, 'src/public-site/render.ts'), outDir: renderDir, emptyOutDir: true, copyPublicDir: false } });
try {
  const { render, PublicPage, PublicHome } = await import(pathToFileURL(resolve(renderDir, 'render.js')).href);
  const market = JSON.parse(await readFile(resolve(root, 'public/market.json'), 'utf8')) as Market;
  const selected = pilotItems;
  if (selected.length < 10 || selected.some(slug => !market.items[slug])) throw new Error('Item pilot has incomplete snapshot coverage');
  const complete = publicData(market);
  const shortlist = ducatParts(complete).sort((a, b) => a.price! / a.ducats! - b.price! / b.ducats!).slice(0, 60);
  const initialSlugs = new Set([...selected, ...shortlist.map(row => row.slug), ...activityCandidates(complete).map(row => row.slug)]);
  const initial = { ...complete, items: complete.items.filter(item => initialSlugs.has(item.slug)) };
  let template = await readFile(resolve(root, 'dist/index.html'), 'utf8');
  template = template.replace(/<noscript>[\s\S]*?<\/noscript>/g, '');
  const home = render(PublicHome);
  await writeFile(resolve(root, 'dist/index.html'), template.replace('<div id="app"></div>', `<div id="app">${home.body}</div>`));
  const routes = [...pages, ...selected.map(slug => ({ path: itemPath(slug), kind: 'item', slug, title: `${initial.items.find(i => i.slug === slug)!.name} Prices & Selling Context`, heading: initial.items.find(i => i.slug === slug)!.name, description: `Dated ${initial.items.find(i => i.slug === slug)!.name} Warframe prices, recent median observations and selling context. Compare sets, parts and ducat value.` }))];
  // Dedicated routes render shared components before client chunks can load their CSS.
  const manifest = JSON.parse(await readFile(resolve(root, 'dist/.vite/manifest.json'), 'utf8')) as Record<string, { css?: string[]; imports?: string[] }>;
  const styles = new Set<string>();
  const visited = new Set<string>();
  function collectStyles(key: string) {
    if (visited.has(key)) return;
    visited.add(key);
    const chunk = manifest[key];
    if (!chunk) throw new Error(`Missing public-page build chunk: ${key}`);
    for (const css of chunk.css ?? []) styles.add(css);
    for (const dependency of chunk.imports ?? []) collectStyles(dependency);
  }
  collectStyles('src/shells/PublicPage.svelte');
  const routeTemplate = template.replace('</head>', [...styles].map(css => `<link rel="stylesheet" href="/${css}">`).join('') + '</head>');
  const escape = (s: string) => s.replaceAll('&','&amp;').replaceAll('"','&quot;').replaceAll('<','&lt;').replaceAll('>','&gt;');
  for (const route of routes) {
    const props = { heading: route.heading, kind: route.kind, slug: 'slug' in route ? route.slug : '', initial, published: selected };
    const rendered = render(PublicPage, { props });
    const decision = 'slug' in route ? decisionAnswer(initial, route.slug) : null;
    const social = decision ? decisionText(decision) : null;
    const socialTitle = social ? `${social.ogTitle} | TennoWorth` : `${route.title} | TennoWorth`;
    const socialDescription = social ? social.ogDescription : route.description;
    const url = `https://tennoworth.app${route.path}`;
    const html = routeTemplate.replace(/<title>[\s\S]*?<\/title>/, `<title>${escape(route.title)} | TennoWorth</title>`)
      .replace(/(<meta\s+name="description"\s+content=")[^"]*/, `$1${escape(route.description)}`)
      .replace(/(<link rel="canonical" href=")[^"]*/, `$1${url}`)
      .replace(/(<meta property="og:url" content=")[^"]*/, `$1${url}`)
      .replace(/(<meta (?:property="og:title"|name="twitter:title") content=")[^"]*/g, `$1${escape(socialTitle)}`)
      .replace(/(<meta\s+(?:property="og:description"|name="twitter:description")\s+content=")[^"]*/g, `$1${escape(socialDescription)}`)
      .replace('<div id="app"></div>', `<div id="app" data-public-page>${rendered.body}</div><script id="public-page-data" type="application/json">${JSON.stringify(props).replaceAll('<','\\u003c')}</script>`);
    const directory = resolve(root, `dist${route.path}`);
    await mkdir(directory, { recursive: true });
    await writeFile(resolve(directory, 'index.html'), html);
  }
  await writeFile(resolve(root, 'dist/sitemap.xml'), `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">${['/', ...routes.map(r => r.path)].map(path => `<url><loc>https://tennoworth.app${path}</loc></url>`).join('')}</urlset>\n`);
  await rm(resolve(root, 'dist/.vite'), { recursive: true });
  console.log(`Prerendered homepage and ${routes.length} public pages.`);
} finally { await rm(renderDir, { recursive: true, force: true }); }
