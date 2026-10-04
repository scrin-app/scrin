import { Field as BaseField } from '@base-ui/react/field';
import { Input as BaseInput } from '@base-ui/react/input';
import type { ComponentProps, ReactNode } from 'react';

import { cn, cx } from '../lib/cn';

export const inputClass = cn(
  'h-ctl w-full min-w-0 rounded-md border border-outline bg-surface px-3 text-sm text-fg',
  'placeholder:text-muted/80',
  'transition-[border-color,box-shadow] duration-(--scrin-dur-fast)',
  'focus-visible:border-accent focus-visible:ring-3 focus-visible:ring-accent/35 focus-visible:outline-none',
  'data-invalid:border-danger data-invalid:ring-danger/30',
  'disabled:cursor-not-allowed disabled:opacity-60',
);

export function Input({ className, ...rest }: ComponentProps<typeof BaseInput>) {
  return <BaseInput className={cx(inputClass, className)} {...rest} />;
}

export interface FieldProps extends Omit<ComponentProps<typeof BaseField.Root>, 'children'> {
  label: ReactNode;
  description?: ReactNode;
  error?: ReactNode;
  children: ReactNode;
}

/**
 * Label + control + description + error, wired with Base UI so the label,
 * `aria-describedby` and `aria-invalid` are connected automatically. Put an
 * `<Input>` (or `<Field.Control>`) inside.
 */
export function Field({
  label,
  description,
  error,
  children,
  className,
  invalid,
  ...rest
}: FieldProps) {
  const isInvalid = invalid === true || (error !== undefined && error !== null && error !== false);
  return (
    <BaseField.Root
      className={cx('flex min-w-0 flex-col gap-1.5', className)}
      invalid={isInvalid}
      {...rest}
    >
      <Label>{label}</Label>
      {children}
      {description ? (
        <BaseField.Description className="text-xs text-muted">{description}</BaseField.Description>
      ) : null}
      {isInvalid && error ? (
        <BaseField.Error match className="text-xs font-medium text-danger">
          {error}
        </BaseField.Error>
      ) : null}
    </BaseField.Root>
  );
}

export function Label({ className, ...rest }: ComponentProps<typeof BaseField.Label>) {
  return <BaseField.Label className={cx('text-sm font-medium text-fg', className)} {...rest} />;
}
