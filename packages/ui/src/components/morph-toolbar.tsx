import { Toolbar as BaseToolbar } from '@base-ui/react/toolbar';
import { m } from 'motion/react';
import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type ComponentProps,
  type KeyboardEvent,
  type PointerEvent,
  type ReactNode,
} from 'react';

import { cn } from '../lib/cn';
import { useMotionDuration } from '../lib/motion';
import { buttonVariants } from './button';

export type ToolbarEdge = 'top' | 'bottom' | 'left' | 'right';

export const TOOLBAR_EDGES: readonly ToolbarEdge[] = ['top', 'bottom', 'left', 'right'];

const EDGE_CLASS: Record<ToolbarEdge, string> = {
  top: 'inset-x-0 top-0 justify-center pt-[max(0.75rem,env(safe-area-inset-top))]',
  bottom: 'inset-x-0 bottom-0 justify-center pb-[max(0.75rem,env(safe-area-inset-bottom))]',
  left: 'inset-y-0 left-0 items-center pl-[max(0.75rem,env(safe-area-inset-left))]',
  right: 'inset-y-0 right-0 items-center pr-[max(0.75rem,env(safe-area-inset-right))]',
};

/** Where the hidden bar slides to: off its own edge. */
const HIDDEN_CLASS: Record<ToolbarEdge, string> = {
  top: '-translate-y-[calc(100%+1rem)]',
  bottom: 'translate-y-[calc(100%+1rem)]',
  left: '-translate-x-[calc(100%+1rem)]',
  right: 'translate-x-[calc(100%+1rem)]',
};

const KEY_EDGE: Partial<Record<string, ToolbarEdge>> = {
  ArrowUp: 'top',
  ArrowDown: 'bottom',
  ArrowLeft: 'left',
  ArrowRight: 'right',
};

/** Pointer distance from an edge (px) that brings an auto-hidden bar back. */
const REVEAL_PX = 48;
const HIDE_AFTER_MS = 2500;

/** The edge closest to a point inside a `width × height` box. */
export function nearestEdge(x: number, y: number, width: number, height: number): ToolbarEdge {
  const d: [ToolbarEdge, number][] = [
    ['top', y],
    ['bottom', height - y],
    ['left', x],
    ['right', width - x],
  ];
  d.sort((a, b) => a[1] - b[1]);
  return d[0]?.[0] ?? 'top';
}

export interface MorphToolbarProps {
  /** Accessible name of the toolbar. */
  label: string;
  expanded: boolean;
  onExpandedChange: (expanded: boolean) => void;
  expandLabel: string;
  collapseLabel: string;
  edge: ToolbarEdge;
  onEdgeChange: (edge: ToolbarEdge) => void;
  /** Accessible name of the drag grip; arrow keys on it move the bar. */
  moveLabel: string;
  autoHide?: boolean;
  /** Icons for the expand/collapse toggle. */
  expandIcon: ReactNode;
  collapseIcon: ReactNode;
  gripIcon: ReactNode;
  /** Shown in the collapsed pill next to the toggle (e.g. a live status). */
  collapsedContent?: ReactNode;
  children: ReactNode;
}

/**
 * The floating session toolbar. It morphs between a collapsed pill and the
 * full bar with a shared `layoutId` (Motion turns the size change into a
 * transform, so only compositor properties animate), sits on any screen edge
 * (drag the grip, or focus it and press an arrow key), and can hide itself
 * until the pointer comes near its edge or focus enters it.
 *
 * Tools inside are `ToolbarButton`s: one tab stop, arrow keys between them.
 */
