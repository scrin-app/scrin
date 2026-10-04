import { act, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { axeViolations } from '../test/a11y';
import {
  dismissIncomingRequest,
  hostRoleActionsFromInvoke,
  parseIncomingRequest,
  pushHostEvent,
  useIncomingRequest,
  type IncomingRequest,
} from './host-role';
import {
  ANONYMOUS_FORBIDDEN,
  defaultAllowed,
  PERMISSIONS,
  type PermissionName,
} from './permissions';
import { RequestDialog } from './request-dialog';

const T0 = 1_000_000;

function request(over: Partial<IncomingRequest> = {}): IncomingRequest {
  return {
    session: 's1',
    peer: 'a'.repeat(64),
    fingerprint: 'AB12-CD34',
    kind: 'anonymous',
    sas: [0, 1, 2, 3, 4],
    requested: [...PERMISSIONS],
    allowed: defaultAllowed('anonymous'),
    acceptEnabledAt: T0 + 5_000,
    expiresAt: T0 + 60_000,
    ...over,
  };
}

function setup(req: IncomingRequest, start = T0) {
  let now = start;
  const clock = () => now;
  const onAccept = vi.fn<(granted: PermissionName[]) => void>();
  const onDeny = vi.fn();
  render(<RequestDialog request={req} onAccept={onAccept} onDeny={onDeny} now={clock} />);
  const advance = (ms: number) => {
    now += ms;
    act(() => {
      vi.advanceTimersByTime(ms);
    });
  };
  return { onAccept, onDeny, advance };
}

afterEach(() => {
  vi.useRealTimers();
});

function Probe() {
  const r = useIncomingRequest();
  return <p data-testid="probe">{r?.session ?? 'none'}</p>;
}

const incoming = (session: string) => ({ type: 'incomingRequest', session, kind: 'trusted' });

describe('RequestDialog (anti-scam, ADR-0009)', () => {
  it('shows the scam warning, the unverified badge and the anonymous caps', async () => {
    render(
      <RequestDialog request={request()} onAccept={vi.fn()} onDeny={vi.fn()} now={() => T0} />,
    );
    const dialog = await screen.findByRole('alertdialog');
    expect(dialog.textContent).toContain('Nobody legitimate calls you asking for access');
    expect(dialog.textContent).toContain('Anonymous · not verified');
    expect(dialog.textContent).toContain('Ends after 60 minutes');
    expect(dialog.textContent).toContain('No file transfer');
    expect(screen.getByRole('list', { name: /Verification emoji/ })).toBeTruthy();
  });

  it('greys out every permission an anonymous controller may never hold', async () => {
    render(
      <RequestDialog request={request()} onAccept={vi.fn()} onDeny={vi.fn()} now={() => T0} />,
    );
    await screen.findByRole('alertdialog');
    for (const p of PERMISSIONS) {
      const row = document.querySelector(`[data-permission="${p}"]`);
      const sw = row?.querySelector('[role="switch"]');
      expect(sw, p).toBeTruthy();
      const disabled = sw?.hasAttribute('data-disabled') ?? false;
      const forbidden = ANONYMOUS_FORBIDDEN.has(p) || p === 'privacy_mode';
      expect(disabled, `${p} disabled`).toBe(forbidden);
      if (forbidden) expect(sw?.getAttribute('aria-checked'), `${p} off`).toBe('false');
    }
    // Even if the engine's ceiling wrongly allowed files, the UI still caps them.
    render(
      <RequestDialog
        request={request({ session: 's2', allowed: [...PERMISSIONS] })}
        onAccept={vi.fn()}
        onDeny={vi.fn()}
        now={() => T0}
      />,
    );
    const files = document.querySelectorAll('[data-permission="files_in"] [role="switch"]');
    for (const sw of files) expect(sw.hasAttribute('data-disabled')).toBe(true);
  });

  it('does not show caps for a trusted device and lets privacy mode through', async () => {
    render(
      <RequestDialog
        request={request({ kind: 'trusted', allowed: defaultAllowed('trusted'), sas: null })}
        onAccept={vi.fn()}
        onDeny={vi.fn()}
        now={() => T0}
      />,
    );
    const dialog = await screen.findByRole('alertdialog');
    expect(dialog.textContent).not.toContain('Limits for anonymous sessions');
    const sw = document.querySelector('[data-permission="privacy_mode"] [role="switch"]');
    expect(sw?.hasAttribute('data-disabled')).toBe(false);
  });

  it('keeps Accept disabled during the anti-scam delay, then sends only allowed grants', async () => {
    vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'setTimeout', 'clearTimeout'] });
    const { onAccept, advance } = setup(request());
    const accept = screen.getByRole('button', { name: /^Accept/ });
    expect(accept.textContent).toBe('Accept in 5 s');
    expect(accept.getAttribute('aria-disabled') ?? accept.getAttribute('disabled')).not.toBeNull();
    accept.click();
    expect(onAccept).not.toHaveBeenCalled();

    advance(5_000);
    expect(accept.textContent).toBe('Accept');
    act(() => {
      accept.click();
    });
    expect(onAccept).toHaveBeenCalledTimes(1);
    const granted = onAccept.mock.calls[0]?.[0] ?? [];
    expect(granted).toContain('view');
    for (const p of granted) expect(ANONYMOUS_FORBIDDEN.has(p)).toBe(false);
  });

  it('denies automatically when the countdown runs out', () => {
    vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'setTimeout', 'clearTimeout'] });
    const { onDeny, onAccept, advance } = setup(request({ expiresAt: T0 + 3_000 }));
    expect(screen.getByTestId('auto-deny').textContent).toContain('Denied automatically in 3 s');
    advance(1_000);
    expect(screen.getByTestId('auto-deny').textContent).toContain('2 s');
    expect(onDeny).not.toHaveBeenCalled();
    advance(2_500);
    expect(onDeny).toHaveBeenCalledTimes(1);
    expect(onDeny).toHaveBeenCalledWith('expired');
    expect(onAccept).not.toHaveBeenCalled();
    advance(5_000);
    expect(onDeny).toHaveBeenCalledTimes(1);
  });

  it('starts focus on Deny and Escape does not dismiss it', async () => {
    const onDeny = vi.fn();
    const user = userEvent.setup();
    render(<RequestDialog request={request()} onAccept={vi.fn()} onDeny={onDeny} now={() => T0} />);
    await screen.findByRole('alertdialog');
    await vi.waitFor(() => expect(document.activeElement?.textContent).toBe('Deny'));
    await user.keyboard('{Escape}');
    expect(screen.getByRole('alertdialog')).toBeTruthy();
    await user.keyboard('{Enter}');
    expect(onDeny).toHaveBeenCalledWith('user');
  });

  it('has no axe violations', async () => {
    render(
      <RequestDialog request={request()} onAccept={vi.fn()} onDeny={vi.fn()} now={() => T0} />,
    );
    const dialog = await screen.findByRole('alertdialog');
    expect(await axeViolations(dialog)).toEqual([]);
  });
});

