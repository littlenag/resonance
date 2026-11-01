import { describe, expect, it } from 'vitest';
import { screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { SpecPage } from '../pages/SpecPage';
import type { SpecItem } from '../types';
import { mockApi } from './api';
import { renderAt } from './render';

const item = (key: string, text: string, extra: Partial<SpecItem> = {}): SpecItem => ({
  id: `req:${key}`,
  type: 'Requirement',
  key,
  title: text,
  text,
  status: 'active',
  kind: 'functional',
  source: null,
  props: { text, kind: 'functional', status: 'active' },
  links: 2,
  staleLinks: 0,
  ...extra,
});

const routes = [
  { path: '/spec', element: <SpecPage /> },
  { path: '/spec/:ref', element: <SpecPage /> },
];

describe('spec items', () => {
  it('lists items, marks pending changes and shows the selected item', async () => {
    const one = item('FR-001', 'The system shall greet.', {
      pendingChange: { seq: 4, kind: 'edited', fields: ['text'], origin: 'ui' },
    });
    const two = item('FR-002', 'The system shall wave.');
    mockApi({
      spec_list: () => [one, two],
      spec_show: (a) => (a.ref === two.id || a.ref === two.key ? two : one),
      show: () => ({ node: {}, properties: {}, links: [{ relation: 'satisfies', status: 'stale', other: { label: 'src/greet.rs::greet' } }], annotations: [] }),
    });
    renderAt('/spec/FR-002', routes);
    expect(await screen.findByTestId('item-FR-001')).toBeInTheDocument();
    expect(screen.getByTestId('item-FR-001')).toHaveAttribute('data-pending');
    expect(screen.getByTestId('item-FR-002')).not.toHaveAttribute('data-pending');
    // The detail is the item in the address.
    expect(await screen.findByRole('heading', { name: 'FR-002' })).toBeInTheDocument();
    expect(await screen.findByText('src/greet.rs::greet')).toBeInTheDocument();
  });

  it('adds an item with the suggested key and shows a refused key', async () => {
    const calls = mockApi({
      spec_list: () => [item('FR-001', 'The system shall greet.')],
      spec_show: () => item('FR-001', 'The system shall greet.'),
      show: () => ({ node: {}, properties: {}, links: [], annotations: [] }),
      spec_suggest_key: () => ({ key: 'FR-002' }),
      spec_add: (a) => {
        if (a.key === 'FR-001') throw new Error('FR-001 already exists');
        return item(String(a.key), 'x');
      },
    });
    const user = userEvent.setup();
    renderAt('/spec?add=Requirement', routes);
    const keyInput = await screen.findByLabelText('Key');
    await waitFor(() => expect(keyInput).toHaveValue('FR-002'));
    await user.clear(keyInput);
    await user.type(keyInput, 'FR-001');
    await user.type(screen.getByRole('textbox', { name: 'Text' }), 'The system shall wave.');
    await user.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Add' }));
    await waitFor(() => expect(calls.map((c) => c.method)).toContain('spec_add'));
    expect(await screen.findByText('FR-001 already exists')).toBeInTheDocument();
    const add = calls.find((c) => c.method === 'spec_add');
    expect(add?.args).toMatchObject({ type: 'Requirement', key: 'FR-001', fields: { text: 'The system shall wave.' } });
  });

  it('shows every type until a facet is checked, and sorts by a column', async () => {
    const fr = item('FR-001', 'The system shall greet.');
    const adr: SpecItem = { ...item('ADR-0001', 'Use SQLite'), id: 'dec:ADR-0001', type: 'Decision', status: 'accepted' };
    mockApi({
      spec_list: () => [fr, adr],
      spec_show: (a) => (a.ref === adr.id || a.ref === adr.key ? adr : fr),
      show: () => ({ node: {}, properties: {}, links: [], annotations: [] }),
    });
    const user = userEvent.setup();
    renderAt('/spec', routes);
    expect(await screen.findByTestId('item-FR-001')).toBeInTheDocument();
    expect(screen.getByTestId('item-ADR-0001')).toBeInTheDocument();
    const types = screen.getByRole('group', { name: 'Type' });
    await user.click(within(types).getByRole('checkbox', { name: 'Decision' }));
    await waitFor(() => expect(screen.queryByTestId('item-FR-001')).toBeNull());
    expect(screen.getByTestId('item-ADR-0001')).toBeInTheDocument();
    await user.click(within(types).getByRole('checkbox', { name: 'Decision' }));
    expect(await screen.findByTestId('item-FR-001')).toBeInTheDocument();
    // Sorted by key: ADR before FR; the other way after a second click.
    const order = () => screen.getAllByTestId(/^item-/).map((r) => r.dataset.testid);
    expect(order()).toEqual(['item-ADR-0001', 'item-FR-001']);
    await user.click(screen.getByRole('button', { name: 'Sort by Key' }));
    await waitFor(() => expect(order()).toEqual(['item-FR-001', 'item-ADR-0001']));
  });

  it('explains what each spec type captures, and the explanation can be folded away', async () => {
    mockApi({
      spec_list: () => [],
    });
    const user = userEvent.setup();
    renderAt('/spec', routes);
    const guide = await screen.findByRole('region', { name: 'Spec types' });
    expect(guide).toHaveTextContent('What the software must do');
    expect(guide).toHaveTextContent('recorded choice and its rationale');
    expect(guide).toHaveTextContent('How something is shaped');
    expect(guide).toHaveTextContent('rule the work must obey');
    expect(guide).toHaveTextContent('Code satisfies it');
    expect(guide).toHaveTextContent('Code realizes it');
    const toggle = screen.getByRole('button', { name: 'What these types capture' });
    expect(toggle).toHaveAttribute('aria-expanded', 'true');
    await user.click(toggle);
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
  });

  it('opens what a link points to, with its lines highlighted', async () => {
    const fr = item('FR-001', 'The system shall greet.');
    mockApi({
      spec_list: () => [fr],
      spec_show: () => fr,
      show: (a) =>
        a.ref === 'unit-1'
          ? {
              node: { id: 'unit-1', label: 'src/greet.rs::greet', type: 'CodeUnit' },
              properties: {},
              links: [],
              annotations: [],
              location: { path: 'src/greet.rs', startLine: 2, endLine: 3, state: 'attached', confidence: 1 },
            }
          : {
              node: {},
              properties: {},
              links: [{ relation: 'satisfied by', status: 'verified', other: { id: 'unit-1', label: 'src/greet.rs::greet', type: 'CodeUnit' } }],
              annotations: [],
            },
      file_text: () => ({ path: 'src/greet.rs', size: 40, lines: 4, text: 'use std;\nfn greet() {\n  hi()\n}\n' }),
    });
    const user = userEvent.setup();
    renderAt('/spec/FR-001', routes);
    await user.click(await screen.findByRole('button', { name: 'src/greet.rs::greet' }));
    const code = await screen.findByTestId('code-view');
    const highlighted = [...code.querySelectorAll('[data-highlighted]')].map((l) => l.getAttribute('data-line'));
    expect(highlighted).toEqual(['2', '3']);
    expect(code.textContent).toContain('fn greet()');
    expect(screen.getByText('src/greet.rs:2–3')).toBeInTheDocument();
  });
});
