import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { Copy, Inbox } from 'lucide-react';
import { useState, type ReactNode } from 'react';
import { describe, expect, it, vi } from 'vitest';

import { axeViolations } from '../test/a11y';
import { ThemeProvider } from '../theme/ThemeProvider';
import { DEFAULT_THEME } from '../theme/tokens';
import { Button, IconButton } from './button';
import { CodeDisplay } from './code-display';
import { Checkbox, RadioGroup, Segmented, Slider, Switch, SwitchRow } from './controls';
import { AlertDialog, Dialog } from './dialog';
import { Badge, Card, CardHeader, EmptyState, Kbd, Progress, Skeleton, StatusDot } from './display';
import { Field, Input } from './field';
import { Menu, MenuItem } from './menu';
import { Popover } from './popover';
import { SasEmoji } from './sas-emoji';
import { Select } from './select';
import { Spinner } from './spinner';
import { Tab, Tabs, TabsList, TabsPanel } from './tabs';
import { Tooltip, TooltipProvider } from './tooltip';

function Themed({ mode, children }: { mode: 'light' | 'dark'; children: ReactNode }) {
  return (
    <ThemeProvider initial={{ ...DEFAULT_THEME, mode }}>
      <TooltipProvider>{children}</TooltipProvider>
    </ThemeProvider>
  );
}

function SelectDemo() {
  const [v, setV] = useState<'a' | 'b'>('a');
  return (
    <Select
      label="Codec"
      value={v}
      onValueChange={setV}
      options={[
        { value: 'a', label: 'AV1' },
        { value: 'b', label: 'H.264' },
      ]}
    />
  );
}

/** Every component in a resting state, for the axe sweep. */
const GALLERY: Record<string, () => ReactNode> = {
  button: () => (
    <>
      <Button>Save</Button>
      <Button variant="outline" loading>
        Saving
      </Button>
      <IconButton label="Copy">
        <Copy aria-hidden />
      </IconButton>
    </>
  ),
  field: () => (
    <Field label="Partner ID" description="Nine digits" error="Too short" invalid>
      <Input />
    </Field>
  ),
  switch: () => (
    <>
      <Switch aria-label="Direct connections" />
      <SwitchRow
        label="Audio"
        description="Play remote audio"
        checked
        onCheckedChange={() => undefined}
      />
    </>
  ),
  checkbox: () => <Checkbox aria-label="Trust" />,
  radio: () => (
    <RadioGroup
      label="Density"
      value="a"
      onValueChange={() => undefined}
      options={[
        { value: 'a', label: 'Compact' },
        { value: 'b', label: 'Spacious' },
      ]}
    />
  ),
  segmented: () => (
    <Segmented
      label="Theme"
      value="light"
      onValueChange={() => undefined}
      options={[
        { value: 'light', label: 'Light' },
        { value: 'dark', label: 'Dark' },
      ]}
    />
  ),
  slider: () => <Slider label="Volume" defaultValue={40} />,
  select: () => <SelectDemo />,
  tabs: () => (
    <Tabs defaultValue="a">
      <TabsList>
        <Tab value="a">One</Tab>
        <Tab value="b">Two</Tab>
      </TabsList>
      <TabsPanel value="a">First</TabsPanel>
      <TabsPanel value="b">Second</TabsPanel>
    </Tabs>
  ),
  display: () => (
    <Card>
      <CardHeader title="Your device" description="Read these aloud" icon={<Inbox aria-hidden />} />
      <Badge tone="success">Online</Badge>
      <StatusDot online label="Online" />
      <Kbd>Ctrl</Kbd>
      <Skeleton className="h-4 w-20" />
      <Progress value={40} label="Upload" />
      <Spinner />
    </Card>
  ),
  empty: () => (
    <EmptyState icon={<Inbox aria-hidden />} title="No devices" description="Nothing yet" />
  ),
  code: () => (
    <CodeDisplay
      value="123 456 789"
      label="Your ID"
      onCopy={() => undefined}
      countdown={{ issuedAt: Date.now(), expiresAt: Date.now() + 600_000 }}
    />
  ),
  sas: () => <SasEmoji indices={[0, 12, 31, 47, 63]} />,
  tooltip: () => (
    <Tooltip content="Copies the ID">
      <Button>Copy</Button>
    </Tooltip>
  ),
  popover: () => (
    <Popover trigger={<Button>Open</Button>} title="Info">
      Body
    </Popover>
  ),
  menu: () => (
    <Menu trigger={<Button>Keys</Button>}>
      <MenuItem>Ctrl+Alt+Del</MenuItem>
    </Menu>
  ),
  dialog: () => <Dialog trigger={<Button>Open</Button>} title="Title" description="Body" />,
};

describe('axe: no violations', () => {
  for (const mode of ['light', 'dark'] as const) {
    for (const [name, Render] of Object.entries(GALLERY)) {
      it(`${name} (${mode})`, async () => {
        const { container } = render(
          <Themed mode={mode}>
            <main>
              <Render />
            </main>
          </Themed>,
        );
        expect(await axeViolations(container)).toEqual([]);
      });
    }
  }

  it('an open dialog is accessible', async () => {
    render(
      <Themed mode="dark">
        <Dialog defaultOpen title="End session?" description="The device will be disconnected." />
      </Themed>,
    );
    const dialog = await screen.findByRole('dialog');
    expect(await axeViolations(dialog)).toEqual([]);
  });
});

