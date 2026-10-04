import { act, fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MotionGlobalConfig } from 'motion/react';
import { useState } from 'react';
import { beforeAll, describe, expect, it, vi } from 'vitest';

import { MotionProvider } from '../lib/motion';
import { axeViolations } from '../test/a11y';
import { ThemeProvider } from '../theme/ThemeProvider';
import { DEFAULT_THEME, type MotionLevel } from '../theme/tokens';
import { MorphToolbar, nearestEdge, ToolbarButton, type ToolbarEdge } from './morph-toolbar';

beforeAll(() => {
  MotionGlobalConfig.skipAnimations = true;
});

function Harness({
  motion = 'full',
  onA = () => undefined,
  initialEdge = 'top',
  autoHide = false,
}: {
  motion?: MotionLevel;
  onA?: () => void;
  initialEdge?: ToolbarEdge;
  autoHide?: boolean;
}) {
  const [expanded, setExpanded] = useState(true);
  const [edge, setEdge] = useState<ToolbarEdge>(initialEdge);
  return (
    <ThemeProvider initial={{ ...DEFAULT_THEME, motion }}>
      <MotionProvider>
        <main>
          <MorphToolbar
            label="Session toolbar"
            expanded={expanded}
            onExpandedChange={setExpanded}
            expandLabel="More tools"
            collapseLabel="Collapse toolbar"
            edge={edge}
            onEdgeChange={setEdge}
            moveLabel="Move the toolbar"
            autoHide={autoHide}
            expandIcon={<span>+</span>}
            collapseIcon={<span>-</span>}
            gripIcon={<span>::</span>}
          >
            <ToolbarButton label="Quality" onClick={onA}>
              <span aria-hidden>Q</span>
            </ToolbarButton>
            <ToolbarButton label="Keys">
              <span aria-hidden>K</span>
            </ToolbarButton>
            <ToolbarButton label="End session" tone="danger">
              <span aria-hidden>E</span>
            </ToolbarButton>
          </MorphToolbar>
        </main>
      </MotionProvider>
    </ThemeProvider>
  );
}

const edgeOf = () => document.querySelector('[data-edge]')?.getAttribute('data-edge');
const isHidden = () => document.querySelector('[data-edge]')?.hasAttribute('data-hidden');

describe('MorphToolbar', () => {
  it('is one tab stop with arrow-key navigation between tools', async () => {
    const user = userEvent.setup();
    const onA = vi.fn();
    render(<Harness onA={onA} />);
    const toolbar = screen.getByRole('toolbar', { name: 'Session toolbar' });
    expect(toolbar.getAttribute('aria-orientation')).toBe('horizontal');
    screen.getByRole('button', { name: 'Quality' }).focus();
    await user.keyboard('{ArrowRight}');
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Keys' }));
    await user.keyboard('{ArrowRight}');
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'End session' }));
    await user.keyboard('{ArrowLeft}{ArrowLeft}{Enter}');
    expect(onA).toHaveBeenCalledTimes(1);
  });

  it('collapses to a pill and expands again from the keyboard', async () => {
    const user = userEvent.setup();
    render(<Harness />);
    const collapse = screen.getByRole('button', { name: 'Collapse toolbar' });
    expect(collapse.getAttribute('aria-expanded')).toBe('true');
    collapse.focus();
    await user.keyboard('{Enter}');
    expect(screen.queryByRole('button', { name: 'Quality' })).toBeNull();
    const expand = screen.getByRole('button', { name: 'More tools' });
    expect(expand.getAttribute('aria-expanded')).toBe('false');
    expand.focus();
    await user.keyboard(' ');
    expect(screen.getByRole('button', { name: 'Quality' })).toBeTruthy();
  });

  it('moves to another edge with arrow keys on the grip and turns vertical on the sides', async () => {
    const user = userEvent.setup();
    render(<Harness />);
    expect(edgeOf()).toBe('top');
    screen.getByRole('button', { name: 'Move the toolbar' }).focus();
    await user.keyboard('{ArrowDown}');
    expect(edgeOf()).toBe('bottom');
    await user.keyboard('{ArrowLeft}');
    expect(edgeOf()).toBe('left');
    expect(screen.getByRole('toolbar').getAttribute('aria-orientation')).toBe('vertical');
  });

  it('snaps to the nearest edge after a pointer drag, but a click is not a move', () => {
    render(<Harness />);
    const grip = screen.getByRole('button', { name: 'Move the toolbar' });
    grip.setPointerCapture = () => undefined;
    fireEvent.pointerDown(grip, { button: 0, pointerId: 1, clientX: 500, clientY: 10 });
    fireEvent.pointerUp(grip, { button: 0, pointerId: 1, clientX: 502, clientY: 11 });
    expect(edgeOf()).toBe('top');
    fireEvent.pointerDown(grip, { button: 0, pointerId: 1, clientX: 500, clientY: 10 });
    fireEvent.pointerMove(grip, { pointerId: 1, clientX: 600, clientY: 400 });
    fireEvent.pointerUp(grip, {
      button: 0,
      pointerId: 1,
      clientX: window.innerWidth - 5,
      clientY: 400,
    });
    expect(edgeOf()).toBe('right');
  });

  it('honours reduced motion: no layout animation, flagged on the root', () => {
    render(<Harness motion="off" />);
    expect(document.querySelector('[data-reduced-motion]')).not.toBeNull();
  });

  it('animates when motion is on', () => {
    render(<Harness motion="full" />);
    expect(document.querySelector('[data-reduced-motion]')).toBeNull();
  });

  it('auto-hides after idle time and comes back when focus enters', () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
    try {
      render(<Harness autoHide />);
      expect(isHidden()).toBe(false);
      act(() => {
        vi.advanceTimersByTime(3_000);
      });
      expect(isHidden()).toBe(true);
      act(() => {
        screen.getByRole('button', { name: 'Quality' }).focus();
      });
      expect(isHidden()).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });

  it('has no axe violations', async () => {
    const { container } = render(<Harness />);
    expect(await axeViolations(container)).toEqual([]);
  });
});

describe('nearestEdge', () => {
  it('picks the closest side', () => {
    expect(nearestEdge(500, 5, 1000, 800)).toBe('top');
    expect(nearestEdge(500, 790, 1000, 800)).toBe('bottom');
    expect(nearestEdge(4, 400, 1000, 800)).toBe('left');
    expect(nearestEdge(995, 400, 1000, 800)).toBe('right');
  });
});
