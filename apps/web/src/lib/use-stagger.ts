import { useMotionDuration } from '@scrin/ui';

/**
 * Enter animation for a grid of cards: fade + rise, staggered. Transform and
 * opacity only (compositor), collapsed to nothing under reduced motion.
 */
export function useStagger() {
  const d = useMotionDuration(0.36);
  return (index: number) =>
    d === 0
      ? {}
      : {
          initial: { opacity: 0, y: 12 },
          animate: { opacity: 1, y: 0 },
          transition: { duration: d, delay: index * d * 0.18, ease: [0.22, 1, 0.36, 1] as const },
        };
}
