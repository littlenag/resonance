// One spec item (FR-291): every field, its links with their statuses and latest evidence,
// annotations, where it came from, and its pending change. A link opens what it points to
// in a drawer (FR-292).

import { useState } from 'react';
import { Alert, Anchor, Badge, Button, Card, Divider, Group, Loader, Stack, Table, Text, Title, Typography } from '@mantine/core';
import { IconAlertTriangle, IconArchive, IconFileDescription, IconPencil } from '@tabler/icons-react';
import { useQuery } from '@tanstack/react-query';
import Markdown from 'react-markdown';
import { call } from '../api';
import { FIELDS, fieldText, label } from '../fields';
import type { LinkView, ShowView, SpecItem } from '../types';
import { LinkDrawer } from './LinkDrawer';
import { StatusBadge } from '../pages/SpecPage';
import { SPEC_TYPE_GUIDE } from '../specTypes';

const SKIP = new Set(['source', 'alsoDefinedIn', 'key', 'specStatus', 'origin']);

const LINK_COLORS: Record<string, string> = {
  verified: 'green',
  asserted: 'blue',
  confirmed: 'teal',
  stale: 'orange',
  orphaned: 'red',
  failing: 'red',
};

export function ItemDetail({
  itemRef,
  listItem,
  onEdit,
  onRetire,
}: {
  itemRef: string;
  listItem?: SpecItem;
  onEdit: (item: SpecItem) => void;
  onRetire: (item: SpecItem) => void;
}) {
  const item = useQuery({
    queryKey: ['item', itemRef, 'spec'],
    queryFn: () => call<SpecItem>('spec_show', { ref: itemRef }),
  });
  const show = useQuery({
    queryKey: ['item', itemRef, 'show'],
    queryFn: () => call<ShowView>('show', { ref: itemRef }),
  });
  const [openLink, setOpenLink] = useState<LinkView | null>(null);
  const view = item.data ?? listItem;
  if (item.isLoading && !view) return <Loader size="sm" />;
  if (item.error && !view) return <Text c="red">{(item.error as Error).message}</Text>;
  if (!view) return null;

  const fields = FIELDS[view.type] ?? [];
  const named = new Set(fields.map((f) => f.name));
  const others = Object.entries(view.props).filter(([k, v]) => !named.has(k) && !SKIP.has(k) && v !== null && v !== '');
  const links = show.data?.links ?? [];
  const notes = show.data?.annotations ?? [];
  const pending = view.pendingChange;

  return (
    <Card withBorder padding="md">
      <Group justify="space-between" align="flex-start" wrap="nowrap">
        <div>
          <Group gap="xs">
            <Title order={3} className="mono" style={{ fontSize: 'var(--mantine-font-size-xl)' }}>
              {view.key ?? view.id}
            </Title>
            <Badge variant="outline">{view.type}</Badge>
            <StatusBadge status={view.status} />
          </Group>
          <Text fw={500} mt={4}>
            {view.title}
          </Text>
          {SPEC_TYPE_GUIDE[view.type] && (
            <Text size="sm" c="dimmed" mt={4}>
              {SPEC_TYPE_GUIDE[view.type].captures}
            </Text>
          )}
        </div>
        <Group gap="xs" wrap="nowrap">
          <Button size="xs" variant="light" leftSection={<IconPencil size={14} />} onClick={() => onEdit(view)}>
            Edit
          </Button>
          <Button
            size="xs"
            variant="light"
            color="red"
            leftSection={<IconArchive size={14} />}
            onClick={() => onRetire(view)}
            disabled={view.status === 'retired'}
          >
            Retire
          </Button>
        </Group>
      </Group>

      {pending && (
        <Alert color="yellow" mt="md" icon={<IconAlertTriangle size={18} />} title={`Pending change #${pending.seq}`}>
          {pending.kind}
          {pending.fields.length > 0 && `: ${pending.fields.join(', ')}`} ({pending.origin}). Your agent has not handled
          it yet.
        </Alert>
      )}
      {view.source && (
        <Alert color="blue" variant="light" mt="md" icon={<IconFileDescription size={18} />}>
          From <span className="mono">{`${view.source.path ?? '?'}:${view.source.line ?? '?'}`}</span>. Edits here
          mark the document for your agent to bring back in step.
        </Alert>
      )}

      <Stack gap="sm" mt="md">
        {fields.map((f) => {
          const value = fieldText(view.props[f.name]);
          return (
            <div key={f.name}>
              <Text size="xs" c="dimmed" fw={600} tt="uppercase">
                {label(f.name)}
              </Text>
              {f.long ? (
                value ? (
                  <Typography className="markdown" fz="sm">
                    <Markdown>{value}</Markdown>
                  </Typography>
                ) : (
                  <Text size="sm" c="dimmed">
                    —
                  </Text>
                )
              ) : (
                <Text size="sm">{value || '—'}</Text>
              )}
            </div>
          );
        })}
      </Stack>

      {others.length > 0 && (
        <>
          <Divider my="md" label="Other properties" labelPosition="left" />
          <Table fz="sm" verticalSpacing={2}>
            <Table.Tbody>
              {others.map(([k, v]) => (
                <Table.Tr key={k}>
                  <Table.Td w={160} c="dimmed">
                    {k}
                  </Table.Td>
                  <Table.Td>{fieldText(v)}</Table.Td>
                </Table.Tr>
              ))}
            </Table.Tbody>
          </Table>
        </>
      )}

      <Divider my="md" label={`Links (${links.length})`} labelPosition="left" />
      {show.isLoading ? (
        <Loader size="xs" />
      ) : links.length === 0 ? (
        <Text size="sm" c="dimmed">
          No links yet. Your agent links code and tests to this item as it implements it.
        </Text>
      ) : (
        <Table fz="sm" verticalSpacing={2}>
          <Table.Thead>
            <Table.Tr>
              <Table.Th>Relation</Table.Th>
              <Table.Th>To</Table.Th>
              <Table.Th>Status</Table.Th>
              <Table.Th>Latest test</Table.Th>
            </Table.Tr>
          </Table.Thead>
          <Table.Tbody>
            {links.map((l, i) => (
              <Table.Tr key={i}>
                <Table.Td>{l.relation}</Table.Td>
                <Table.Td className="mono">
                  {l.other?.id ? (
                    <Anchor component="button" className="mono" fz="sm" ta="left" onClick={() => setOpenLink(l)}>
                      {l.other.label ?? l.other.id}
                    </Anchor>
                  ) : (
                    (l.other?.label ?? '?')
                  )}
                </Table.Td>
                <Table.Td>
                  <Badge size="sm" variant="light" color={LINK_COLORS[l.status] ?? 'gray'}>
                    {l.status}
                  </Badge>
                </Table.Td>
                <Table.Td>
                  {l.latestEvidence ? `${l.latestEvidence.result ?? '?'} at ${l.latestEvidence.timestamp ?? '?'}` : '—'}
                </Table.Td>
              </Table.Tr>
            ))}
          </Table.Tbody>
        </Table>
      )}

      {notes.length > 0 && (
        <>
          <Divider my="md" label={`Annotations (${notes.length})`} labelPosition="left" />
          <Stack gap="xs">
            {notes.map((a, i) => (
              <Text size="sm" key={i}>
                <Badge size="xs" variant="outline" mr="xs">
                  {a.kind ?? 'note'}
                </Badge>
                {a.text} <Text span c="dimmed" size="xs">({a.author})</Text>
              </Text>
            ))}
          </Stack>
        </>
      )}
      {openLink && <LinkDrawer link={openLink} onClose={() => setOpenLink(null)} />}
    </Card>
  );
}
