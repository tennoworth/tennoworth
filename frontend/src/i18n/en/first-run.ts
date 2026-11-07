// The desktop first-run page.
import { m, type Source } from '../source';

export const FIRST_RUN = {
  'firstRun.heading': m('Get your personal sell list', 'Desktop first-run page, the main heading. Shown in capitals.', 40),
  'firstRun.pitch': m('Scan once and TennoWorth ranks <em>your</em> inventory by what to sell right now.', 'First-run page, under the heading. Keep <em>…</em> around the word that is stressed.'),
  'firstRun.requirement': m('Warframe must be open and past the login screen.', 'First-run page, a condition for scanning.'),
  'firstRun.scan': m('Scan inventory', 'First-run page, the main button. Reads the game\'s inventory.', 26),
  'firstRun.scanning': m('Scanning game…', 'The same button while a scan runs.', 26),
  'firstRun.trust': m("Reads the running game's memory only - nothing leaves your machine.", 'First-run page, privacy line under the button.'),
  'firstRun.lookup': m('Or look anything up', 'First-run page, label above the market search. Shown in capitals.', 32),
} satisfies Record<string, Source>;
