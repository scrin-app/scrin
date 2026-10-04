import { Tooltip as BaseTooltip } from '@base-ui/react/tooltip';
import type { ReactElement, ReactNode } from 'react';

import { cn } from '../lib/cn';

export function TooltipProvider({ children }: { children: ReactNode }) {
  return <BaseTooltip.Provider delay={400}>{children}</BaseTooltip.Provider>;
}

export interface TooltipProps {
  content: ReactNode;
  children: ReactElement;
  side?: 'top' | 'bottom' | 'left' | 'right';
}

/**
 * Supplementary hint only — the trigger must already have an accessible name
 * (tooltips are not announced reliably on touch or by every screen reader).
 */
export function Tooltip({ content, children, side = 'top' }: TooltipProps) {
  return (
    <BaseTooltip.Root>
      <BaseTooltip.Trigger render={children} />
      <BaseTooltip.Portal>
        <BaseTooltip.Positioner side={side} sideOffset={8}>
          <BaseTooltip.Popup
            className={cn(
              'z-50 rounded-md bg-fg px-2 py-1 text-xs font-medium text-bg shadow-lg',
              'origin-(--transform-origin) transition-[opacity,transform] duration-(--scrin-dur-fast)',
              'data-ending-style:opacity-0 data-starting-style:scale-95 data-starting-style:opacity-0',
            )}
          >
            {content}
          </BaseTooltip.Popup>
        </BaseTooltip.Positioner>
      </BaseTooltip.Portal>
    </BaseTooltip.Root>
  );
}
