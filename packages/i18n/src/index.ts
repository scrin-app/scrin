import { createInstance, type i18n as I18n } from 'i18next';
import { initReactI18next, useTranslation } from 'react-i18next';

import { en } from './locales/en';
import { ro } from './locales/ro';
import { isLocale, LOCALES, type Locale } from './types';

// Makes `t('home.connect')` type-checked in every package: an unknown key is a
// compile error. Lives here (not in a .d.ts) so declaration emit carries it
// across project references.
declare module 'i18next' {
  interface CustomTypeOptions {
    defaultNS: 'translation';
    resources: { translation: typeof en };
    returnNull: false;
  }
}

export {
  LOCALES,
  LOCALE_NAMES,
  LOCALE_TAGS,
  isLocale,
  type Locale,
  type Resources,
  type TranslationKey,
} from './types';
export * from './format';
export { useTranslation, Trans } from 'react-i18next';
export { en } from './locales/en';
export { ro } from './locales/ro';

/** Storage key for the persisted language preference. */
export const LOCALE_STORAGE_KEY = 'scrin.locale';

/** Minimal key/value store, so the host decides where preferences live. */
export interface LocaleStorage {
  get(key: string): string | null;
  set(key: string, value: string): void;
}

/** Picks the best supported locale from a list of BCP 47 tags. */
export function resolveLocale(preferred: readonly string[]): Locale {
  for (const tag of preferred) {
    const base = tag.toLowerCase().split('-')[0];
    if (isLocale(base)) return base;
  }
  return 'en';
}

/** Stored preference first, then the browser languages, then English. */
export function detectLocale(storage?: LocaleStorage): Locale {
  const stored = storage?.get(LOCALE_STORAGE_KEY);
  if (isLocale(stored)) return stored;
  if (typeof navigator === 'undefined') return 'en';
  return resolveLocale(navigator.languages.length > 0 ? navigator.languages : [navigator.language]);
}

let instance: I18n | null = null;
let store: LocaleStorage | undefined;

export function initI18n(opts: { locale?: Locale; storage?: LocaleStorage } = {}): I18n {
  if (instance) return instance;
  store = opts.storage;
  const lng = opts.locale ?? detectLocale(store);
  const i = createInstance();
  void i.use(initReactI18next).init({
    lng,
    fallbackLng: 'en',
    supportedLngs: [...LOCALES],
    resources: { en: { translation: en }, ro: { translation: ro } },
    // React escapes already; double-escaping would mangle diacritics.
    interpolation: { escapeValue: false },
    initAsync: false,
    returnNull: false,
  });
  if (typeof document !== 'undefined') document.documentElement.lang = lng;
  instance = i;
  return i;
}

export function getI18n(): I18n {
  return instance ?? initI18n();
}

export async function setLocale(locale: Locale): Promise<void> {
  const i = getI18n();
  await i.changeLanguage(locale);
  store?.set(LOCALE_STORAGE_KEY, locale);
  if (typeof document !== 'undefined') document.documentElement.lang = locale;
}

export function currentLocale(): Locale {
  const lng = getI18n().language;
  return isLocale(lng) ? lng : 'en';
}

/** The active locale, re-rendering when it changes. */
export function useLocale(): Locale {
  const { i18n } = useTranslation();
  return isLocale(i18n.language) ? i18n.language : 'en';
}
