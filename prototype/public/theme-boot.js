// Runs synchronously in <head> BEFORE the stylesheet paints, so a returning
// user never sees the default mode flash before their own. This is the
// ONE sanctioned raw localStorage read in the app: the state-store seam
// (src/lib/state-store.ts) is an ES module and can't execute this early, and
// the CSP's `script-src 'self'` forbids an inline script - hence a classic
// same-origin file, not a hash/nonce. Keys and the resolution rules mirror
// src/lib/theme.ts + LOCAL_SETTING_KEYS; change both together. On the desktop
// build settings live in SQLite, so this finds nothing and main.ts applies the
// theme after hydrate() - before mount, so at worst the bare ground colour
// changes once.
//
// There is only one look ('yorha'), so nothing is read for it - it is always
// stamped, because the structural CSS in app.css is scoped by the attribute.
// An old `wfminv:theme-look-v1` value from the four-look era is simply never
// read; it costs nothing to leave sitting in storage.
(function () {
  var mode = 'system';
  try {
    var m = localStorage.getItem('wfminv:theme-mode-v1');
    if (m === 'light' || m === 'dark' || m === 'system') mode = m;
  } catch (e) { /* storage disabled - defaults */ }
  if (mode === 'system') {
    mode = window.matchMedia && matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
  }
  var el = document.documentElement;
  el.setAttribute('data-look', 'yorha');
  el.setAttribute('data-mode', mode);
})();
