import { useTranslation } from '@scrin/i18n';
import { Toaster as SonnerToaster } from 'sonner';

import { useOptionalTheme } from '../theme/ThemeProvider';

/**
 * Sonner, themed from the live tokens. Mount once near the root.
 */
export function Toaster() {
  const theme = useOptionalTheme();
  const { t } = useTranslation();
  return (
    <SonnerToaster
      theme={theme?.resolvedMode ?? 'system'}
      position="bottom-right"
      containerAriaLabel={t('ui.notifications')}
      closeButton
      toastOptions={{
        style: {
          background: 'var(--scrin-surface)',
          color: 'var(--scrin-fg)',
          border: '1px solid var(--scrin-outline)',
        },
      }}
    />
  );
}

export { toast } from 'sonner';
