import { Popover as BasePopover } from '@base-ui/react/popover';
import type { ReactElement, ReactNode } from 'react';

import { cn } from '../lib/cn';

export const floatingClass = cn(
  'z-50 rounded-lg glass-panel p-1 text-sm text-fg shadow-xl outline-none',
  'origin-(--transform-origin) transition-[opacity,transform] duration-(--scrin-dur-fast) ease-scrin',
  'data-starting-style:scale-95 data-starting-style:opacity-0',
  'data-ending-style:scale-95 data-ending-style:opacity-0',
);

export interface PopoverProps {
  trigger: ReactElement;
  title?: ReactNode;
  children: ReactNode;
  side?: 'top' | 'bottom' | 'left' | 'right';
  align?: 'start' | 'center' | 'end';
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  className?: string;
}

export function Popover({
  trigger,
  title,
  children,
  side = 'bottom',
  align = 'center',
  open,
  onOpenChange,
  className,
}: PopoverProps) {
  return (
    <BasePopover.Root
      {...(open !== undefined ? { open } : {})}
      {...(onOpenChange ? { onOpenChange: (o: boolean) => onOpenChange(o) } : {})}
    >
      <BasePopover.Trigger render={trigger} />
      <BasePopover.Portal>
        <BasePopover.Positioner side={side} align={align} sideOffset={8} collisionPadding={8}>
          <BasePopover.Popup className={cn(floatingClass, 'w-72 p-4', className)}>
            {title ? (
              <BasePopover.Title className="mb-2 text-sm font-semibold">{title}</BasePopover.Title>
            ) : null}
            {children}
          </BasePopover.Popup>
        </BasePopover.Positioner>
      </BasePopover.Portal>
    </BasePopover.Root>
  );
}
