import { cn } from '@scrin/ui';

export function Logo({ compact = false, className }: { compact?: boolean; className?: string }) {
  return (
    <span
      className={cn('inline-flex items-center gap-2.5 font-semibold tracking-tight', className)}
    >
      <svg viewBox="0 0 32 32" aria-hidden className="size-8 shrink-0">
        <rect width="32" height="32" rx="8" className="fill-accent" />
        <rect
          x="7"
          y="8.5"
          width="18"
          height="12"
          rx="2"
          fill="none"
          strokeWidth="2.2"
          className="stroke-accent-fg"
        />
        <path
          d="M12.5 24.5h7M16 20.5v4"
          strokeWidth="2.2"
          strokeLinecap="round"
          className="stroke-accent-fg"
        />
        <circle cx="16" cy="14.5" r="2.2" className="fill-accent-fg" />
      </svg>
      {compact ? <span className="sr-only">scrin</span> : <span className="text-lg">scrin</span>}
    </span>
  );
}
