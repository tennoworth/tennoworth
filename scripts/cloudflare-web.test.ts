import { expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

// The site's static files are served by Cloudflare, while the box keeps the
// live data and collector paths. Both halves are configured in this repository
// and must describe the same site.
const read = (path: string) => readFileSync(new URL(`../${path}`, import.meta.url), 'utf8');
const caddy = read('deploy/caddy/Caddyfile');
const routes = JSON.parse(read('deploy/cloudflare-web/routes.json')) as { site: string; box: string[]; worker: string };
const wrangler = JSON.parse(read('deploy/cloudflare-web/wrangler.jsonc').replace(/^\s*\/\/.*$/gm, ''));

function caddyHeaders() {
  const block = caddy.match(/\(public_headers\) \{\s*header \{([\s\S]*?)\n\t\}/)![1];
  return Object.fromEntries([...block.matchAll(/^\s*([A-Za-z-]+) "(.*)"$/gm)].map(m => [m[1], m[2]]));
}

function staticHeaders(path: string) {
  const block = read('frontend/public/_headers').split(/\n\s*\n/).find(b => b.startsWith(`${path}\n`))!;
  return Object.fromEntries(block.split('\n').slice(1).filter(line => line.trim()).map(line => {
    const at = line.indexOf(':');
    return [line.slice(0, at).trim(), line.slice(at + 1).trim()];
  }));
}

// Every path the box answers itself, as the Caddy site block names it.
function boxPaths() {
  const site = caddy.slice(caddy.indexOf(':8081 {'), caddy.indexOf('handle_errors {'));
  const handled = [...site.matchAll(/^\s*handle (\/\S+) \{/gm)].map(m => m[1]);
  const matchers = [...site.matchAll(/^\s*@\w+ path (.+)$/gm)].flatMap(m => m[1].trim().split(/\s+/));
  return [...handled, ...matchers];
}

const routeRegex = (pattern: string) =>
  new RegExp(`^${pattern.replace(/[.+?^${}()|[\]\\]/g, '\\$&').replace(/\*/g, '.*')}$`);
const toBox = (path: string) => routes.box.some(pattern => routeRegex(pattern).test(`tennoworth.app${path}`));

test('Cloudflare sends the site the same security headers as the box', () => {
  expect(staticHeaders('/*')).toEqual(caddyHeaders());
  expect(staticHeaders('/assets/*')).toEqual({ 'Cache-Control': 'public, max-age=31536000, immutable' });
});

test('every path the box serves bypasses the Worker', () => {
  const paths = boxPaths();
  expect(paths).toContain('/market.json');
  expect(paths).toContain('/api/reports/*');
  for (const path of paths) expect({ path, box: toBox(path.replace(/\*$/, 'x')) }).toEqual({ path, box: true });
  for (const page of ['/', '/what-to-sell/', '/items/bo_prime_set/', '/assets/index.js', '/404.html']) {
    expect({ page, box: toBox(page) }).toEqual({ page, box: false });
  }
  expect(routeRegex(routes.site).test('tennoworth.app/what-to-sell/')).toBe(true);
});

test('box-served files are never uploaded as stale static copies', () => {
  const ignored = read('frontend/public/.assetsignore').split('\n').filter(line => line.startsWith('/'));
  const files = boxPaths().filter(path => /^\/[\w.-]+\.json$/.test(path));
  expect(files.length).toBeGreaterThan(5);
  for (const file of files) expect(ignored).toContain(file);
});

test('a deploy cannot move traffic', () => {
  expect(wrangler.name).toBe(routes.worker);
  expect('routes' in wrangler || 'route' in wrangler).toBe(false);
  expect(wrangler.workers_dev).toBe(false);
  expect(wrangler.main).toBeUndefined();
  expect(wrangler.assets.directory).toBe('../../frontend/dist');
  expect(wrangler.assets.not_found_handling).toBe('404-page');
});
