import { readdirSync, readFileSync, existsSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const frontend = fileURLToPath(new URL('../frontend/', import.meta.url));
for (const output of ['dist', 'dist-desktop']) {
  const root = join(frontend, output);
  if (!existsSync(join(root, 'index.html'))) throw new Error(`Build ${output} before checking artifact boundaries`);
  if (existsSync(join(root, 'admin.html'))) throw new Error(`Private dashboard entry leaked into ${output}`);
  for (const file of readdirSync(join(root, 'assets')).filter(file => file.endsWith('.js'))) {
    // The API route is fixed by the server, so a wording change cannot hide a leak.
    const source = readFileSync(join(root, 'assets', file), 'utf8');
    if (['/api/announcements/change', '/api/jobs/run', 'Publish announcement'].some(marker => source.includes(marker))) throw new Error(`Publisher UI leaked into ${output}`);
  }
}
const admin = join(frontend, 'dist-admin');
if (!existsSync(join(admin, 'admin.html'))) throw new Error('Build the private dashboard before checking its artifacts');
const permitted = new Set(['admin.html', 'assets', 'fonts', 'favicon.svg']);
for (const file of readdirSync(admin)) if (!permitted.has(file)) throw new Error(`Unexpected public payload in dashboard: ${file}`);
console.log('Private dashboard and public/desktop artifacts are separate');
