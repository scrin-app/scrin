import axe from 'axe-core';

/**
 * Runs axe on a container and returns readable violations. axe-core is used
 * directly: the matcher wrappers add nothing a five-line helper does not.
 *
 * `color-contrast` is disabled here because happy-dom computes no styles;
 * contrast is proven numerically in `theme/contrast.test.ts` over every preset
 * and mode, and in the Playwright axe run against real rendering.
 */
export async function axeViolations(container: Element): Promise<string[]> {
  const result = await axe.run(container, {
    rules: {
      'color-contrast': { enabled: false },
      // A component rendered alone is not a page.
      region: { enabled: false },
      'landmark-one-main': { enabled: false },
      'page-has-heading-one': { enabled: false },
    },
  });
  return result.violations.map(
    (v) => `${v.id}: ${v.help} (${v.nodes.map((n) => n.target.join(' ')).join(', ')})`,
  );
}
