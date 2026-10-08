import { parse as parseYaml, stringify as stringifyYaml } from 'yaml';
import type { YueRequest } from '../types';

/**
 * Prompt files in the engine's own request format, the one yue2.cpp's WebUI
 * saves and `yue-synth --request` reads: engine field names, sparse, plus the
 * optional `title` the WebUI uses to name the song.
 */

export type RequestFileFormat = 'json' | 'yaml';

const ENGINE_FIELDS = [
  'style', 'lyrics', 'abc', 'cot', 'duration', 'lm_seed', 'seed', 'steps',
  'lm_batch_size', 'synth_batch_size', 'cfg_scale', 'semantic_tokens',
  'abc_sampling', 'semantic_sampling', 'output_format', 'peak_clip', 'mp3_bitrate',
  'adapters', 'transpose', 'vocals_only', 'companion_scale',
] as const;

export function toEngineRequest(request: YueRequest): Record<string, unknown> {
  const source: Record<string, unknown> = { ...request, duration: request.duration_seconds };
  const out: Record<string, unknown> = {};
  if (request.title) out.title = request.title;
  for (const key of ENGINE_FIELDS) {
    const value = source[key];
    if (value === undefined || value === '') continue;
    if (typeof value === 'object' && value !== null && Object.keys(value).length === 0) continue;
    out[key] = value;
  }
  return out;
}

export function serializeRequest(request: YueRequest, format: RequestFileFormat): string {
  const engine = toEngineRequest(request);
  return format === 'json' ? `${JSON.stringify(engine, null, 2)}\n` : stringifyYaml(engine);
}

export const REQUEST_FILE_ACCEPT = '.json,.yml,.yaml,application/json,application/x-yaml';

/** Reads a JSON or YAML prompt file; the extension decides the parser. */
export function parseRequestFile(name: string, text: string): Record<string, unknown> {
  const yaml = /\.ya?ml$/i.test(name);
  const parsed: unknown = yaml ? parseYaml(text) : JSON.parse(text);
  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
    throw new Error('the file does not hold a request object');
  }
  return parsed as Record<string, unknown>;
}

export function requestFileTitle(name: string, request: Record<string, unknown>): string {
  if (typeof request.title === 'string' && request.title.trim()) return request.title.trim();
  return name.replace(/\.(json|ya?ml)$/i, '');
}
