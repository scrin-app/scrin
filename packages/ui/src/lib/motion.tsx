import { LazyMotion, MotionConfig } from 'motion/react';
import type { ReactNode } from 'react';

import { useOptionalTheme } from '../theme/ThemeProvider';

const loadFeatures = () => import('./motion-features').then((m) => m.default);

/**
 * LazyMotion + domMax (loaded asynchronously) with `strict`, so only the tiny
 * `m.*` components are allowed and the feature bundle never blocks first
 * render. `reducedMotion` follows the theme engine, which already folds in
 * the OS preference.
 */
export function MotionProvider({ children }: { children: ReactNode }) {
  const theme = useOptionalTheme();
  const off = theme ? theme.motionScale === 0 : false;
  return (
    <LazyMotion features={loadFeatures} strict>
      <MotionConfig reducedMotion={off ? 'always' : 'user'}>{children}</MotionConfig>
    </LazyMotion>
  );
}

/**
 * Scales a duration (seconds) by the theme's motion level: 0 when the user or
 * OS turned motion off, 0.6× for "subtle".
 */
export function useMotionDuration(seconds: number): number {
  const theme = useOptionalTheme();
  return seconds * (theme ? theme.motionScale : 1);
}

/** A spring tuned once so every surface moves the same way. */
export function useSpring(): { type: 'spring'; bounce: number; visualDuration: number } {
  const d = useMotionDuration(0.35);
  return { type: 'spring', bounce: 0.15, visualDuration: d };
}
