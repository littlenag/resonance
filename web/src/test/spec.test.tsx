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
  priority: null,
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
      spec_show: (a) => (a.ref === two.id ? two : one),
      show: () => ({ node: {}, properties: {}, links: [{ relation: 'satisfies', status: 'stale', other: { label: 'src/greet.rs::greet' } }], annotations: [] }),
    });
    renderAt('/spec/FR-002', routes);
    expect(await screen.findByTestId('item-FR-001')).toBeInTheDocument();
    expect(within(screen.getByTestId('item-FR-001')).getByLabelText('pending change')).toBeInTheDocument();
    expect(within(screen.getByTestId('item-FR-002')).queryByLabelText('pending change')).toBeNull();
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
});
