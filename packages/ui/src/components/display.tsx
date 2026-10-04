import { Progress as BaseProgress } from '@base-ui/react/progress';
import { cva, type VariantProps } from 'class-variance-authority';
import type { ComponentProps, ReactNode } from 'react';

import { cn } from '../lib/cn';

export function Skeleton({ className, ...rest }: ComponentProps<'div'>) {
  return (
    <div
      aria-hidden
      className={cn(
        'animate-shimmer rounded-md bg-[linear-gradient(90deg,var(--scrin-surface-2)_25%,color-mix(in_oklch,var(--scrin-outline)_55%,var(--scrin-surface-2))_50%,var(--scrin-surface-2)_75%)] bg-size-[200%_100%]',
        className,
      )}
      {...rest}
    />
  );
}

export function Progress({
  value,
  label,
  className,
}: {
  /** 0–100, or null for indeterminate. */
  value: number | null;
  label: string;
  className?: string;
}) {
  return (
    <BaseProgress.Root value={value} aria-label={label} className={cn('w-full', className)}>
      <BaseProgress.Track className="h-1.5 w-full overflow-hidden rounded-full bg-surface-2">
        <BaseProgress.Indicator
          className={cn(
            'block h-full rounded-full bg-accent transition-[width] duration-(--scrin-dur) ease-scrin',
            value === null && 'w-1/3! animate-[shimmer_1.4s_ease-in-out_infinite]',
          )}
        />
      </BaseProgress.Track>
    </BaseProgress.Root>
  );
}

export const badgeVariants = cva(
  'inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs font-medium whitespace-nowrap [&_svg]:size-3',
  {
    variants: {
      tone: {
        neutral: 'bg-surface-2 text-fg',
        accent: 'bg-accent/15 text-fg ring-1 ring-accent/40 ring-inset',
        success: 'bg-success/15 text-fg ring-1 ring-success/40 ring-inset',
        warning: 'bg-warning/15 text-fg ring-1 ring-warning/40 ring-inset',
        danger: 'bg-danger/15 text-fg ring-1 ring-danger/40 ring-inset',
      },
    },
    defaultVariants: { tone: 'neutral' },
  },
);

export function Badge({
  tone,
  className,
  ...rest
}: ComponentProps<'span'> & VariantProps<typeof badgeVariants>) {
  return <span className={cn(badgeVariants({ tone }), className)} {...rest} />;
}

/** A status dot that pairs colour with text (never colour alone). */
export function StatusDot({ online, label }: { online: boolean; label: string }) {
  return (
    <span className="inline-flex items-center gap-1.5 text-xs text-muted">
      <span
        aria-hidden
        className={cn(
          'size-2 rounded-full',
          online ? 'animate-pulse-soft bg-success' : 'bg-muted/50',
        )}
      />
      {label}
    </span>
  );
}

export function Card({
  className,
  as: Tag = 'section',
  ref: _ref,
  ...rest
}: ComponentProps<'section'> & { as?: 'section' | 'div' | 'article' | 'li' }) {
  return (
    <Tag
      className={cn(
        '@container rounded-xl glass-panel p-pad shadow-sm sm:p-[calc(var(--spacing-pad)*1.5)]',
        className,
      )}
      {...rest}
    />
  );
}

export function CardHeader({
  title,
  description,
  icon,
  actions,
  headingLevel = 2,
}: {
  title: ReactNode;
  description?: ReactNode;
  icon?: ReactNode;
  actions?: ReactNode;
  headingLevel?: 2 | 3;
}) {
  const H = headingLevel === 2 ? 'h2' : 'h3';
  return (
    <header className="mb-4 flex items-start gap-3">
      {icon ? (
        <span className="grid size-10 shrink-0 place-items-center rounded-lg bg-accent/12 text-accent [&_svg]:size-5">
          {icon}
        </span>
      ) : null}
      <div className="min-w-0 flex-1">
        <H className="text-base font-semibold text-fg">{title}</H>
        {description ? <p className="mt-0.5 text-sm text-muted">{description}</p> : null}
      </div>
      {actions ? <div className="flex shrink-0 items-center gap-1">{actions}</div> : null}
    </header>
  );
}

export function Kbd({ className, ...rest }: ComponentProps<'kbd'>) {
  return (
    <kbd
      className={cn(
        'inline-flex h-5 min-w-5 items-center justify-center rounded border border-outline bg-surface-2 px-1.5',
        'font-mono text-[0.7rem] font-medium text-muted shadow-[inset_0_-1px_0_var(--scrin-outline)]',
        className,
      )}
      {...rest}
    />
  );
}

export function EmptyState({
  icon,
  title,
  description,
  action,
  className,
}: {
  icon: ReactNode;
  title: ReactNode;
  description?: ReactNode;
  action?: ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cn(
        'flex flex-col items-center justify-center gap-3 rounded-xl border border-dashed border-outline px-6 py-12 text-center',
        className,
      )}
    >
      <span className="grid size-14 place-items-center rounded-2xl bg-surface-2 text-muted [&_svg]:size-7">
        {icon}
      </span>
      <div>
        <p className="text-base font-semibold text-fg">{title}</p>
        {description ? (
          <p className="mx-auto mt-1 max-w-sm text-sm text-muted">{description}</p>
        ) : null}
      </div>
      {action}
    </div>
  );
}
