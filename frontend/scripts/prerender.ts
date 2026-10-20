import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { resolve } from 'node:path';
import { createServer } from 'vite';
import { pages, pilotItems, itemPath } from '../src/public-site/pages';
import { publicData } from '../src/public-site/model';
import type { Market } from '../src/contracts/data';

const root = resolve(import.meta.dirname, '..');
const server = await createServer({ root, server: { middlewareMode: true }, appType: 'custom' });
try {
  const { render } = await server.ssrLoadModule('/src/public-site/render.ts');
  const { default: PublicPage } = await server.ssrLoadModule('/src/shells/PublicPage.svelte');
  const { default: PublicHome } = await server.ssrLoadModule('/src/shells/PublicHome.svelte');
  const market = JSON.parse(await readFile(resolve(root, 'public/market.json'), 'utf8')) as Market;
  const selected = pilotItems;
  if (selected.length < 10 || selected.some(slug => !market.items[slug])) throw new Error('Item pilot has incomplete snapshot coverage');
  const initial = publicData(market, [...new Set([...selected, ...Object.entries(market.items).filter(([,i]) => i.ducats && i.median_now).sort(([,a],[,b]) => a.median_now! / a.ducats! - b.median_now! / b.ducats!).slice(0,60).map(([slug]) => slug)])]);
  let template = await readFile(resolve(root, 'dist/index.html'), 'utf8');
  template = template.replace(/<noscript>[\s\S]*?<\/noscript>/g, '');
  const home = render(PublicHome);
  await writeFile(resolve(root, 'dist/index.html'), template.replace('<div id="app"></div>', `<div id="app">${home.body}</div>`));
  const routes = [...pages, ...selected.map(slug => ({ path: itemPath(slug), kind: 'item', slug, title: `${initial.items.find(i => i.slug === slug)!.name} Prices & Selling Context`, heading: initial.items.find(i => i.slug === slug)!.name, description: `Dated ${initial.items.find(i => i.slug === slug)!.name} Warframe prices, recent median observations and selling context. Compare sets, parts and ducat value.` }))];
  const escape = (s: string) => s.replaceAll('&','&amp;').replaceAll('"','&quot;').replaceAll('<','&lt;').replaceAll('>','&gt;');
  for (const route of routes) {
    const props = { heading: route.heading, kind: route.kind, slug: 'slug' in route ? route.slug : '', initial, published: selected };
    const rendered = render(PublicPage, { props });
    const url = `https://tennoworth.app${route.path}`;
    let html = template.replace(/<title>[\s\S]*?<\/title>/, `<title>${escape(route.title)} | TennoWorth</title>`)
      .replace(/(<meta\s+name="description"\s+content=")[^"]*/, `$1${escape(route.description)}`)
      .replace(/(<link rel="canonical" href=")[^"]*/, `$1${url}`)
      .replace(/(<meta property="og:url" content=")[^"]*/, `$1${url}`)
      .replace(/(<meta (?:property="og:title"|name="twitter:title") content=")[^"]*/g, `$1${escape(route.title)} | TennoWorth`)
      .replace(/(<meta\s+(?:property="og:description"|name="twitter:description")\s+content=")[^"]*/g, `$1${escape(route.description)}`)
      .replace('<div id="app"></div>', `<div id="app" data-public-page>${rendered.body}</div><script id="public-page-data" type="application/json">${JSON.stringify(props).replaceAll('<','\\u003c')}</script>`);
    const directory = resolve(root, `dist${route.path}`);
    await mkdir(directory, { recursive: true });
    await writeFile(resolve(directory, 'index.html'), html);
  }
  await writeFile(resolve(root, 'dist/sitemap.xml'), `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">${['/', ...routes.map(r => r.path)].map(path => `<url><loc>https://tennoworth.app${path}</loc></url>`).join('')}</urlset>\n`);
  console.log(`Prerendered homepage and ${routes.length} public pages.`);
} finally { await server.close(); }
