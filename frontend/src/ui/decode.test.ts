import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { decodeChange } from './decode';

let frames: FrameRequestCallback[] = [];
let now = 0;
const runFrame = (at: number) => {
  now = at;
  const due = frames;
  frames = [];
  for (const callback of due) callback(at);
};

beforeEach(() => {
  frames = [];
  now = 0;
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => frames.push(callback));
  vi.stubGlobal('cancelAnimationFrame', () => { frames = []; });
  vi.spyOn(performance, 'now').mockImplementation(() => now);
});
afterEach(() => vi.unstubAllGlobals());

function page() {
  const root = document.createElement('div');
  root.innerHTML = '<h1>Settings</h1><span>Ivara Prime Neuroptics</span><dialog data-no-decode><p>Continue</p></dialog>';
  return root;
}

it('decodes only the text a language switch rewrote', async () => {
  const root = page();
  const [title, item, picker] = [root.querySelector('h1')!, root.querySelector('span')!, root.querySelector('p')!];
  await decodeChange(root, 400, async () => {
    title.textContent = 'Einstellungen';
    picker.textContent = 'Weiter';
  });
  runFrame(100);
  expect(title.textContent).not.toBe('Einstellungen');
  expect([...title.textContent!].every((c) => 'Einstellungen'.includes(c))).toBe(true);
  expect(item.textContent).toBe('Ivara Prime Neuroptics');
  expect(picker.textContent).toBe('Weiter');
  runFrame(400);
  expect(title.textContent).toBe('Einstellungen');
});

it('settles a running decode before the next switch, so no word from before stays', async () => {
  const root = page();
  const title = root.querySelector('h1')!;
  await decodeChange(root, 400, async () => { title.textContent = 'Einstellungen'; });
  runFrame(100);
  await decodeChange(root, 400, async () => {
    expect(title.textContent).toBe('Einstellungen');
    title.textContent = 'Paramètres';
  });
  runFrame(500);
  runFrame(1000);
  expect(title.textContent).toBe('Paramètres');
});

it('only switches under reduced motion', async () => {
  const root = page();
  const title = root.querySelector('h1')!;
  await decodeChange(root, 0, async () => { title.textContent = 'Einstellungen'; });
  expect(frames).toEqual([]);
  expect(title.textContent).toBe('Einstellungen');
});