describe('host-role store', () => {
  it('parses native incomingRequest events and drops malformed ones', () => {
    const ok = parseIncomingRequest({
      type: 'incomingRequest',
      session: 'x',
      peer: 'p',
      fingerprint: 'f',
      kind: 'anonymous',
      sas: [1, 2, 3, 4, 5],
      requested: ['view', 'bogus'],
      allowed: [],
      acceptEnabledAt: 10,
      expiresAt: 20,
    });
    expect(ok?.requested).toEqual(['view']);
    expect(ok?.allowed).toEqual(defaultAllowed('anonymous'));
    expect(ok?.sas).toEqual([1, 2, 3, 4, 5]);
    expect(
      parseIncomingRequest({ type: 'incomingRequest', session: 'x', kind: 'evil' }),
    ).toBeNull();
    expect(parseIncomingRequest({ type: 'status' })).toBeNull();
    expect(parseIncomingRequest(null)).toBeNull();
  });

  it('queues requests and dismisses them', () => {
    render(<Probe />);
    act(() => {
      pushHostEvent(incoming('a'));
      pushHostEvent(incoming('b'));
    });
    expect(screen.getByTestId('probe').textContent).toBe('a');
    act(() => dismissIncomingRequest('a'));
    expect(screen.getByTestId('probe').textContent).toBe('b');
    act(() => dismissIncomingRequest('b'));
    expect(screen.getByTestId('probe').textContent).toBe('none');
  });

  it('answers through scrin_accept / scrin_reject', async () => {
    const invoke = vi.fn(() => Promise.resolve(null));
    const actions = hostRoleActionsFromInvoke(invoke);
    await actions.accept('s', ['view', 'input']);
    await actions.reject('s');
    expect(invoke).toHaveBeenNthCalledWith(1, 'scrin_accept', {
      session: 's',
      permissions: ['view', 'input'],
    });
    expect(invoke).toHaveBeenNthCalledWith(2, 'scrin_reject', { session: 's' });
  });
});
