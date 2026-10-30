// The change log (FR-285, FR-291): people's changes to spec items that the agent has not
// handled yet. Clearing removes entries from the log only, as for agents (FR-276).

import { useState } from 'react';
import { Badge, Button, Card, Group, Loader, Table, Text, Title } from '@mantine/core';
import { useHotkeys } from '@mantine/hooks';
import { modals } from '@mantine/modals';
import { notifications } from '@mantine/notifications';
import { IconTrash } from '@tabler/icons-react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Link } from 'react-router-dom';
import { call } from '../api';
import { fieldText } from '../fields';
import type { ChangeEntry } from '../types';

const KIND_COLORS: Record<string, string> = {
  created: 'green',
  added: 'green',
  edited: 'blue',
  retired: 'red',
  restored: 'teal',
  superseded: 'violet',
  status: 'grape',
};

export function ChangeLogPage() {
  const queryClient = useQueryClient();
  const log = useQuery({ queryKey: ['changes'], queryFn: () => call<ChangeEntry[]>('changes', {}) });
  const entries = log.data ?? [];
  const [cursor, setCursor] = useState(0);
  const [open, setOpen] = useState<number | null>(null);
  const selected = entries[Math.min(cursor, entries.length - 1)];

  const clear = async (seqs: number[]) => {
    try {
      await call('changes_done', { seqs, note: 'cleared in the web UI' });
      const n = seqs.length;
      notifications.show({
        color: 'green',
        message: `Cleared ${n} entr${n === 1 ? 'y' : 'ies'} from the change log; your agent will not see ${n === 1 ? 'it' : 'them'}.`,
      });
      await queryClient.invalidateQueries();
    } catch (e) {
      notifications.show({ color: 'red', title: 'Not cleared', message: (e as Error).message });
    }
  };

  const clearAll = () => {
    if (entries.length === 0) return;
    modals.openConfirmModal({
      title: `Clear all ${entries.length} entries?`,
      children: (
        <Text size="sm">
          Your agent will not see these changes. Clear them when you judge that nothing more is needed; the changes
          themselves stay in the graph and its history.
        </Text>
      ),
      labels: { confirm: 'Clear all', cancel: 'Cancel' },
      confirmProps: { color: 'red' },
      onConfirm: () => void clear(entries.map((e) => e.seq)),
    });
  };

  useHotkeys([
    ['j', () => setCursor((c) => Math.min(entries.length - 1, c + 1))],
    ['ArrowDown', () => setCursor((c) => Math.min(entries.length - 1, c + 1))],
    ['k', () => setCursor((c) => Math.max(0, c - 1))],
    ['ArrowUp', () => setCursor((c) => Math.max(0, c - 1))],
    ['Enter', () => selected && setOpen((o) => (o === selected.seq ? null : selected.seq))],
    ['d', () => selected && void clear([selected.seq])],
    ['shift+D', () => clearAll()],
  ]);

  return (
    <>
      <Group justify="space-between" mb="sm">
        <div>
          <Title order={3}>Change log</Title>
          <Text size="sm" c="dimmed">
            People's changes to spec items, and spec changes from git, that your agent has not handled yet.
          </Text>
        </div>
        <Button color="red" variant="light" leftSection={<IconTrash size={16} />} onClick={clearAll} disabled={entries.length === 0}>
          Clear all
        </Button>
      </Group>
      {log.isLoading && <Loader size="sm" />}
      {log.error && <Text c="red">{(log.error as Error).message}</Text>}
      {!log.isLoading && entries.length === 0 && (
        <Card withBorder>
          <Text c="dimmed">The change log is empty: your agent has handled everything.</Text>
        </Card>
      )}
      {entries.length > 0 && (
        <Table highlightOnHover fz="sm">
          <Table.Thead>
            <Table.Tr>
              <Table.Th>#</Table.Th>
              <Table.Th>When</Table.Th>
              <Table.Th>Item</Table.Th>
              <Table.Th>Change</Table.Th>
              <Table.Th>Summary</Table.Th>
              <Table.Th>Who</Table.Th>
              <Table.Th />
            </Table.Tr>
          </Table.Thead>
          <Table.Tbody>
            {entries.map((e, i) => (
              <EntryRows
                key={e.seq}
                entry={e}
                selected={i === cursor}
                open={open === e.seq}
                onToggle={() => {
                  setCursor(i);
                  setOpen((o) => (o === e.seq ? null : e.seq));
                }}
                onClear={() => void clear([e.seq])}
              />
            ))}
          </Table.Tbody>
        </Table>
      )}
    </>
  );
}