describe('Button', () => {
  it('shows a spinner and blocks clicks while loading', async () => {
    const onClick = vi.fn();
    render(
      <Themed mode="light">
        <Button loading onClick={onClick}>
          Connect
        </Button>
      </Themed>,
    );
    const btn = screen.getByRole('button', { name: /connect/i });
    expect(btn.getAttribute('aria-busy')).toBe('true');
    await userEvent.click(btn);
    expect(onClick).not.toHaveBeenCalled();
  });

  it('IconButton exposes its label as the accessible name', () => {
    render(
      <IconButton label="Copy ID">
        <Copy aria-hidden />
      </IconButton>,
    );
    expect(screen.getByRole('button', { name: 'Copy ID' })).toBeTruthy();
  });
});

describe('keyboard', () => {
  it('Dialog opens with Enter, traps focus and closes with Escape', async () => {
    const user = userEvent.setup();
    render(
      <Themed mode="light">
        <Dialog trigger={<Button>Open dialog</Button>} title="Session" description="Details" />
      </Themed>,
    );
    screen.getByRole('button', { name: 'Open dialog' }).focus();
    await user.keyboard('{Enter}');
    const dialog = await screen.findByRole('dialog');
    await waitFor(() => expect(dialog.contains(document.activeElement)).toBe(true));
    await user.keyboard('{Escape}');
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
  });

  it('Menu opens from the keyboard and moves with arrows', async () => {
    const user = userEvent.setup();
    const onCad = vi.fn();
    render(
      <Themed mode="light">
        <Menu trigger={<Button>Keys</Button>}>
          <MenuItem onClick={onCad}>Ctrl+Alt+Del</MenuItem>
          <MenuItem>Alt+Tab</MenuItem>
        </Menu>
      </Themed>,
    );
    screen.getByRole('button', { name: 'Keys' }).focus();
    await user.keyboard('{ArrowDown}');
    const menu = await screen.findByRole('menu');
    expect(menu).toBeTruthy();
    const items = screen.getAllByRole('menuitem');
    await waitFor(() => expect(document.activeElement).toBe(items[0]));
    await user.keyboard('{ArrowDown}');
    await waitFor(() => expect(document.activeElement).toBe(items[1]));
    await user.keyboard('{ArrowUp}{Enter}');
    await waitFor(() => expect(onCad).toHaveBeenCalledTimes(1));
  });

  it('Tabs move selection with arrow keys', async () => {
    const user = userEvent.setup();
    render(
      <Tabs defaultValue="a">
        <TabsList>
          <Tab value="a">General</Tab>
          <Tab value="b">Video</Tab>
        </TabsList>
        <TabsPanel value="a">General panel</TabsPanel>
        <TabsPanel value="b">Video panel</TabsPanel>
      </Tabs>,
    );
    const [first, second] = screen.getAllByRole('tab');
    first!.focus();
    await user.keyboard('{ArrowRight}');
    expect(document.activeElement).toBe(second);
    await user.keyboard('{Enter}');
    await waitFor(() => expect(second!.getAttribute('aria-selected')).toBe('true'));
    expect(screen.getByText('Video panel')).toBeTruthy();
  });

  it('Switch toggles with Space', async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<Switch aria-label="Audio" onCheckedChange={onChange} />);
    screen.getByRole('switch').focus();
    await user.keyboard(' ');
    expect(onChange).toHaveBeenCalledWith(true, expect.anything());
  });

  it('AlertDialog confirm runs the action', async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn();
    render(
      <Themed mode="light">
        <AlertDialog
          trigger={<Button>End</Button>}
          title="End session?"
          description="Disconnects the device"
          confirmLabel="End session"
          destructive
          onConfirm={onConfirm}
        />
      </Themed>,
    );
    await user.click(screen.getByRole('button', { name: 'End' }));
    const dialog = await screen.findByRole('alertdialog');
    expect(dialog).toBeTruthy();
    await user.click(screen.getByRole('button', { name: 'End session' }));
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });
});

describe('CodeDisplay', () => {
  it('copies the raw value without spaces', async () => {
    const onCopy = vi.fn();
    render(<CodeDisplay value="123 456 789" label="Your ID" onCopy={onCopy} />);
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: /copy/i }));
      await Promise.resolve();
    });
    expect(onCopy).toHaveBeenCalledWith('123456789');
  });
});

describe('SasEmoji', () => {
  it('names every emoji for screen readers', () => {
    render(<SasEmoji indices={[0, 1, 2, 3, 63]} />);
    const list = screen.getByRole('list');
    expect(list.getAttribute('aria-label')).toBe('Verification emoji: dog, cat, lion, horse, pin');
    expect(screen.getAllByRole('listitem')).toHaveLength(5);
  });
});
