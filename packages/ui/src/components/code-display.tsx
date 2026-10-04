import { useTranslation } from '@scrin/i18n';
import { Check, Copy } from 'lucide-react';
import { useEffect, useState, type ReactNode } from 'react';

import { cn } from '../lib/cn';
import { IconButton } from './button';

export interface CodeDisplayProps {
  /** Already-formatted value, e.g. `123 456 789`. */
  value: string;
  /** Raw value placed on the clipboard (defaults to `value` without spaces). */
  copyValue?: string;
  label: string;
  onCopy?: (text: string) => Promise<void> | void;
  /** Epoch ms window for the countdown ring; omit for no ring. */
  countdown?: { issuedAt: number; expiresAt: number };
  size?: 'md' | 'lg';
  actions?: ReactNode;
  className?: string;
}

/**
 * The big monospace ID / one-time code with copy and an optional countdown
 * ring. The value is selectable text, not an image, so it can be read by
 * screen readers and copied by hand.
 */
export function CodeDisplay({
  value,
  copyValue,
  label,
  onCopy,
  countdown,
  size = 'lg',
  actions,
  className,
}: CodeDisplayProps) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (!copied) return undefined;
    const id = setTimeout(() => setCopied(false), 1600);
    return () => clearTimeout(id);
  }, [copied]);

  const copy = async () => {
    const text = copyValue ?? value.replace(/\s/g, '');
    await onCopy?.(text);
    setCopied(true);
  };

  return (
    <div className={cn('flex items-center gap-3', className)}>
      {countdown ? <CountdownRing {...countdown} /> : null}
      <div className="min-w-0 flex-1">
        <p className="text-xs font-medium tracking-wide text-muted uppercase">{label}</p>
        <output
          aria-label={label}
          className={cn(
            'block font-mono font-semibold tracking-[0.08em] text-fg tabular-nums select-all',
            size === 'lg' ? 'text-[clamp(1.5rem,7cqi,2.5rem)] leading-tight' : 'text-xl',
          )}
        >
          {value}
        </output>
      </div>
      <div className="flex shrink-0 items-center gap-1">
        {actions}
        {onCopy ? (
          <IconButton
            label={copied ? t('common.copied') : t('ui.copyToClipboard')}
            onClick={() => void copy()}
          >
            {copied ? <Check aria-hidden className="text-success" /> : <Copy aria-hidden />}
          </IconButton>
        ) : null}
      </div>
    </div>
  );
}

/** Remaining-time ring; updates once per second, announces nothing per tick. */
export function CountdownRing({ issuedAt, expiresAt }: { issuedAt: number; expiresAt: number }) {
  const { t } = useTranslation();
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, []);
  const total = Math.max(1, expiresAt - issuedAt);
  const left = Math.max(0, expiresAt - now);
  const ratio = left / total;
  const seconds = Math.ceil(left / 1000);
  const r = 18;
  const circ = 2 * Math.PI * r;
  const tone = ratio > 0.25 ? 'text-accent' : ratio > 0.08 ? 'text-warning' : 'text-danger';
  return (
    <span className={cn('relative grid size-12 shrink-0 place-items-center', tone)}>
      <span className="sr-only">{t('ui.countdown', { seconds })}</span>
      <svg viewBox="0 0 44 44" className="absolute inset-0 size-full -rotate-90" aria-hidden>
        <circle cx="22" cy="22" r={r} fill="none" stroke="var(--scrin-surface-2)" strokeWidth="4" />
        <circle
          cx="22"
          cy="22"
          r={r}
          fill="none"
          stroke="currentColor"
          strokeWidth="4"
          strokeLinecap="round"
          strokeDasharray={circ}
          strokeDashoffset={circ * (1 - ratio)}
          className="transition-[stroke-dashoffset] duration-1000 ease-linear"
        />
      </svg>
      <span aria-hidden className="font-mono text-[0.65rem] font-semibold text-fg tabular-nums">
        {seconds >= 60 ? `${Math.floor(seconds / 60)}m` : `${seconds}s`}
      </span>
    </span>
  );
}
