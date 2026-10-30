import { describe, expect, it } from 'vitest';
import { screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { ItemForm } from '../components/ItemForm';
import type { SpecItem } from '../types';
import { mockApi } from './api';
import { renderAt } from './render';

const base: SpecItem = {
  id: 'req:FR-001',
  type: 'Requirement',
  key: 'FR-001',
  title: 'Greet',
  text: 'Greet',
  status: 'active',
  kind: 'functional',
  priority: null,
  source: null,
  props: { text: 'Greet', kind: 'functional', status: 'active' },
  links: 0,
  staleLinks: 0,
};

describe('editing', () => {
  it('sends only changed fields', async () => {
    const calls = mockApi({ spec_show: () => base, spec_edit: () => base });
    const user = userEvent.setup();
    let saved: string | null = null;
    renderAt('/', [{ path: '/', element: <ItemForm mode={{ mode: 'edit', item: base }} onClose={() => {}} onSaved={(r) => (saved = r)} /> }]);
    const kind = await screen.findByRole('textbox', { name: 'Kind' });
    await user.clear(kind);
    await user.type(kind, 'quality');
    await user.click(screen.getByRole('button', { name: 'Save' }));
    await screen.findByText(/Saved FR-001/);
    expect(calls.find((c) => c.method === 'spec_edit')?.args).toEqual({ ref: 'req:FR-001', fields: { kind: 'quality' } });
    expect(saved).toBe('FR-001');
  });

  it('says when the item changed underneath, and can take the newer version', async () => {
    const newer = { ...base, props: { ...base.props, text: 'Greet warmly' } };
    const calls = mockApi({ spec_show: () => newer, spec_edit: () => newer });
    const user = userEvent.setup();
    renderAt('/', [{ path: '/', element: <ItemForm mode={{ mode: 'edit', item: base }} onClose={() => {}} onSaved={() => {}} /> }]);
    expect(await screen.findByText('This item changed while you were editing')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Save' }));
    expect(await screen.findByText(/Choose which version to keep/)).toBeInTheDocument();
    expect(calls.some((c) => c.method === 'spec_edit')).toBe(false);
    await user.click(screen.getByRole('button', { name: 'Take the newer version' }));
    expect(screen.queryByText('This item changed while you were editing')).toBeNull();
    expect(screen.getByRole('textbox', { name: 'Text' })).toHaveValue('Greet warmly');
  });
});
