import { describe, expect, it } from 'vitest';
import { applyFilters, facetCounts, readView, sortItems, writeView } from '../specFilter';
import type { SpecItem } from '../types';

const item = (key: string | null, type: SpecItem['type'], extra: Partial<SpecItem> = {}): SpecItem => ({
  id: `id-${key ?? Math.random()}`,
  type,
  key,
  title: `Title ${key}`,
  text: `Text of ${key}`,
  status: 'active',
  kind: null,
  priority: null,
  source: null,
  props: {},
  links: 0,
  staleLinks: 0,
  ...extra,
});

const items = [
  item('FR-10', 'Requirement', { links: 2, staleLinks: 1 }),
  item('FR-2', 'Requirement', { status: 'draft', links: 1 }),
  item('ADR-0001', 'Decision', { status: 'accepted', pendingChange: { seq: 1, kind: 'edited', fields: [], origin: 'ui' } }),
  item(null, 'Design', { source: { path: 'docs/a.md', line: 3 } }),
];

describe('facets', () => {
  it('show everything when nothing is checked', () => {
    expect(applyFilters(items, '', {})).toHaveLength(4);
    expect(applyFilters(items, '', { type: [] })).toHaveLength(4);
  });

  it('treat values of one facet as alternatives, and facets as narrowing each other', () => {
    expect(applyFilters(items, '', { type: ['Requirement', 'Decision'] }).map((i) => i.key)).toEqual(['FR-10', 'FR-2', 'ADR-0001']);
    expect(applyFilters(items, '', { type: ['Requirement'], status: ['draft'] }).map((i) => i.key)).toEqual(['FR-2']);
    expect(applyFilters(items, '', { links: ['stale'] }).map((i) => i.key)).toEqual(['FR-10']);
    expect(applyFilters(items, '', { pending: ['pending'] }).map((i) => i.key)).toEqual(['ADR-0001']);
    expect(applyFilters(items, '', { source: ['document'] }).map((i) => i.type)).toEqual(['Design']);
    expect(applyFilters(items, 'adr', {}).map((i) => i.key)).toEqual(['ADR-0001']);
  });

  it('count each value with the other facets applied', () => {
    const counts = facetCounts(items, '', { type: ['Requirement'] });
    // The type facet ignores its own selection, so every type keeps its count.
    expect(counts.type).toEqual([
      { value: 'Requirement', count: 2 },
      { value: 'Decision', count: 1 },
      { value: 'Design', count: 1 },
      { value: 'Constraint', count: 0 },
    ]);
    // Statuses count only requirements, in the usual order.
    expect(counts.status).toEqual([
      { value: 'active', count: 1 },
      { value: 'draft', count: 1 },
    ]);
  });
});

describe('sorting', () => {
  it('sorts keys naturally, keyless items last, and reverses', () => {
    expect(sortItems(items, 'key', 'asc').map((i) => i.key)).toEqual(['ADR-0001', 'FR-2', 'FR-10', null]);
    expect(sortItems(items, 'key', 'desc').map((i) => i.key)).toEqual([null, 'FR-10', 'FR-2', 'ADR-0001']);
  });

  it('sorts by links, stale first when descending', () => {
    expect(sortItems(items, 'links', 'desc')[0].key).toBe('FR-10');
  });
});

describe('the view in the address', () => {
  it('round-trips', () => {
    const params = writeView(new URLSearchParams('add=Decision'), {
      text: 'greet',
      selection: { type: ['Requirement', 'Design'], status: [] },
      sort: 'links',
      dir: 'desc',
    });
    expect(params.toString()).toBe('add=Decision&type=Requirement%2CDesign&q=greet&sort=links&dir=desc');
    expect(readView(params)).toEqual({
      text: 'greet',
      selection: { type: ['Requirement', 'Design'] },
      sort: 'links',
      dir: 'desc',
    });
    expect(readView(new URLSearchParams('sort=bogus')).sort).toBe('key');
  });
});