function EntryRows({
  entry,
  selected,
  open,
  onToggle,
  onClear,
}: {
  entry: ChangeEntry;
  selected: boolean;
  open: boolean;
  onToggle: () => void;
  onClear: () => void;
}) {
  const ref = entry.item.key ?? entry.item.id;
  return (
    <>
      <Table.Tr className={`row-clickable ${selected ? 'row-selected' : ''}`} onClick={onToggle}>
        <Table.Td className="mono">{entry.seq}</Table.Td>
        <Table.Td style={{ whiteSpace: 'nowrap' }}>{new Date(entry.at).toLocaleString()}</Table.Td>
        <Table.Td>
          <Link to={`/spec/${encodeURIComponent(ref)}`} onClick={(e) => e.stopPropagation()} className="mono">
            {entry.item.key ?? entry.item.id}
          </Link>{' '}
          <Text span size="xs" c="dimmed">
            {entry.item.title}
          </Text>
        </Table.Td>
        <Table.Td>
          <Badge size="sm" variant="light" color={KIND_COLORS[entry.kind] ?? 'gray'}>
            {entry.kind}
          </Badge>
          {entry.fields.length > 0 && (
            <Text span size="xs" c="dimmed" ml={4}>
              {entry.fields.join(', ')}
            </Text>
          )}
        </Table.Td>
        <Table.Td>{entry.summary}</Table.Td>
        <Table.Td>
          <Text size="xs">{entry.actor}</Text>
          <Badge size="xs" variant="outline" color="gray">
            {entry.origin}
          </Badge>
        </Table.Td>
        <Table.Td>
          <Button
            size="compact-xs"
            variant="subtle"
            color="red"
            onClick={(e) => {
              e.stopPropagation();
              onClear();
            }}
          >
            Clear
          </Button>
        </Table.Td>
      </Table.Tr>
      {open && (
        <Table.Tr>
          <Table.Td colSpan={7}>
            <EntryDiff seq={entry.seq} />
          </Table.Td>
        </Table.Tr>
      )}
    </>
  );
}

function EntryDiff({ seq }: { seq: number }) {
  const full = useQuery({ queryKey: ['changes', 'show', seq], queryFn: () => call<ChangeEntry>('changes_show', { seq }) });
  if (full.isLoading) return <Loader size="xs" />;
  if (full.error) return <Text c="red">{(full.error as Error).message}</Text>;
  const before = full.data?.before ?? {};
  const after = full.data?.after ?? {};
  const keys = [...new Set([...Object.keys(before), ...Object.keys(after)])].filter(
    (k) => fieldText(before[k]) !== fieldText(after[k]),
  );
  if (keys.length === 0) return <Text size="sm" c="dimmed">No field differences recorded.</Text>;
  return (
    <Table fz="xs" withColumnBorders>
      <Table.Thead>
        <Table.Tr>
          <Table.Th>Field</Table.Th>
          <Table.Th>Before</Table.Th>
          <Table.Th>After</Table.Th>
        </Table.Tr>
      </Table.Thead>
      <Table.Tbody>
        {keys.map((k) => (
          <Table.Tr key={k}>
            <Table.Td className="mono">{k}</Table.Td>
            <Table.Td style={{ whiteSpace: 'pre-wrap' }}>{fieldText(before[k]) || '—'}</Table.Td>
            <Table.Td style={{ whiteSpace: 'pre-wrap' }}>{fieldText(after[k]) || '—'}</Table.Td>
          </Table.Tr>
        ))}
      </Table.Tbody>
    </Table>
  );
}
