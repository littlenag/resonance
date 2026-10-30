import { vi } from 'vitest';

export interface Recorded {
  method: string;
  args: Record<string, unknown>;
}

/** Answer `/api/call` from `handlers`; a handler that throws answers with an error. */
export function mockApi(handlers: Record<string, (args: Record<string, unknown>) => unknown>) {
  const calls: Recorded[] = [];
  const fetchMock = vi.fn(async (_url: string, init?: RequestInit) => {
    const body = JSON.parse(String(init?.body ?? '{}')) as { method: string; args: Record<string, unknown> };
    calls.push({ method: body.method, args: body.args });
    const h = handlers[body.method];
    if (!h) return new Response(JSON.stringify({ error: { message: `no handler for ${body.method}`, exitCode: 1 } }), { status: 403 });
    try {
      return new Response(JSON.stringify({ result: h(body.args) }), { status: 200 });
    } catch (e) {
      return new Response(JSON.stringify({ error: { message: (e as Error).message, exitCode: 1 } }), { status: 422 });
    }
  });
  vi.stubGlobal('fetch', fetchMock);
  return calls;
}
