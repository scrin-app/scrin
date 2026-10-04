---
name: add-locale-string
description: Add, rename or remove a user-visible string in scrin in English AND Romanian — i18next keys in packages/i18n for desktop/web and Android strings.xml in values/ and values-ro/ — with correct diacritics, plurals, interpolation and drift tests. Use for any new UI text, aria-label, toast, error message or notification.
---

# Add a locale string

Every key exists in `en` and `ro` in the same commit. The drift tests fail otherwise.

## 1. Find the namespace and existing wording

```powershell
rg --files packages/i18n | rg 'locales'
rg -n '"connect' packages/i18n                   # reuse an existing key before adding one
rg -n 'name="connect' android/app/src/main/res/values/strings.xml
```

Reuse a key when the meaning is identical; add a new key when only the words match.

## 2. Name the key

- `namespace.screen.element[.state]`, camelCase segments: `session.toolbar.disconnect`,
  `permissions.clipboard.description`, `errors.pairing.wrongCode`.
- Never derive keys from the English text. Never build sentences by concatenating keys.

## 3. Add to both web/desktop locales

- `packages/i18n/.../en/<namespace>.json` and `.../ro/<namespace>.json`, same key path.
- Interpolation: `{{name}}`. Plurals: i18next `_one` / `_other` in EN; Romanian needs
  `_one`, `_few`, `_other` (e.g. 1 dispozitiv, 2 dispozitive, 20 de dispozitive).
- Romanian: diacritics with comma below (ș, ț — not ş, ţ), plus ă, â, î. Formal "dumneavoastră"
  is not used; address the user with the imperative ("Conectează-te").
- Fiscal/legal terms only where relevant, spelled officially.

## 4. Use it

```tsx
const { t } = useTranslation('session');
<button aria-label={t('toolbar.disconnect')}>…</button>
```

No literal user-visible strings in JSX, including `aria-label`, `title`, `alt` and toasts.

## 5. Android

- `android/app/src/main/res/values/strings.xml` and `values-ro/strings.xml` (and `tv/` if the TV
  module shows it). Key `snake_case` mirroring the i18n key: `session_toolbar_disconnect`.
- Plurals with `<plurals>` (`one`, `few`, `other` for ro). Escape apostrophes (`\'`).
- `stringResource(R.string.…)` in Compose; `contentDescription` for icon-only controls.

## 6. Check

```powershell
pwsh -NoProfile -File scripts/gates.ps1 -Only js,android
rg -n '[şţŞŢ]' packages/i18n android                   # must print nothing (cedilla forms)
```

The js lane runs the i18n drift test (keys equal across locales, no empty values).

## 7. Removing or renaming

- Remove from **both** locales and both platforms; `rg` the old key across `apps/`, `packages/`
  and `android/` to confirm no caller remains.

## Done means

- [ ] Key present in `en` and `ro` (and Android `values/` + `values-ro/` if shown there)
- [ ] Romanian uses ș/ț with comma below; plurals have one/few/other
- [ ] No string literal left in the component; aria-labels translated
- [ ] Drift test and `gates.ps1 -Only js,android` green
- [ ] Old keys removed everywhere when renaming
