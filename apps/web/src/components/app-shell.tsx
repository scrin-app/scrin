import { useTranslation } from '@scrin/i18n';
import { cn, IconButton, Tooltip, TooltipProvider } from '@scrin/ui';
import { Link, Outlet } from '@tanstack/react-router';
import { PanelLeftClose, PanelLeftOpen } from 'lucide-react';
import { useState } from 'react';

import { host } from '../host';
import { CommandPalette } from './command-palette';
import { LanguageSwitch } from './language-switch';
import { Logo } from './logo';
import { NAV_ITEMS } from './nav-items';
import { ThemeToggle } from './theme-toggle';

const SIDEBAR_KEY = 'scrin.sidebar';

/**
 * Sidebar on ≥ md, a native-style bottom tab bar on phones. On ultrawide
 * screens the content is capped and centred with breathing room on the sides.
 */
export function AppShell() {
  const { t } = useTranslation();
  const [collapsed, setCollapsed] = useState(() => host.storage.get(SIDEBAR_KEY) === '1');
  const toggle = () => {
    setCollapsed((c) => {
      host.storage.set(SIDEBAR_KEY, c ? '0' : '1');
      return !c;
    });
  };

  return (
    <TooltipProvider>
      <div className="flex min-h-dvh bg-bg text-fg">
        <a
          href="#main"
          className="sr-only z-[60] rounded-md bg-accent px-3 py-2 text-accent-fg focus:not-sr-only focus:fixed focus:top-3 focus:left-3"
        >
          {t('app.skipToContent')}
        </a>

        <aside
          className={cn(
            'sticky top-0 hidden h-dvh shrink-0 flex-col border-y-0 border-l-0 glass-panel md:flex',
            'transition-[width] duration-(--scrin-dur) ease-scrin',
            collapsed ? 'w-[4.5rem]' : 'w-64',
          )}
        >
          <div className={cn('flex h-16 items-center px-4', collapsed && 'justify-center px-0')}>
            <Link to="/" aria-label={t('nav.home')} className="rounded-md">
              <Logo compact={collapsed} />
            </Link>
          </div>
          <nav aria-label={t('nav.primary')} className="flex flex-1 flex-col gap-1 px-3 py-2">
            {NAV_ITEMS.map(({ to, labelKey, icon: Icon }) => {
              const link = (
                <Link
                  key={to}
                  to={to}
                  activeOptions={{ exact: to === '/' }}
                  className={cn(
                    'group flex h-11 items-center gap-3 rounded-lg px-3 text-sm font-medium text-muted',
                    'transition-colors duration-(--scrin-dur-fast) hover:bg-surface-2 hover:text-fg',
                    'data-[status=active]:bg-accent/12 data-[status=active]:text-fg',
                    collapsed && 'justify-center px-0',
                  )}
                  aria-label={collapsed ? t(labelKey) : undefined}
                >
                  <Icon
                    aria-hidden
                    className="size-5 shrink-0 group-data-[status=active]:text-accent"
                  />
                  {collapsed ? null : <span>{t(labelKey)}</span>}
                </Link>
              );
              return collapsed ? (
                <Tooltip key={to} content={t(labelKey)} side="right">
                  {link}
                </Tooltip>
              ) : (
                link
              );
            })}
          </nav>
          <div className={cn('flex p-3', collapsed ? 'justify-center' : 'justify-end')}>
            <IconButton label={collapsed ? t('nav.expand') : t('nav.collapse')} onClick={toggle}>
              {collapsed ? <PanelLeftOpen aria-hidden /> : <PanelLeftClose aria-hidden />}
            </IconButton>
          </div>
        </aside>

        <div className="flex min-w-0 flex-1 flex-col">
          <header className="sticky top-0 z-30 border-x-0 border-t-0 glass-panel pt-[env(safe-area-inset-top)]">
            <div className="mx-auto flex h-16 w-full max-w-[110rem] items-center gap-3 px-4 sm:px-6">
              <Link to="/" aria-label={t('nav.home')} className="rounded-md md:hidden">
                <Logo compact />
              </Link>
              <CommandPalette />
              <div className="ml-auto flex items-center gap-1">
                <LanguageSwitch />
                <ThemeToggle />
              </div>
            </div>
          </header>

          <main
            id="main"
            tabIndex={-1}
            className="@container mx-auto w-full max-w-[110rem] flex-1 px-4 pt-6 pb-[calc(5.5rem+env(safe-area-inset-bottom))] outline-none sm:px-6 md:pb-10 3xl:px-12"
          >
            <Outlet />
          </main>
        </div>

        <nav
          aria-label={t('nav.primary')}
          className="fixed inset-x-0 bottom-0 z-40 border-x-0 border-b-0 glass-panel pb-[env(safe-area-inset-bottom)] md:hidden"
        >
          <ul className="grid grid-cols-3">
            {NAV_ITEMS.map(({ to, labelKey, icon: Icon }) => (
              <li key={to}>
                <Link
                  to={to}
                  activeOptions={{ exact: to === '/' }}
                  className={cn(
                    'group flex h-16 flex-col items-center justify-center gap-1 text-[0.7rem] font-medium text-muted',
                    'data-[status=active]:text-fg',
                  )}
                >
                  <span className="grid h-7 w-14 place-items-center rounded-full transition-colors duration-(--scrin-dur-fast) group-data-[status=active]:bg-accent/15">
                    <Icon aria-hidden className="size-5 group-data-[status=active]:text-accent" />
                  </span>
                  {t(labelKey)}
                </Link>
              </li>
            ))}
          </ul>
        </nav>
      </div>
    </TooltipProvider>
  );
}
