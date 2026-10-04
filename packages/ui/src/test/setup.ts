import { initI18n } from '@scrin/i18n';
import { cleanup } from '@testing-library/react';
import { afterEach } from 'vitest';

initI18n({ locale: 'en' });

afterEach(() => {
  cleanup();
  document.documentElement.removeAttribute('style');
  for (const a of ['data-mode', 'data-surface', 'data-density', 'data-motion']) {
    document.documentElement.removeAttribute(a);
  }
});
