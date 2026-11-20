export interface Voice {
  id: string;
  model_name: string;
  index_name: string | null;
  version: number | null;
  origin?: { name: string };
}

export const voiceName = (voice: Voice) => voice.origin?.name ?? voice.model_name;

export async function responseJson<T>(response: Response): Promise<T> {
  if (response.ok) return response.json() as Promise<T>;
  const body = await response.json().catch(() => null);
  throw new Error(body?.error ?? `HTTP ${response.status}`);
}

