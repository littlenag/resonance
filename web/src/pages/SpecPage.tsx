// Spec items (FR-291): the list with filters beside the selected item's detail; add, edit
// and retire. Every item has its own address, /spec/<key>.

import { useMemo, useRef, useState } from 'react';
import { Badge, Button, Grid, Group, Menu, ScrollArea, Select, Table, Text, TextInput, Tooltip } from '@mantine/core';
import { useHotkeys } from '@mantine/hooks';
import { modals } from '@mantine/modals';
import { notifications } from '@mantine/notifications';
import { IconChevronDown, IconPlus, IconSearch } from '@tabler/icons-react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useNavigate, useParams, useSearchParams } from 'react-router-dom';
import { call } from '../api';
import { STATUSES } from '../fields';
import type { SpecItem, SpecType } from '../types';
import { SPEC_TYPES } from '../types';
import { ItemDetail } from '../components/ItemDetail';
import { ItemForm, type FormMode } from '../components/ItemForm';

export function refOf(item: SpecItem) {
  return item.key ?? item.id;
}

export function SpecPage() {
  const { ref } = useParams();
  const navigate = useNavigate();
  const [search, setSearch] = useSearchParams();
  const queryClient = useQueryClient();
  const [text, setText] = useState('');
  const [type, setType] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [form, setForm] = useState<FormMode | null>(null);
  const filterRef = useRef<HTMLInputElement>(null);

  const list = useQuery({
    queryKey: ['spec', { text, type, status }],
    queryFn: () =>
      call<SpecItem[]>('spec_list', {
        type: type ?? undefined,
        status: status ?? undefined,
        text: text.trim() || undefined,
      }),
    placeholderData: (prev) => prev,
  });
  const items = useMemo(() => list.data ?? [], [list.data]);
  const decoded = ref ? decodeURIComponent(ref) : undefined;
  const index = decoded ? items.findIndex((i) => i.key === decoded || i.id === decoded) : -1;
  const selected = index >= 0 ? items[index] : undefined;
  const selectedRef = decoded ?? (items[0] ? refOf(items[0]) : undefined);

  // `/spec?add=Decision` (from the command palette) opens the add form.
  const addParam = search.get('add');
  const fromUrl: FormMode | null =
    addParam && (SPEC_TYPES as string[]).includes(addParam) ? { mode: 'add', type: addParam as SpecType } : null;
  const openForm = form ?? fromUrl;
  const closeForm = () => {
    setForm(null);
    if (addParam) {
      search.delete('add');
      setSearch(search, { replace: true });
    }
  };

  const select = (item: SpecItem | undefined) => {
    if (item) navigate(`/spec/${encodeURIComponent(refOf(item))}`);
  };
  const move = (delta: number) => {
    if (items.length === 0) return;
    const from = index >= 0 ? index : 0;
    select(items[Math.min(items.length - 1, Math.max(0, from + (index >= 0 ? delta : 0)))]);
  };

  const current = selected ?? (!decoded ? items[0] : undefined);

  const retire = (item: SpecItem) => {
    if (item.status === 'retired') {
      notifications.show({ message: 'Already retired.' });
      return;
    }
    const label = item.key ?? item.title;
    modals.openConfirmModal({
      title: `Retire ${label}?`,
      children: (
        <Text size="sm">
          Retiring never deletes: {label} stays in the graph as retired, and its {item.links} link(s) are kept, so
          that they can be reviewed. Your agent sees the change in the change log.
        </Text>
      ),
      labels: { confirm: 'Retire', cancel: 'Cancel' },
      confirmProps: { color: 'red' },
      onConfirm: async () => {
        try {
          await call('spec_retire', { ref: item.id });
          notifications.show({ color: 'green', message: `Retired ${label} (its ${item.links} link(s) were kept).` });
          await queryClient.invalidateQueries();
        } catch (e) {
          notifications.show({ color: 'red', title: 'Not retired', message: (e as Error).message });
        }
      },
    });
  };

  useHotkeys([
    ['/', (e) => {
      e.preventDefault();
      filterRef.current?.focus();
    }],
    ['j', () => move(1)],
    ['ArrowDown', () => move(1)],
    ['n', () => move(1)],
    ['k', () => move(-1)],
    ['ArrowUp', () => move(-1)],
    ['p', () => move(-1)],
    ['a', () => setForm({ mode: 'add', type: (type as SpecType | null) ?? 'Requirement' })],
    ['e', () => current && setForm({ mode: 'edit', item: current })],
    ['r', () => current && retire(current)],
  ]);

  return (
    <>
      <Group mb="sm" gap="sm" wrap="wrap">
        <TextInput
          ref={filterRef}
          placeholder="Filter by text (/)"
          leftSection={<IconSearch size={16} />}
          value={text}
          onChange={(e) => setText(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === 'Escape') e.currentTarget.blur();
          }}
          w={260}
          aria-label="Filter by text"
        />
        <Select
          placeholder="Any type"
          data={SPEC_TYPES}
          value={type}
          onChange={setType}
          clearable
          w={160}
          aria-label="Type"
        />
        <Select
          placeholder="Any status"
          data={STATUSES}
          value={status}
          onChange={setStatus}
          clearable
          w={160}
          aria-label="Status"
        />
        <Menu position="bottom-start">
          <Menu.Target>
            <Button leftSection={<IconPlus size={16} />} rightSection={<IconChevronDown size={14} />}>
              Add
            </Button>
          </Menu.Target>
          <Menu.Dropdown>
            {SPEC_TYPES.map((t) => (
              <Menu.Item key={t} onClick={() => setForm({ mode: 'add', type: t })}>
                {t}
              </Menu.Item>
            ))}
          </Menu.Dropdown>
        </Menu>
        <Text size="sm" c="dimmed">
          {list.isLoading ? 'Loading…' : `${items.length} item(s)`}
        </Text>
      </Group>
      {list.error && (
        <Text c="red" mb="sm">
          {(list.error as Error).message}
        </Text>
      )}
      <Grid gap="md">
        <Grid.Col span={{ base: 12, lg: 5 }}>
          <ScrollArea h="calc(100vh - 150px)" type="auto">
            <Table highlightOnHover stickyHeader verticalSpacing={4} fz="sm">
              <Table.Thead>
                <Table.Tr>
                  <Table.Th w={16} />
                  <Table.Th>Key</Table.Th>
                  <Table.Th>Status</Table.Th>
                  <Table.Th>Title</Table.Th>
                  <Table.Th>Links</Table.Th>
                </Table.Tr>
              </Table.Thead>
              <Table.Tbody>
                {items.map((item) => {
                  const isSelected = current?.id === item.id;
                  return (
                    <Table.Tr
                      key={item.id}
                      className={`row-clickable ${isSelected ? 'row-selected' : ''}`}
                      onClick={() => select(item)}
                      aria-selected={isSelected}
                      data-testid={`item-${refOf(item)}`}
                    >
                      <Table.Td>
                        {item.pendingChange && (
                          <Tooltip label={`Pending change #${item.pendingChange.seq} (${item.pendingChange.kind}): not handled by your agent yet`}>
                            <Text c="yellow.7" fw={700} aria-label="pending change">
                              •
                            </Text>
                          </Tooltip>
                        )}
                      </Table.Td>
                      <Table.Td className="mono" style={{ whiteSpace: 'nowrap' }}>
                        {item.key ?? '—'}
                      </Table.Td>
                      <Table.Td>
                        <StatusBadge status={item.status} />
                      </Table.Td>
                      <Table.Td>
                        <Text size="sm" lineClamp={1}>
                          {item.title}
                        </Text>
                        <Text size="xs" c="dimmed">
                          {item.type}
                        </Text>
                      </Table.Td>
                      <Table.Td style={{ whiteSpace: 'nowrap' }}>
                        {item.links}
                        {item.staleLinks > 0 && (
                          <Text span c="orange" size="xs">
                            {' '}
                            ({item.staleLinks} stale)
                          </Text>
                        )}
                      </Table.Td>
                    </Table.Tr>
                  );
                })}
              </Table.Tbody>
            </Table>
            {!list.isLoading && items.length === 0 && (
              <Text c="dimmed" p="md">
                No spec items{text || type || status ? ' match these filters' : ' yet: add one, or ask your agent to bring in your spec documents'}.
              </Text>
            )}
          </ScrollArea>
        </Grid.Col>
        <Grid.Col span={{ base: 12, lg: 7 }}>
          {selectedRef ? (
            <ItemDetail
              itemRef={current ? current.id : selectedRef}
              listItem={current}
              onEdit={(item) => setForm({ mode: 'edit', item })}
              onRetire={retire}
            />
          ) : (
            <Text c="dimmed">Select an item to see its details.</Text>
          )}
        </Grid.Col>
      </Grid>
      {openForm && (
        <ItemForm
          mode={openForm}
          onClose={closeForm}
          onSaved={(saved) => {
            closeForm();
            void queryClient.invalidateQueries().then(() => {
              if (saved) navigate(`/spec/${encodeURIComponent(saved)}`);
            });
          }}
        />
      )}
    </>
  );
}

const STATUS_COLORS: Record<string, string> = {
  active: 'green',
  accepted: 'green',
  draft: 'gray',
  proposed: 'blue',
  superseded: 'violet',
  deprecated: 'orange',
  retired: 'red',
};

export function StatusBadge({ status }: { status: string }) {
  return (
    <Badge size="sm" variant="light" color={STATUS_COLORS[status] ?? 'gray'}>
      {status}
    </Badge>
  );
}
