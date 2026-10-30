// Fields per spec item type, and which are long text: the same as the TUI's (MVP-041).

import type { SpecType } from './types';

export interface FieldSpec {
  name: string;
  long: boolean;
}

export const FIELDS: Record<SpecType, FieldSpec[]> = {
  Requirement: [
    { name: 'text', long: true },
    { name: 'kind', long: false },
    { name: 'priority', long: false },
    { name: 'status', long: false },
  ],
  Decision: [
    { name: 'title', long: false },
    { name: 'decision', long: true },
    { name: 'context', long: true },
    { name: 'consequences', long: true },
    { name: 'status', long: false },
  ],
  Design: [
    { name: 'title', long: false },
    { name: 'text', long: true },
    { name: 'status', long: false },
  ],
  Constraint: [
    { name: 'title', long: false },
    { name: 'text', long: true },
    { name: 'scope', long: false },
    { name: 'status', long: false },
  ],
};

export const STATUSES = ['active', 'draft', 'proposed', 'accepted', 'superseded', 'deprecated', 'retired'];

/** A property as text, the way the TUI's forms show it. */
export function fieldText(v: unknown): string {
  if (v === null || v === undefined) return '';
  if (typeof v === 'string') return v;
  return JSON.stringify(v);
}

/** The values of `fields` from an item's properties. */
export function valuesOf(type: SpecType, props: Record<string, unknown>): Record<string, string> {
  const out: Record<string, string> = {};
  for (const f of FIELDS[type]) out[f.name] = fieldText(props[f.name]);
  return out;
}

export function label(field: string): string {
  return field.charAt(0).toUpperCase() + field.slice(1);
}
