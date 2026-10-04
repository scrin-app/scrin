import { useTranslation } from '@scrin/i18n';
import type { ComponentProps } from 'react';

import { cn } from '../lib/cn';

export interface SpinnerProps extends ComponentProps<'span'> {
  size?: 'sm' | 'md' | 'lg';
  /** Announce to assistive tech. Off when the parent already says "loading". */
  label?: string | false;
}

const SIZES = { sm: 'size-4', md: 'size-5', lg: 'size-8' } as const;

export function Spinner({ size = 'md', label, className, ...rest }: SpinnerProps) {
  const { t } = useTranslation();
  const text = label === false ? undefined : (label ?? t('ui.loading'));
  return (
    <span
      role={text ? 'status' : undefined}
      aria-label={text}
      aria-hidden={text ? undefined : true}
      className={cn('inline-flex shrink-0', SIZES[size], className)}
      {...rest}
    >
      <svg viewBox="0 0 24 24" fill="none" className="size-full animate-spin">
        <circle cx="12" cy="12" r="9" stroke="currentColor" strokeOpacity="0.25" strokeWidth="3" />
        <path
          d="M21 12a9 9 0 0 0-9-9"
          stroke="currentColor"
          strokeWidth="3"
          strokeLinecap="round"
        />
      </svg>
    </span>
  );
}
