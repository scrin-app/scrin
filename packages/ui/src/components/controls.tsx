import { Checkbox as BaseCheckbox } from '@base-ui/react/checkbox';
import { Radio } from '@base-ui/react/radio';
import { RadioGroup as BaseRadioGroup } from '@base-ui/react/radio-group';
import { Slider as BaseSlider } from '@base-ui/react/slider';
import { Switch as BaseSwitch } from '@base-ui/react/switch';
import { Check } from 'lucide-react';
import { useId, type ComponentProps, type ReactNode } from 'react';

import { cn, cx } from '../lib/cn';

const focusRing =
  'focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent';

export function Switch({ className, ...rest }: ComponentProps<typeof BaseSwitch.Root>) {
  return (
    <BaseSwitch.Root
      className={cx(
        cn(
          'relative inline-flex h-6 w-11 shrink-0 cursor-pointer items-center rounded-full border border-outline bg-surface-2 p-0.5',
          'transition-colors duration-(--scrin-dur-fast) data-checked:border-accent data-checked:bg-accent',
          'data-disabled:cursor-not-allowed data-disabled:opacity-50',
          focusRing,
        ),
        className,
      )}
      {...rest}
    >
      <BaseSwitch.Thumb
        className={cn(
          'block size-4.5 rounded-full bg-fg shadow transition-[translate,background-color] duration-(--scrin-dur-fast) ease-scrin',
          'data-checked:translate-x-5 data-checked:bg-accent-fg',
        )}
      />
    </BaseSwitch.Root>
  );
}

/** A labelled row with a switch on the right, the common settings pattern. */
export function SwitchRow({
  label,
  description,
  checked,
  onCheckedChange,
  disabled,
}: {
  label: ReactNode;
  description?: ReactNode;
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  disabled?: boolean;
}) {
  const id = useId();
  return (
    <div className="flex items-center justify-between gap-4 py-2">
      <div className="min-w-0">
        <label htmlFor={id} className="block text-sm font-medium text-fg">
          {label}
        </label>
        {description ? (
          <p id={`${id}-d`} className="mt-0.5 text-xs text-muted">
            {description}
          </p>
        ) : null}
      </div>
      <Switch
        id={id}
        checked={checked}
        onCheckedChange={(c: boolean) => onCheckedChange(c)}
        {...(disabled !== undefined ? { disabled } : {})}
        {...(description ? { 'aria-describedby': `${id}-d` } : {})}
      />
    </div>
  );
}

export function Checkbox({ className, ...rest }: ComponentProps<typeof BaseCheckbox.Root>) {
  return (
    <BaseCheckbox.Root
      className={cx(
        cn(
          'inline-flex size-5 shrink-0 cursor-pointer items-center justify-center rounded-sm border border-outline bg-surface',
          'transition-colors duration-(--scrin-dur-fast) data-checked:border-accent data-checked:bg-accent',
          'data-disabled:cursor-not-allowed data-disabled:opacity-50',
          focusRing,
        ),
        className,
      )}
      {...rest}
    >
      <BaseCheckbox.Indicator className="text-accent-fg data-unchecked:hidden">
        <Check aria-hidden className="size-3.5" strokeWidth={3} />
      </BaseCheckbox.Indicator>
    </BaseCheckbox.Root>
  );
}

export interface RadioOption<V extends string> {
  value: V;
  label: ReactNode;
  description?: ReactNode;
}

export function RadioGroup<V extends string>({
  value,
  onValueChange,
  options,
  label,
  className,
}: {
  value: V;
  onValueChange: (v: V) => void;
  options: readonly RadioOption<V>[];
  label: string;
  className?: string;
}) {
  return (
    <BaseRadioGroup
      aria-label={label}
      value={value}
      onValueChange={(v: unknown) => {
        const picked = options.find((o) => o.value === v);
        if (picked) onValueChange(picked.value);
      }}
      className={cn('flex flex-col gap-2', className)}
    >
      {options.map((o) => (
        <label key={o.value} className="flex cursor-pointer items-start gap-3 text-sm">
          <Radio.Root
            value={o.value}
            className={cn(
              'mt-0.5 inline-flex size-5 shrink-0 items-center justify-center rounded-full border border-outline bg-surface',
              'data-checked:border-accent',
              focusRing,
            )}
          >
            <Radio.Indicator className="size-2.5 rounded-full bg-accent data-unchecked:hidden" />
          </Radio.Root>
          <span>
            <span className="block font-medium text-fg">{o.label}</span>
            {o.description ? (
              <span className="block text-xs text-muted">{o.description}</span>
            ) : null}
          </span>
        </label>
      ))}
    </BaseRadioGroup>
  );
}

/** Pill-shaped single-choice control (a radio group styled as segments). */
export function Segmented<V extends string>({
  value,
  onValueChange,
  options,
  label,
  className,
}: {
  value: V;
  onValueChange: (v: V) => void;
  options: readonly { value: V; label: ReactNode; icon?: ReactNode }[];
  label: string;
  className?: string;
}) {
  return (
    <BaseRadioGroup
      aria-label={label}
      value={value}
      onValueChange={(v: unknown) => {
        const picked = options.find((o) => o.value === v);
        if (picked) onValueChange(picked.value);
      }}
      className={cn('inline-flex flex-wrap gap-1 rounded-lg bg-surface-2 p-1', className)}
    >
      {options.map((o) => (
        <Radio.Root
          key={o.value}
          value={o.value}
          className={cn(
            'inline-flex h-ctl-sm items-center gap-1.5 rounded-md px-3 text-sm font-medium text-muted',
            'cursor-pointer transition-colors duration-(--scrin-dur-fast) hover:text-fg',
            'data-checked:bg-surface data-checked:text-fg data-checked:shadow-sm [&_svg]:size-4',
            focusRing,
          )}
        >
          {o.icon}
          {o.label}
        </Radio.Root>
      ))}
    </BaseRadioGroup>
  );
}

export function Slider({
  label,
  className,
  ...rest
}: ComponentProps<typeof BaseSlider.Root> & { label: string }) {
  return (
    <BaseSlider.Root className={cx('w-full', className)} {...rest}>
      <BaseSlider.Control className="flex h-6 w-full touch-none items-center select-none">
        <BaseSlider.Track className="h-1.5 w-full rounded-full bg-surface-2">
          <BaseSlider.Indicator className="rounded-full bg-accent" />
          <BaseSlider.Thumb
            aria-label={label}
            className={cn(
              'size-5 rounded-full border-2 border-accent bg-surface shadow transition-transform duration-(--scrin-dur-fast)',
              'data-dragging:scale-110',
              focusRing,
            )}
          />
        </BaseSlider.Track>
      </BaseSlider.Control>
    </BaseSlider.Root>
  );
}
