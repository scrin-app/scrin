import { Menu as BaseMenu } from '@base-ui/react/menu';
import { Check } from 'lucide-react';
import type { ComponentProps, ReactElement, ReactNode } from 'react';

import { cn, cx } from '../lib/cn';
import { floatingClass } from './popover';

const itemClass = cn(
  'flex min-h-9 cursor-default items-center gap-2 rounded-md px-2.5 text-sm outline-none select-none',
  'data-disabled:opacity-50 data-highlighted:bg-surface-2 [&_svg]:size-4 [&_svg]:text-muted',
);

export interface MenuProps {
  trigger: ReactElement;
  children: ReactNode;
  side?: 'top' | 'bottom' | 'left' | 'right';
  align?: 'start' | 'center' | 'end';
  className?: string;
}

export function Menu({
  trigger,
  children,
  side = 'bottom',
  align = 'start',
  className,
}: MenuProps) {
  return (
    <BaseMenu.Root>
      <BaseMenu.Trigger render={trigger} />
      <BaseMenu.Portal>
        <BaseMenu.Positioner side={side} align={align} sideOffset={6} collisionPadding={8}>
          <BaseMenu.Popup className={cn(floatingClass, 'min-w-48', className)}>
            {children}
          </BaseMenu.Popup>
        </BaseMenu.Positioner>
      </BaseMenu.Portal>
    </BaseMenu.Root>
  );
}

export function MenuItem({ className, ...rest }: ComponentProps<typeof BaseMenu.Item>) {
  return <BaseMenu.Item className={cx(itemClass, className)} {...rest} />;
}

export function MenuSeparator() {
  return <BaseMenu.Separator className="my-1 h-px bg-outline" />;
}

export function MenuGroup({ label, children }: { label: ReactNode; children: ReactNode }) {
  return (
    <BaseMenu.Group>
      <BaseMenu.GroupLabel className="px-2.5 pt-2 pb-1 text-xs font-medium text-muted">
        {label}
      </BaseMenu.GroupLabel>
      {children}
    </BaseMenu.Group>
  );
}

export function MenuRadioGroup<V extends string>({
  value,
  onValueChange,
  children,
}: {
  value: V;
  onValueChange: (v: V) => void;
  children: ReactNode;
}) {
  return (
    <BaseMenu.RadioGroup value={value} onValueChange={onValueChange}>
      {children}
    </BaseMenu.RadioGroup>
  );
}

export function MenuRadioItem({
  className,
  children,
  ...rest
}: ComponentProps<typeof BaseMenu.RadioItem>) {
  return (
    <BaseMenu.RadioItem className={cx(cn(itemClass, 'pr-8'), className)} {...rest}>
      <span className="flex-1">{children}</span>
      <BaseMenu.RadioItemIndicator className="absolute right-2.5">
        <Check aria-hidden className="text-accent!" />
      </BaseMenu.RadioItemIndicator>
    </BaseMenu.RadioItem>
  );
}
