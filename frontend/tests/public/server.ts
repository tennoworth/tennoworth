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
    return await file.exists() ? new Response(file) : new Response('Not found', { status:404 });
  },
});
