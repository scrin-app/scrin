import { Button as BaseButton } from '@base-ui/react/button';
import { cva, type VariantProps } from 'class-variance-authority';
import type { ComponentProps, ReactNode } from 'react';

import { cx } from '../lib/cn';
import { Spinner } from './spinner';

export const buttonVariants = cva(
  [
    'relative inline-flex shrink-0 cursor-pointer items-center justify-center gap-2 whitespace-nowrap select-none',
    'rounded-md font-medium',
    'transition-[background-color,color,box-shadow,transform,opacity] duration-(--scrin-dur-fast) ease-scrin',
    'focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent',
    'active:scale-[0.97] disabled:pointer-events-none disabled:opacity-50 data-disabled:pointer-events-none data-disabled:opacity-50',
    '[&_svg]:pointer-events-none [&_svg]:shrink-0',
  ],
  {
    variants: {
      variant: {
        primary: 'bg-accent text-accent-fg shadow-sm hover:brightness-110',
        secondary: 'bg-surface-2 text-fg hover:bg-outline/60',
        outline: 'border border-outline bg-transparent text-fg hover:bg-surface-2',
        ghost: 'bg-transparent text-fg hover:bg-surface-2',
        danger: 'bg-danger text-accent-fg shadow-sm hover:brightness-110',
        link: 'h-auto px-0 text-accent underline-offset-4 hover:underline',
      },
      size: {
        sm: 'h-ctl-sm px-3 text-sm [&_svg]:size-4',
        md: 'h-ctl px-4 text-sm [&_svg]:size-4',
        lg: 'h-ctl-lg px-6 text-base [&_svg]:size-5',
        icon: 'size-ctl [&_svg]:size-5',
        'icon-sm': 'size-ctl-sm [&_svg]:size-4',
      },
    },
    defaultVariants: { variant: 'primary', size: 'md' },
  },
);

export interface ButtonProps
  extends ComponentProps<typeof BaseButton>, VariantProps<typeof buttonVariants> {
  loading?: boolean;
  /** Optional leading icon, replaced by the spinner while loading. */
  icon?: ReactNode;
}

export function Button({
  className,
  variant,
  size,
  loading = false,
  disabled,
  icon,
  children,
  ...rest
}: ButtonProps) {
  return (
    <BaseButton
      className={cx(buttonVariants({ variant, size }), className)}
      disabled={disabled === true || loading}
      focusableWhenDisabled={loading}
      aria-busy={loading || undefined}
      {...rest}
    >
      {loading ? <Spinner size="sm" label={false} /> : icon}
      {children}
    </BaseButton>
  );
}

export interface IconButtonProps extends Omit<ButtonProps, 'size' | 'icon' | 'aria-label'> {
  /** Required: an icon-only button needs an accessible name. */
  label: string;
  size?: 'icon' | 'icon-sm';
}

export function IconButton({
  label,
  size = 'icon',
  variant = 'ghost',
  children,
  ...rest
}: IconButtonProps) {
  return (
    <Button aria-label={label} title={label} size={size} variant={variant} {...rest}>
      {children}
    </Button>
  );
}