export function MorphToolbar({
  label,
  expanded,
  onExpandedChange,
  expandLabel,
  collapseLabel,
  edge,
  onEdgeChange,
  moveLabel,
  autoHide = false,
  expandIcon,
  collapseIcon,
  gripIcon,
  collapsedContent,
  children,
}: MorphToolbarProps) {
  const d = useMotionDuration(0.32);
  const reduced = d === 0;
  const vertical = edge === 'left' || edge === 'right';
  const [hiddenState, setHidden] = useState(false);
  // Turning auto-hide off shows the bar at once, without an effect.
  const hidden = autoHide && hiddenState;
  const [drag, setDrag] = useState<{ x: number; y: number } | null>(null);
  const dragStart = useRef<{ x: number; y: number } | null>(null);
  const rootRef = useRef<HTMLDivElement>(null);
  const hover = useRef(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const schedule = useCallback(() => {
    if (timer.current) clearTimeout(timer.current);
    timer.current = null;
    if (!autoHide) return;
    timer.current = setTimeout(() => {
      const focusInside = rootRef.current?.contains(document.activeElement) ?? false;
      if (!hover.current && !focusInside) setHidden(true);
    }, HIDE_AFTER_MS);
  }, [autoHide]);

  const reveal = useCallback(() => {
    setHidden(false);
    schedule();
  }, [schedule]);

  useEffect(() => {
    if (!autoHide) return undefined;
    schedule();
    const onMove = (e: globalThis.PointerEvent) => {
      const near =
        (edge === 'top' && e.clientY < REVEAL_PX) ||
        (edge === 'bottom' && e.clientY > window.innerHeight - REVEAL_PX) ||
        (edge === 'left' && e.clientX < REVEAL_PX) ||
        (edge === 'right' && e.clientX > window.innerWidth - REVEAL_PX);
      if (near) reveal();
    };
    window.addEventListener('pointermove', onMove);
    return () => {
      window.removeEventListener('pointermove', onMove);
      if (timer.current) clearTimeout(timer.current);
    };
  }, [autoHide, edge, reveal, schedule]);

  const onGripKey = (e: KeyboardEvent<HTMLButtonElement>) => {
    const next = KEY_EDGE[e.key];
    if (!next) return;
    e.preventDefault();
    onEdgeChange(next);
  };

  const onGripDown = (e: PointerEvent<HTMLButtonElement>) => {
    if (e.button !== 0) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    dragStart.current = { x: e.clientX, y: e.clientY };
    setDrag({ x: 0, y: 0 });
  };
  const onGripMove = (e: PointerEvent<HTMLButtonElement>) => {
    const start = dragStart.current;
    if (start) setDrag({ x: e.clientX - start.x, y: e.clientY - start.y });
  };
  const onGripUp = (e: PointerEvent<HTMLButtonElement>) => {
    const start = dragStart.current;
    dragStart.current = null;
    setDrag(null);
    if (!start) return;
    // A click (no real movement) is not a move.
    if (Math.hypot(e.clientX - start.x, e.clientY - start.y) < 8) return;
    const parent = rootRef.current?.parentElement?.getBoundingClientRect();
    const w = parent?.width ?? window.innerWidth;
    const h = parent?.height ?? window.innerHeight;
    onEdgeChange(
      nearestEdge(e.clientX - (parent?.left ?? 0), e.clientY - (parent?.top ?? 0), w, h),
    );
  };

  const transition = reduced
    ? { duration: 0 }
    : ({ type: 'spring', bounce: 0.18, visualDuration: d } as const);

  return (
    <div
      ref={rootRef}
      data-edge={edge}
      data-hidden={hidden || undefined}
      data-reduced-motion={reduced || undefined}
      className={cn('pointer-events-none absolute z-40 flex', EDGE_CLASS[edge])}
      onPointerEnter={() => {
        hover.current = true;
        reveal();
      }}
      onPointerLeave={() => {
        hover.current = false;
        schedule();
      }}
      onFocus={reveal}
      onBlur={schedule}
    >
      <m.div
        layoutId="scrin-session-toolbar"
        layout={!reduced}
        transition={transition}
        className={cn(
          'pointer-events-auto flex max-h-full max-w-full items-center gap-0.5 rounded-2xl glass-panel p-1.5 shadow-2xl',
          vertical && 'flex-col',
          'transition-[opacity,translate] duration-(--scrin-dur) ease-scrin',
          hidden && !drag && cn('opacity-0', HIDDEN_CLASS[edge]),
        )}
        style={{
          borderRadius: 16,
          ...(drag ? { translate: `${drag.x}px ${drag.y}px`, transition: 'none' } : {}),
        }}
      >
        <button
          type="button"
          aria-label={moveLabel}
          title={moveLabel}
          aria-keyshortcuts="ArrowUp ArrowDown ArrowLeft ArrowRight"
          onKeyDown={onGripKey}
          onPointerDown={onGripDown}
          onPointerMove={onGripMove}
          onPointerUp={onGripUp}
          onPointerCancel={() => {
            dragStart.current = null;
            setDrag(null);
          }}
          className={cn(
            buttonVariants({ variant: 'ghost', size: 'icon-sm' }),
            'cursor-grab touch-none text-muted active:cursor-grabbing',
            vertical ? 'h-6 w-full' : 'h-full w-6',
          )}
        >
          {gripIcon}
        </button>
        <BaseToolbar.Root
          aria-label={label}
          orientation={vertical ? 'vertical' : 'horizontal'}
          className={cn(
            'flex items-center gap-0.5',
            vertical ? 'flex-col overflow-y-auto' : 'overflow-x-auto',
          )}
        >
          {expanded ? (
            <m.div
              key="tools"
              className={cn('flex items-center gap-0.5', vertical && 'flex-col')}
              initial={reduced ? false : { opacity: 0 }}
              animate={{ opacity: 1 }}
              transition={{ duration: d * 0.6 }}
            >
              {children}
            </m.div>
          ) : (
            collapsedContent
          )}
          <m.span layout={reduced ? false : 'position'} transition={transition}>
            <ToolbarButton
              label={expanded ? collapseLabel : expandLabel}
              aria-expanded={expanded}
              onClick={() => onExpandedChange(!expanded)}
            >
              {expanded ? collapseIcon : expandIcon}
            </ToolbarButton>
          </m.span>
        </BaseToolbar.Root>
      </m.div>
    </div>
  );
}

export interface ToolbarButtonProps extends Omit<
  ComponentProps<typeof BaseToolbar.Button>,
  'aria-label' | 'className'
> {
  label: string;
  pressed?: boolean;
  tone?: 'ghost' | 'danger';
  className?: string;
}

/**
 * An icon-only tool. Works as a Menu/Popover trigger too: pass it as the
 * `trigger` element and Base UI merges the trigger props in.
 */
export function ToolbarButton({
  label,
  pressed,
  tone = 'ghost',
  className,
  children,
  ...rest
}: ToolbarButtonProps) {
  return (
    <BaseToolbar.Button
      aria-label={label}
      title={label}
      {...(pressed !== undefined ? { 'aria-pressed': pressed } : {})}
      className={cn(
        buttonVariants({ variant: tone, size: 'icon-sm' }),
        pressed && 'bg-accent/15 text-accent',
        className,
      )}
      {...rest}
    >
      {children}
    </BaseToolbar.Button>
  );
}

export function ToolbarSeparator({ vertical }: { vertical?: boolean }) {
  return (
    <BaseToolbar.Separator
      orientation={vertical ? 'horizontal' : 'vertical'}
      className={cn('bg-outline', vertical ? 'my-1 h-px w-6' : 'mx-1 h-6 w-px')}
    />
  );
}
