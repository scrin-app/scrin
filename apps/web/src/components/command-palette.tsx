import { Combobox } from '@base-ui/react/combobox';
import { Dialog as BaseDialog } from '@base-ui/react/dialog';
import { formatScrinId, useTranslation } from '@scrin/i18n';
import { cn, Kbd, toast, useTheme } from '@scrin/ui';
import { useQueryClient } from '@tanstack/react-query';
import { useNavigate } from '@tanstack/react-router';
import {
  ArrowRight,
  Copy,
  House,
  KeyRound,
  Monitor,
  MonitorSmartphone,
  Moon,
  Search,
  Settings,
} from 'lucide-react';
import { useEffect, useMemo, useState, type ReactNode } from 'react';

import { host } from '../host';
import { codeQuery, myIdQuery, useRegenerateCode } from '../lib/engine';
import { DEVICES } from '../lib/mock-data';

interface Command {
  id: string;
  group: 'navigate' | 'actions' | 'devices';
  label: string;
  hint?: string;
  icon: ReactNode;
  run: () => void;
}

/**
 * Ctrl/⌘+K palette: a Dialog hosting an inline Base UI Combobox, so the list
 * gets combobox/listbox semantics and arrow-key navigation for free.
 */
export function CommandPalette() {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState('');
  const navigate = useNavigate();
  const qc = useQueryClient();
  const { toggleMode } = useTheme();
  const regenerate = useRegenerateCode();

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        setOpen((o) => !o);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  const commands = useMemo<Command[]>(() => {
    const go =
      (to: '/' | '/devices' | '/settings'): (() => void) =>
      () => {
        void navigate({ to });
      };
    const base: Command[] = [
      { id: 'nav-home', group: 'navigate', label: t('nav.home'), icon: <House />, run: go('/') },
      {
        id: 'nav-devices',
        group: 'navigate',
        label: t('nav.devices'),
        icon: <MonitorSmartphone />,
        run: go('/devices'),
      },
      {
        id: 'nav-settings',
        group: 'navigate',
        label: t('nav.settings'),
        icon: <Settings />,
        run: go('/settings'),
      },
      {
        id: 'act-code',
        group: 'actions',
        label: t('command.newCode'),
        icon: <KeyRound />,
        run: () => {
          regenerate.mutate(undefined, {
            onSuccess: () => toast.success(t('home.codeRegenerated')),
          });
        },
      },
      {
        id: 'act-copy',
        group: 'actions',
        label: t('command.copyId'),
        icon: <Copy />,
        run: () => {
          void qc.query(myIdQuery).then(async (id) => {
            await host.writeClipboard(id);
            toast.success(t('home.idCopied'));
          });
        },
      },
      {
        id: 'act-theme',
        group: 'actions',
        label: t('command.toggleTheme'),
        icon: <Moon />,
        run: toggleMode,
      },
    ];
    const devices: Command[] = DEVICES.slice(0, 12).map((d) => ({
      id: `dev-${d.id}`,
      group: 'devices',
      label: d.name,
      hint: formatScrinId(d.id),
      icon: <Monitor />,
      run: () => {
        void navigate({ to: '/', search: { id: d.id } });
      },
    }));
    return [...base, ...devices];
  }, [t, navigate, qc, toggleMode, regenerate]);

  const digits = query.replace(/\D/g, '');
  const filtered = useMemo((): Command[] => {
    const q = query.trim().toLowerCase();
    const list = q
      ? commands.filter(
          (c) =>
            c.label.toLowerCase().includes(q) ||
            (c.hint?.replace(/\s/g, '').includes(digits) ?? false),
        )
      : commands;
    if (digits.length === 9) {
      return [
        {
          id: 'connect-id',
          group: 'actions',
          label: t('command.connectTo', { id: formatScrinId(digits) }),
          icon: <ArrowRight />,
          run: () => {
            void navigate({ to: '/', search: { id: digits } });
          },
        },
        ...list,
      ];
    }
    return list;
  }, [commands, query, digits, t, navigate]);

  const groups: { key: Command['group']; label: string }[] = [
    { key: 'actions', label: t('command.groupActions') },
    { key: 'navigate', label: t('command.groupNavigate') },
    { key: 'devices', label: t('command.groupDevices') },
  ];

  const run = (c: Command | null): void => {
    if (!c) return;
    setOpen(false);
    setQuery('');
    c.run();
  };

  // Prefetch so "copy my ID" is instant.
  useEffect(() => {
    if (open) {
      qc.query(myIdQuery).catch(() => undefined);
      qc.query(codeQuery).catch(() => undefined);
    }
  }, [open, qc]);

  return (
    <BaseDialog.Root open={open} onOpenChange={(o: boolean) => setOpen(o)}>
      <BaseDialog.Trigger
        className={cn(
          'flex h-10 min-w-0 flex-1 items-center gap-2 rounded-lg border border-outline bg-surface-2/60 px-3 text-sm text-muted',
          'transition-colors duration-(--scrin-dur-fast) hover:text-fg sm:max-w-md',
        )}
      >
        <Search aria-hidden className="size-4 shrink-0" />
        <span className="truncate">{t('topbar.search')}</span>
        <Kbd className="ml-auto hidden sm:inline-flex">{t('topbar.searchShortcut')}</Kbd>
      </BaseDialog.Trigger>
      <BaseDialog.Portal>
        <BaseDialog.Backdrop className="fixed inset-0 z-50 bg-black/40 backdrop-blur-[2px] transition-opacity duration-(--scrin-dur) data-ending-style:opacity-0 data-starting-style:opacity-0" />
        <BaseDialog.Popup
          className={cn(
            'fixed top-[12vh] left-1/2 z-50 w-[min(40rem,calc(100vw-1.5rem))] -translate-x-1/2 overflow-hidden rounded-xl glass-panel shadow-2xl outline-none',
            'transition-[opacity,transform] duration-(--scrin-dur) ease-scrin data-ending-style:opacity-0 data-starting-style:scale-[0.98] data-starting-style:opacity-0',
          )}
        >
          <BaseDialog.Title className="sr-only">{t('command.title')}</BaseDialog.Title>
          <Combobox.Root<Command>
            inline
            open
            items={filtered}
            filter={null}
            inputValue={query}
            onInputValueChange={(v: string) => setQuery(v)}
            itemToStringLabel={(c: Command) => c.label}
            onValueChange={(c: Command | null) => run(c)}
            autoHighlight
          >
            <div className="flex items-center gap-2 border-b border-outline px-4">
              <Search aria-hidden className="size-4 text-muted" />
              <Combobox.Input
                aria-label={t('command.placeholder')}
                placeholder={t('command.placeholder')}
                className="h-14 w-full bg-transparent text-base text-fg outline-none placeholder:text-muted"
              />
              <Kbd>Esc</Kbd>
            </div>
            <Combobox.Empty className="px-4 py-10 text-center text-sm text-muted empty:hidden">
              {filtered.length === 0 ? t('command.empty', { query }) : null}
            </Combobox.Empty>
            <Combobox.List className="max-h-[min(60vh,28rem)] overflow-y-auto p-2 empty:p-0">
              {groups.map((g) => {
                const items = filtered.filter((c) => c.group === g.key);
                if (items.length === 0) return null;
                return (
                  <Combobox.Group key={g.key} items={items} className="mb-1">
                    <Combobox.GroupLabel className="px-2 pt-2 pb-1 text-xs font-medium text-muted">
                      {g.label}
                    </Combobox.GroupLabel>
                    {items.map((c) => (
                      <Combobox.Item
                        key={c.id}
                        value={c}
                        className={cn(
                          'flex h-11 cursor-default items-center gap-3 rounded-lg px-3 text-sm text-fg select-none',
                          'data-highlighted:bg-accent/12 [&_svg]:size-4 [&_svg]:text-muted data-highlighted:[&_svg]:text-accent',
                        )}
                      >
                        <span aria-hidden>{c.icon}</span>
                        <span className="flex-1 truncate">{c.label}</span>
                        {c.hint ? (
                          <span className="font-mono text-xs text-muted">{c.hint}</span>
                        ) : null}
                      </Combobox.Item>
                    ))}
                  </Combobox.Group>
                );
              })}
            </Combobox.List>
          </Combobox.Root>
        </BaseDialog.Popup>
      </BaseDialog.Portal>
    </BaseDialog.Root>
  );
}
