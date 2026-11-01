// The spec page's explanation of the four spec types. Open by default, and the
// person's last choice is kept for this browser tab.

import { useState } from 'react';
import { Collapse, Group, Paper, SimpleGrid, Stack, Text, UnstyledButton } from '@mantine/core';
import { IconChevronDown, IconChevronRight } from '@tabler/icons-react';
import { SPEC_TYPE_GUIDE } from '../specTypes';
import { SPEC_TYPES, type SpecType } from '../types';

const STORAGE_KEY = 'resonance.specTypesGuide';

function readOpen(): boolean {
  try {
    return sessionStorage.getItem(STORAGE_KEY) !== 'closed';
  } catch {
    return true;
  }
}

export function SpecTypesGuide() {
  const [open, setOpen] = useState(readOpen);
  const toggle = () => {
    const next = !open;
    setOpen(next);
    try {
      sessionStorage.setItem(STORAGE_KEY, next ? 'open' : 'closed');
    } catch {
      // A private-mode browser may refuse storage. The panel still toggles.
    }
  };

  return (
    <Paper
      withBorder
      p="sm"
      component="section"
      aria-label="Spec types"
      // On a short window the four cards are taller than the page. Cap the guide and
      // scroll it, so the item list underneath keeps the rest of the height.
      style={{ flex: '0 1 auto', minHeight: 0, maxHeight: '50%', overflow: 'auto' }}
    >
      <UnstyledButton onClick={toggle} aria-expanded={open} aria-controls="spec-types-body">
        <Group gap={4}>
          {open ? <IconChevronDown size={16} /> : <IconChevronRight size={16} />}
          <Text size="sm" fw={600}>
            What these types capture
          </Text>
        </Group>
      </UnstyledButton>
      <Collapse expanded={open} id="spec-types-body">
        <Text size="sm" mt="xs">
          Requirements, decisions, designs and constraints are the spec. Each one records a different kind of
          statement and keeps a permanent key. Retiring one keeps the item and its links. A newer item of the same
          kind can supersede an older one, and links to the older one become stale so they can be checked against
          the new one.
        </Text>
        <SimpleGrid cols={{ base: 1, sm: 2 }} mt="sm" spacing="sm">
          {SPEC_TYPES.map((type) => (
            <TypeCard key={type} type={type} />
          ))}
        </SimpleGrid>
      </Collapse>
    </Paper>
  );
}

function TypeCard({ type }: { type: SpecType }) {
  const doc = SPEC_TYPE_GUIDE[type];
  return (
    <Stack gap={2}>
      <Text size="sm" fw={600}>
        {type}
      </Text>
      <Text size="sm">{doc.captures}</Text>
      <Text size="xs" c="dimmed">
        Keys: {doc.keys}
      </Text>
      <Text size="xs" c="dimmed">
        Fields: {doc.fields}
      </Text>
      <Text size="xs" c="dimmed">
        {doc.graph}
      </Text>
    </Stack>
  );
}
