// Renders a page's `maturity` frontmatter as labels under its title. GPUI Kit
// is used in production, but its capabilities do not share one track record;
// a label on the page states that where the reader decides whether to use it.
// Each label links to the definitions on the documentation home.

export const MATURITY_LEVELS = ['stable', 'preview', 'experimental', 'showcase-only', 'platform-dependent'];

const labels = {
  en: {
    stable: ['Stable', 'Used by production desktop applications.'],
    preview: ['Preview', 'Usable and documented; the API and edge-case behavior may still change.'],
    experimental: ['Experimental', 'Works with known gaps; validate it for your product before depending on it.'],
    'showcase-only': ['Showcase only', 'Currently used to demonstrate components, not to ship applications.'],
    'platform-dependent': ['Platform-dependent', 'Availability or behavior differs by platform; this page lists the differences.'],
  },
};

const definitions = {
  en: '/docs#maturity',
};

export function remarkMaturity() {
  return (tree, file) => {
    const levels = file.data?.astro?.frontmatter?.maturity;
    if (!levels) return;

    const path = file.path ?? file.history?.[0] ?? '';
    const lang = 'en';
    const list = Array.isArray(levels) ? levels : [levels];
    for (const level of list) {
      if (!MATURITY_LEVELS.includes(level)) {
        throw new Error(`${path}: unknown maturity "${level}"; use one of ${MATURITY_LEVELS.join(', ')}`);
      }
    }

    const heading = tree.children.findIndex((node) => node.type === 'heading' && node.depth === 1);
    const children = list.map((level) => {
      const [text, title] = labels[lang][level];
      return {
        type: 'link',
        // A site-root path: remarkDocLinks, which runs after this plugin,
        // moves it inside the version being built.
        url: definitions[lang],
        title,
        data: { hProperties: { className: ['doc-maturity__label'], dataMaturity: level } },
        children: [{ type: 'text', value: text }],
      };
    });
    tree.children.splice(heading + 1, 0, {
      type: 'paragraph',
      data: { hName: 'p', hProperties: { className: ['doc-maturity'] } },
      children,
    });
  };
}
