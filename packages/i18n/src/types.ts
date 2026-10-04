import type { en } from './locales/en';

export const LOCALES = ['en', 'ro'] as const;
export type Locale = (typeof LOCALES)[number];

/** Each locale's own name, so the picker reads naturally in any language. */
export const LOCALE_NAMES: Record<Locale, string> = {
  en: 'English',
  ro: 'Română',
};

/** BCP 47 tags used for `Intl` formatting. */
export const LOCALE_TAGS: Record<Locale, string> = {
  en: 'en-GB',
  ro: 'ro-RO',
};

type Widen<T> = { readonly [K in keyof T]: T[K] extends string ? string : Widen<T[K]> };

/** The key schema: the English object with every leaf widened to `string`. */
export type Resources = Widen<typeof en>;

type Leaves<T, P extends string = ''> = {
  [K in keyof T & string]: T[K] extends string ? `${P}${K}` : Leaves<T[K], `${P}${K}.`>;
}[keyof T & string];

/** Every valid dotted key, e.g. `home.connect`. */
export type TranslationKey = Leaves<typeof en>;

export function isLocale(value: unknown): value is Locale {
  return typeof value === 'string' && (LOCALES as readonly string[]).includes(value);
}
