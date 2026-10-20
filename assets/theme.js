'use strict';
(() => {
  const read = () => { try { return localStorage.getItem('mm-theme') || 'system'; } catch (_) { return 'system'; } };
  const media = matchMedia('(prefers-color-scheme: dark)');
  const apply = (value) => {
    const mode = ['light', 'dark', 'system'].includes(value) ? value : 'system';
    document.documentElement.dataset.theme = mode === 'system' ? (media.matches ? 'dark' : 'light') : mode;
    document.documentElement.dataset.preference = mode;
  };
  apply(read());
  media.addEventListener('change', () => { if (document.documentElement.dataset.preference === 'system') apply('system'); });
  addEventListener('storage', () => apply(read()));
  addEventListener('DOMContentLoaded', () => {
    document.querySelectorAll('[data-theme-picker]').forEach(select => {
      select.value = document.documentElement.dataset.preference;
      select.addEventListener('change', () => { apply(select.value); try { localStorage.setItem('mm-theme', select.value); } catch (_) {} });
    });
  });
})();
