// Adding and editing spec items (FR-291, FR-292). Adding suggests the next free key and the
// daemon refuses one that exists (FR-273). Editing sends only what changed, and says so when
// the item changed underneath (another person, an agent or git) before saving.

import { useMemo, useState } from 'react';
import {
  Alert,
  Button,
  Group,
  Modal,
  Select,
  Stack,
  Tabs,
  Text,
  TextInput,
  Textarea,
  Typography,
} from '@mantine/core';
import { getHotkeyHandler } from '@mantine/hooks';
import { notifications } from '@mantine/notifications';
import { useQuery } from '@tanstack/react-query';
import Markdown from 'react-markdown';
import { call } from '../api';
import { FIELDS, choicesOf, label, valuesOf } from '../fields';
import type { SpecItem, SpecType } from '../types';
import { SPEC_TYPES } from '../types';
import { SPEC_TYPE_GUIDE } from '../specTypes';

export type FormMode = { mode: 'add'; type: SpecType } | { mode: 'edit'; item: SpecItem };

export function ItemForm({
  mode,
  onClose,
  onSaved,
}: {
  mode: FormMode;
  onClose: () => void;
  /** Called with the saved item's key (or ID). */
  onSaved: (ref: string | null) => void;
}) {
  const editing = mode.mode === 'edit' ? mode.item : null;
  const [type, setType] = useState<SpecType>(mode.mode === 'add' ? mode.type : mode.item.type);
  const [key, setKey] = useState('');
  const [keyTouched, setKeyTouched] = useState(false);
  const [values, setValues] = useState<Record<string, string>>(() =>
    editing ? valuesOf(editing.type, editing.props) : emptyValues(type),
  );
  // What the edit is based on; compared with the latest version of the item.
  const [base, setBase] = useState<Record<string, string>>(() => (editing ? valuesOf(editing.type, editing.props) : {}));
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const suggestion = useQuery({
    queryKey: ['suggest', type],
    queryFn: () => call<{ key: string }>('spec_suggest_key', { type }),
    enabled: mode.mode === 'add',
  });
  // The suggested key until the person types their own (FR-273).
  const shownKey = keyTouched ? key : (suggestion.data?.key ?? '');

  // The latest version of the item being edited; the event stream refreshes it.
  const latest = useQuery({
    queryKey: ['item', editing?.id ?? '', 'spec'],
    queryFn: () => call<SpecItem>('spec_show', { ref: editing!.id }),
    enabled: !!editing,
  });
  const newer = useMemo(
    () => (editing && latest.data ? valuesOf(editing.type, latest.data.props) : null),
    [editing, latest.data],
  );
  const conflict = !!newer && JSON.stringify(newer) !== JSON.stringify(base);

  const fields = FIELDS[type];
  const changed = editing
    ? Object.fromEntries(fields.filter((f) => values[f.name] !== base[f.name]).map((f) => [f.name, values[f.name]]))
    : Object.fromEntries(fields.filter((f) => values[f.name]?.trim()).map((f) => [f.name, values[f.name]]));

  const save = async () => {
    if (conflict) {
      setError('This item changed while you were editing. Choose which version to keep first.');
      return;
    }
    setError(null);
    if (editing && Object.keys(changed).length === 0) {
      notifications.show({ message: 'Nothing changed.' });
      onClose();
      return;
    }
    setSaving(true);
    try {
      const view = editing
        ? await call<SpecItem>('spec_edit', { ref: editing.id, fields: changed })
        : await call<SpecItem>('spec_add', { type, key: shownKey.trim() || undefined, fields: changed });
      notifications.show({
        color: 'green',
        message: editing ? `Saved ${editing.key ?? editing.title}.` : `Added ${view.key ?? view.title}.`,
      });
      onSaved(view.key ?? view.id);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setSaving(false);
    }
  };

  const title = editing ? `Edit ${editing.key ?? editing.title}` : `New ${type.toLowerCase()}`;
  return (
    <Modal opened onClose={onClose} title={title} size="xl" onKeyDown={getHotkeyHandler([['mod+S', () => void save()], ['F2', () => void save()]])}>
      <Stack gap="sm">
        {editing?.source && (
          <Alert color="blue" variant="light">
            This item came from <span className="mono">{`${editing.source.path}:${editing.source.line}`}</span>. Saving
            marks that document for your agent to bring back in step.
          </Alert>
        )}
        {conflict && newer && (
          <Alert color="orange" title="This item changed while you were editing">
            <Text size="sm">
              Changed: {fields.filter((f) => newer[f.name] !== base[f.name]).map((f) => f.name).join(', ')}.
            </Text>
            <Group mt="xs" gap="xs">
              <Button
                size="xs"
                variant="light"
                onClick={() => {
                  setValues(newer);
                  setBase(newer);
                }}
              >
                Take the newer version
              </Button>
              <Button size="xs" variant="light" color="orange" onClick={() => setBase(newer)}>
                Keep my edits
              </Button>
            </Group>
          </Alert>
        )}
        {!editing && (
          <Group grow>
            <Select
              label="Type"
              data={SPEC_TYPES}
              value={type}
              allowDeselect={false}
              onChange={(t) => {
                if (!t) return;
                setType(t as SpecType);
                setValues(emptyValues(t as SpecType));
                setKeyTouched(false);
              }}
            />
            <TextInput
              label="Key"
              description="Suggested: the next free key. Keys are unique."
              value={shownKey}
              onChange={(e) => {
                setKey(e.currentTarget.value.replace(/\s/g, ''));
                setKeyTouched(true);
              }}
              data-autofocus
            />
          </Group>
        )}
        <Text size="sm" c="dimmed">
          {SPEC_TYPE_GUIDE[type].captures} {SPEC_TYPE_GUIDE[type].graph}
        </Text>
        {fields.map((f) =>
          f.long ? (
            <LongField
              key={f.name}
              name={f.name}
              value={values[f.name] ?? ''}
              onChange={(v) => setValues((s) => ({ ...s, [f.name]: v }))}
            />
          ) : choicesOf(type, f.name) ? (
            <Select
              key={f.name}
              label={label(f.name)}
              data={options(choicesOf(type, f.name) ?? [], values[f.name] ?? '')}
              value={values[f.name] || null}
              placeholder={editing ? undefined : 'Default for the type'}
              allowDeselect={!editing}
              clearable={!editing}
              onChange={(v) => setValues((s) => ({ ...s, [f.name]: v ?? '' }))}
            />
          ) : (
            <TextInput
              key={f.name}
              label={label(f.name)}
              value={values[f.name] ?? ''}
              onChange={(e) => {
                const v = e.currentTarget.value;
                setValues((s) => ({ ...s, [f.name]: v }));
              }}
            />
          ),
        )}
        {error && (
          <Alert color="red" title="Not saved">
            {error}
          </Alert>
        )}
        <Group justify="space-between">
          <Text size="xs" c="dimmed">
            ⌘S / ctrl+S saves, esc cancels. Your change goes to the change log for your agent.
          </Text>
          <Group gap="xs">
            <Button variant="default" onClick={onClose}>
              Cancel
            </Button>
            <Button onClick={() => void save()} loading={saving}>
              {editing ? 'Save' : 'Add'}
            </Button>
          </Group>
        </Group>
      </Stack>
    </Modal>
  );
}

