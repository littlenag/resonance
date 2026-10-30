// Calls to the daemon (POST /api/call), as the person, with origin `ui` (MVP-043).

let clientId: number | null = null;

/** The diagnostics client this page is, once the event stream has said (MVP-044). */
export function setClientId(id: number | null) {
  clientId = id;
}

export class ApiError extends Error {
  status: number;
  exitCode: number;
  constructor(message: string, status: number, exitCode: number) {
    super(message);
    this.status = status;
    this.exitCode = exitCode;
  }

  /** The page is not signed in to this daemon (its cookie is missing or stale). */
  get unauthorized() {
    return this.status === 401;
  }
}

export async function call<T>(method: string, args: Record<string, unknown> = {}, opts: { cached?: boolean } = {}): Promise<T> {
  const headers: Record<string, string> = { 'Content-Type': 'application/json' };
  if (clientId !== null) headers['X-Resonance-Client'] = String(clientId);
  let res: Response;
  try {
    res = await fetch('/api/call', {
      method: 'POST',
      headers,
      credentials: 'same-origin',
      body: JSON.stringify({ method, args, cached: opts.cached ?? false }),
    });
  } catch {
    throw new ApiError('The daemon is not answering. If it stopped, run `resonance ui` to start it again.', 0, 1);
  }
  let body: { result?: T; error?: { message: string; exitCode: number } };
  try {
    body = await res.json();
  } catch {
    throw new ApiError(`Unexpected answer from the daemon (HTTP ${res.status}).`, res.status, 1);
  }
  if (!res.ok || body.error) {
    throw new ApiError(body.error?.message ?? `HTTP ${res.status}`, res.status, body.error?.exitCode ?? 1);
  }
  return body.result as T;
}

/** Download `data` as a JSON file. */
export function downloadJson(name: string, data: unknown) {
  const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = name;
  a.click();
  URL.revokeObjectURL(url);
}
