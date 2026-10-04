import { useTranslation } from '@scrin/i18n';
import { IconButton, useTheme } from '@scrin/ui';
import { Moon, Sun } from 'lucide-react';
import { flushSync } from 'react-dom';

/** Light/dark toggle with a View Transition cross-fade where supported. */
export function ThemeToggle() {
  const { t } = useTranslation();
  const { resolvedMode, toggleMode, motionScale } = useTheme();
  const onClick = () => {
    if (motionScale > 0 && typeof document.startViewTransition === 'function') {
      document.startViewTransition(() => {
        flushSync(toggleMode);
      });
    } else {
      toggleMode();
    }
  };
  return (
    <IconButton label={t('topbar.themeToggle')} onClick={onClick}>
      {resolvedMode === 'dark' ? <Sun aria-hidden /> : <Moon aria-hidden />}
    </IconButton>
  );
}
