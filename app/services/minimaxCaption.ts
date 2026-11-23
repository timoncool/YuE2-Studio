import { useEffect, useState } from 'react';
import { apiUrl } from './apiBase';

/**
 * MiniMax Music3 Studio 4.0.2's structured caption: the model reads its description as a labelled document — Global
 * Metadata, Vocal Details, Arrangement — and the studio writes it in three panes joined into the one caption the
 * engine takes. Also its prompt check: what the engine would drop or refuse, asked once typing pauses.
 */

const SECTIONS = ['Global Metadata', 'Vocal Details', 'Arrangement'] as const;

/**
 * Splits a caption on its section headings. A caption that does not carry them
 * — a hand-written one, for instance — stays whole in the first field so
 * nothing is silently dropped.
 */
export function splitCaption(caption: string): { globalMetadata: string; vocalDetails: string; arrangement: string } {
  // a heading counts only at the start of a line, so the word inside a sentence stays text
  const found = SECTIONS.map((section, field) => {
    const match = new RegExp(`(^|\\n)${section}`).exec(caption);
    return { field, index: match ? match.index + match[1].length : -1 };
  })
    .filter(entry => entry.index >= 0)
    .sort((a, b) => a.index - b.index);
  if (found.length === 0) {
    return { globalMetadata: caption.trim(), vocalDetails: '', arrangement: '' };
  }
  const fields = ['', '', ''];
  found.forEach((entry, order) => {
    const end = order + 1 < found.length ? found[order + 1].index : caption.length;
    fields[entry.field] = caption.slice(entry.index + SECTIONS[entry.field].length, end).trim();
  });
  const lead = caption.slice(0, found[0].index).trim();
  if (lead) fields[0] = fields[0] ? `${lead}\n${fields[0]}` : lead;
  return { globalMetadata: fields[0], vocalDetails: fields[1], arrangement: fields[2] };
}

/** Rebuilds the single caption string the engine takes from the three panes. */
export function joinCaption(globalMetadata: string, vocalDetails: string, arrangement: string): string {
  return SECTIONS.map((heading, index) => {
    // a trigger's comma with nothing after it is not part of the caption
    const body = [globalMetadata, vocalDetails, arrangement][index].trim().replace(/,$/, '').trimEnd();
    return body ? `${heading}\n${body}` : '';
  })
    .filter(Boolean)
    .join('\n');
}

export interface PromptIssue {
  code: 'words_on_tag_line' | 'vocal_gender_missing' | 'prompt_too_long';
  severity: 'error' | 'warning';
  line?: number;
  text?: string;
}

export interface PromptCheck {
  tokens: number;
  /** False when the engine was not running and `tokens` is the service's estimate. */
  exact: boolean;
  limit: number;
  issues: PromptIssue[];
}

/** What the engine would lose or refuse in a caption and lyrics, asked once typing pauses. */
export function usePromptCheck(caption: string, lyrics: string, instrumental: boolean, enabled: boolean): { data?: PromptCheck; error: Error | null } {
  const [answer, setAnswer] = useState<{ data?: PromptCheck; error: Error | null }>({ error: null });
  useEffect(() => {
    if (!enabled) return undefined;
    let current = true;
    const timer = window.setTimeout(() => {
      void fetch(apiUrl('/v1/music/prompt-check'), {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ caption, lyrics, instrumental }),
      })
        .then(async response => {
          if (!response.ok) throw new Error(`/v1/music/prompt-check answered ${response.status}`);
          const data = (await response.json()) as PromptCheck;
          if (!Array.isArray(data?.issues)) throw new Error('/v1/music/prompt-check answered without its issues');
          if (current) setAnswer({ data, error: null });
        })
        .catch((reason: unknown) => { if (current) setAnswer(previous => ({ data: previous.data, error: reason instanceof Error ? reason : new Error(String(reason)) })); });
    }, 400);
    return () => { current = false; window.clearTimeout(timer); };
  }, [caption, lyrics, instrumental, enabled]);
  return answer;
}
