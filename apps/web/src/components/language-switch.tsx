import {
  LOCALE_NAMES,
  LOCALES,
  setLocale,
  useLocale,
  useTranslation,
  type Locale,
} from '@scrin/i18n';
import { IconButton, Menu, MenuRadioGroup, MenuRadioItem } from '@scrin/ui';
import { Languages } from 'lucide-react';

export function LanguageSwitch() {
  const { t } = useTranslation();
  const locale = useLocale();
  return (
    <Menu
      align="end"
      trigger={
        <IconButton label={`${t('topbar.language')}: ${LOCALE_NAMES[locale]}`}>
          <Languages aria-hidden />
        </IconButton>
      }
    >
      <MenuRadioGroup<Locale> value={locale} onValueChange={(l) => void setLocale(l)}>
        {LOCALES.map((l) => (
          <MenuRadioItem key={l} value={l} lang={l}>
            {LOCALE_NAMES[l]}
          </MenuRadioItem>
        ))}
      </MenuRadioGroup>
    </Menu>
  );
}
