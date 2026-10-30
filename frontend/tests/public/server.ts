import { resolve, sep } from 'node:path';
const root = resolve('dist');
Bun.serve({
  hostname: '127.0.0.1', port: 5189,
  async fetch(request) {
    const url = new URL(request.url);
    let path: string;
    try { path = resolve(root, '.' + decodeURIComponent(url.pathname)); }
    catch { return new Response('Invalid path', { status:400 }); }
    if (path !== root && !path.startsWith(root + sep)) return new Response('Not found', { status:404 });
    const index = Bun.file(resolve(path, 'index.html'));
    if (await index.exists()) {
      if (!url.pathname.endsWith('/')) return Response.redirect(new URL(url.pathname + '/', url), 308);
      return new Response(index);
    }
    const file = Bun.file(path);
    if (await file.exists()) return new Response(file);
    // Mirrors deploy/caddy/Caddyfile's handle_errors: the built page with a 404 status.
    return new Response(Bun.file(resolve(root, '404.html')), { status:404 });
  },
});