/** The field's values, and the current one if it is not among them, so that it shows. */
function options(allowed: string[], current: string) {
  const all = current && !allowed.includes(current) ? [...allowed, current] : allowed;
  return all.map((v) => ({ value: v, label: label(v) }));
}

function emptyValues(type: SpecType): Record<string, string> {
  return Object.fromEntries(FIELDS[type].map((f) => [f.name, '']));
}

function LongField({ name, value, onChange }: { name: string; value: string; onChange: (v: string) => void }) {
  return (
    <div>
      <Text size="sm" fw={500} mb={4}>
        {label(name)}
      </Text>
      <Tabs defaultValue="write" variant="outline">
        <Tabs.List>
          <Tabs.Tab value="write">Write</Tabs.Tab>
          <Tabs.Tab value="preview">Preview</Tabs.Tab>
        </Tabs.List>
        <Tabs.Panel value="write" pt="xs">
          <Textarea
            aria-label={label(name)}
            autosize
            minRows={3}
            maxRows={16}
            value={value}
            onChange={(e) => onChange(e.currentTarget.value)}
          />
        </Tabs.Panel>
        <Tabs.Panel value="preview" pt="xs">
          <Typography className="markdown" fz="sm">
            {value ? <Markdown>{value}</Markdown> : <Text c="dimmed">Nothing to preview.</Text>}
          </Typography>
        </Tabs.Panel>
      </Tabs>
    </div>
  );
}
