import { Tabs as BaseTabs } from '@base-ui/react/tabs';
import type { ComponentProps } from 'react';

import { cn, cx } from '../lib/cn';

export function Tabs(props: ComponentProps<typeof BaseTabs.Root>) {
  return <BaseTabs.Root {...props} />;
}

export function TabsList({ className, children, ...rest }: ComponentProps<typeof BaseTabs.List>) {
  return (
    <BaseTabs.List
      className={cx(
        'relative inline-flex items-center gap-1 rounded-lg bg-surface-2 p-1',
        className,
      )}
      {...rest}
    >
      {children}
      <BaseTabs.Indicator
        className={cn(
          'absolute top-1/2 left-0 -z-0 h-(--active-tab-height) w-(--active-tab-width) -translate-y-1/2',
          'translate-x-(--active-tab-left) rounded-md bg-surface shadow-sm',
          'transition-[translate,width] duration-(--scrin-dur) ease-scrin',
        )}
      />
    </BaseTabs.List>
  );
}

export function Tab({ className, ...rest }: ComponentProps<typeof BaseTabs.Tab>) {
  return (
    <BaseTabs.Tab
      className={cx(
        cn(
          'relative z-10 inline-flex h-ctl-sm items-center gap-2 rounded-md px-3 text-sm font-medium text-muted',
          'transition-colors duration-(--scrin-dur-fast) hover:text-fg data-active:text-fg',
          'focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent [&_svg]:size-4',
        ),
        className,
      )}
      {...rest}
    />
  );
}

export function TabsPanel({ className, ...rest }: ComponentProps<typeof BaseTabs.Panel>) {
  return (
    <BaseTabs.Panel
      className={cx(
        'mt-4 outline-none focus-visible:outline-2 focus-visible:outline-accent',
        className,
      )}
      {...rest}
    />
  );
}
