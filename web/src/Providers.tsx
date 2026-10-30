import type { ReactNode } from 'react';
import { MantineProvider, createTheme } from '@mantine/core';
import { ModalsProvider } from '@mantine/modals';
import { Notifications } from '@mantine/notifications';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';

const theme = createTheme({
  primaryColor: 'indigo',
  fontFamilyMonospace: 'ui-monospace, SFMono-Regular, Menlo, Consolas, monospace',
  defaultRadius: 'sm',
});

export function makeQueryClient() {
  return new QueryClient({
    defaultOptions: {
      // The event stream says when something changed; no refetching on focus.
      queries: { retry: 1, refetchOnWindowFocus: false, staleTime: 30_000 },
    },
  });
}

export function Providers({ children, client }: { children: ReactNode; client?: QueryClient }) {
  return (
    <MantineProvider theme={theme} defaultColorScheme="auto">
      <QueryClientProvider client={client ?? defaultClient}>
        <ModalsProvider>
          <Notifications position="bottom-right" />
          {children}
        </ModalsProvider>
      </QueryClientProvider>
    </MantineProvider>
  );
}

const defaultClient = makeQueryClient();
