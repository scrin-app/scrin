import { initI18n } from '@scrin/i18n';
import { cleanup } from '@testing-library/react';
import { MotionGlobalConfig } from 'motion/react';
import { afterEach } from 'vitest';

initI18n({ locale: 'en' });
// happy-dom's WAAPI rejects `finished` on cancel; Motion's own test switch
// makes every animation instant instead.
MotionGlobalConfig.skipAnimations = true;

afterEach(() => {
  cleanup();
});
