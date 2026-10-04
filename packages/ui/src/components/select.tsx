import { Select as BaseSelect } from '@base-ui/react/select';
import { Check, ChevronsUpDown } from 'lucide-react';
import type { ReactNode } from 'react';

import { cn } from '../lib/cn';
import { inputClass } from './field';
import { floatingClass } from './popover';

export interface SelectOption<V extends string> {
  value: V;
  label: ReactNode;
}

export interface SelectProps<V extends string> {
  value: V;
  onValueChange: (value: V) => void;
  options: readonly SelectOption<V>[];
  /** Accessible name when there is no visible <label>. */
  label?: string;
  id?: string;
  className?: string;
}

export function Select<V extends string>({
  value,
  onValueChange,
  options,
  label,
  id,
  className,
}: SelectProps<V>) {
  const items = Object.fromEntries(options.map((o) => [o.value, o.label]));
  return (
    <BaseSelect.Root
      items={items}
      value={value}
      onValueChange={(v: unknown) => {
        const picked = options.find((o) => o.value === v);
        if (picked) onValueChange(picked.value);
      }}
    >
      <BaseSelect.Trigger
        {...(id ? { id } : {})}
        {...(label ? { 'aria-label': label } : {})}
        className={cn(
          inputClass,
          'flex cursor-pointer items-center justify-between gap-2 text-left',
          className,
        )}
      >
        <BaseSelect.Value />
        <BaseSelect.Icon>
          <ChevronsUpDown aria-hidden className="size-4 text-muted" />
        </BaseSelect.Icon>
      </BaseSelect.Trigger>
      <BaseSelect.Portal>
        <BaseSelect.Positioner sideOffset={6} collisionPadding={8} alignItemWithTrigger={false}>
          <BaseSelect.Popup className={cn(floatingClass, 'min-w-(--anchor-width)')}>
            <BaseSelect.List>
              {options.map((o) => (
                <BaseSelect.Item
                  key={o.value}
                  value={o.value}
                  className={cn(
                    'flex min-h-9 cursor-default items-center gap-2 rounded-md px-2.5 pr-8 text-sm outline-none select-none',
                    'relative data-highlighted:bg-surface-2',
                  )}
                >
                  <BaseSelect.ItemText>{o.label}</BaseSelect.ItemText>
                  <BaseSelect.ItemIndicator className="absolute right-2.5">
                    <Check aria-hidden className="size-4 text-accent" />
                  </BaseSelect.ItemIndicator>
                </BaseSelect.Item>
              ))}
            </BaseSelect.List>
          </BaseSelect.Popup>
        </BaseSelect.Positioner>
      </BaseSelect.Portal>
    </BaseSelect.Root>
  );
}
