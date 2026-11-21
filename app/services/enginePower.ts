/** Pausing the owned yue-server releases its GPU allocations while
 * YuE2's library and ordinary audio player continue running. */
export interface EnginePowerStatus {
  paused: boolean;
  running: boolean;
}

async function unpack(response: Response, action: string): Promise<EnginePowerStatus> {
  if (!response.ok) {
    const body: { error?: string } | null = await response.json().catch(() => null);
    throw new Error(body?.error || action + ' failed (' + response.status + ')');
  }
  return response.json() as Promise<EnginePowerStatus>;
}

export async function readEnginePower(): Promise<EnginePowerStatus> {
  return unpack(await fetch('/engine/power', { headers: { 'x-hub-skip': '1' } }), 'Reading engine state');
}

export async function changeEnginePower(running: boolean): Promise<EnginePowerStatus> {
  return unpack(await fetch('/engine/power', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', 'x-hub-skip': '1' },
    body: JSON.stringify({ running }),
  }), running ? 'Starting the engine' : 'Stopping the engine');
}
