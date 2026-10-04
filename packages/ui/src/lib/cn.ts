import { clsx, type ClassValue } from 'clsx';
import { extendTailwindMerge } from 'tailwind-merge';

// Teach tailwind-merge about the custom theme tokens so `bg-surface bg-accent`
// collapses to the last one instead of keeping both.
const twMerge = extendTailwindMerge({
  extend: {
    theme: {
      color: [
        'bg',
        'surface',
        'surface-2',
        'fg',
        'muted',
        'outline',
        'accent',
        'accent-fg',
        'success',
        'warning',
        'danger',
      ],
      spacing: ['ctl-sm', 'ctl', 'ctl-lg', 'pad', 'gap'],
    },
  },
});

export function cn(...inputs: ClassValue[]): string {
  return twMerge(clsx(inputs));
}

/**
 * Merges a base class string with a Base UI `className` prop, which may be a
 * string or a function of the component's state.
 */
export function cx<S>(
  base: string,
  className: string | ((state: S) => string | undefined) | undefined,
): string | ((state: S) => string) {
  if (typeof className === 'function') return (state: S) => cn(base, className(state));
  return cn(base, className);
}
