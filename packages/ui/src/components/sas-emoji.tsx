import { useTranslation } from '@scrin/i18n';

import { cn } from '../lib/cn';

/**
 * Emoji table, index-aligned with `crates/scrin-crypto/src/sas.rs::EMOJI`.
 * The order is part of the protocol: never reorder. Names are translated
 * through `sasEmoji.e<index>`.
 */
export const SAS_EMOJI: readonly string[] = [
  '🐶',
  '🐱',
  '🦁',
  '🐴',
  '🦄',
  '🐷',
  '🐘',
  '🐰',
  '🐼',
  '🐓',
  '🐧',
  '🐢',
  '🐟',
  '🐙',
  '🦋',
  '🌷',
  '🌳',
  '🌵',
  '🍄',
  '🌏',
  '🌙',
  '☁️',
  '🔥',
  '🍌',
  '🍎',
  '🍓',
  '🌽',
  '🍕',
  '🎂',
  '❤️',
  '😀',
  '🤖',
  '🎩',
  '👓',
  '🔧',
  '🎅',
  '👍',
  '☂️',
  '⌛',
  '⏰',
  '🎁',
  '💡',
  '📕',
  '✏️',
  '📎',
  '✂️',
  '🔒',
  '🔑',
  '🔨',
  '☎️',
  '🏁',
  '🚂',
  '🚲',
  '✈️',
  '🚀',
  '🏆',
  '⚽',
  '🎸',
  '🎺',
  '🔔',
  '⚓',
  '🎧',
  '📁',
  '📌',
];

export function SasEmoji({
  indices,
  className,
}: {
  indices: readonly number[];
  className?: string;
}) {
  const { t } = useTranslation();
  // The whole name table at once: index-aligned because object key order is
  // insertion order (e0…e63).
  const table = Object.values(t('sasEmoji', { returnObjects: true }));
  const names = indices.map((i) => table[i] ?? '?');
  return (
    <ol
      aria-label={`${t('ui.sasLabel')}: ${names.join(', ')}`}
      className={cn('grid grid-cols-5 gap-2', className)}
    >
      {indices.map((i, pos) => (
        <li
          // Position matters as much as the emoji, so the key encodes both.
          key={`${pos}-${i}`}
          className="flex flex-col items-center gap-1 rounded-lg bg-surface-2 px-1 py-3 text-center"
        >
          <span aria-hidden className="text-[clamp(1.75rem,8cqi,2.75rem)] leading-none">
            {SAS_EMOJI[i] ?? '?'}
          </span>
          <span className="text-[0.7rem] leading-tight font-medium text-muted">{names[pos]}</span>
        </li>
      ))}
    </ol>
  );
}
