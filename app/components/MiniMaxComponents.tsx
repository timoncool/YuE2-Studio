import React, { useEffect, useState } from 'react';
import { useI18n } from '../context/I18nContext';
import { minimaxCaptionStrings } from '../i18n/minimaxCaption';
import { apiUrl } from '../services/apiBase';
import { CONTROL } from './AceStudioFields';
import { Select } from './kit/Select';

/**
 * MiniMax Music3 Studio 4.0.2's components for one request: the engine loads any GGUF of its model folder for each
 * of its five roles, so a song can run with its own LM, depth, condition, DiT and VAE files in place of the set's.
 * All five or none: the engine refuses a partial set.
 */

export const MINIMAX_ROLES = [['lm', 'LM'], ['depth', 'Depth'], ['cond', 'Cond'], ['dit', 'DiT'], ['vae', 'VAE']] as const;
type Files = { file: string; installed: boolean }[];

/** The components to send: all five roles, or nothing. */
export const completeComponents = (chosen: Record<string, string>): Record<string, string> | undefined =>
  MINIMAX_ROLES.every(([role]) => chosen[role]) ? chosen : undefined;

const shortName = (file: string) => file.replace(/^MiniMax-Music3-/, '').replace(/\.gguf$/, '');

export function MiniMaxComponents({ value, onChange, setFiles }: {
  value: Record<string, string>;
  onChange: (value: Record<string, string>) => void;
  /** The chosen set's file for each role. */
  setFiles?: Record<string, string>;
}) {
  const { language } = useI18n();
  const text = minimaxCaptionStrings[language];
  const [roles, setRoles] = useState<Record<string, Files> | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void fetch(apiUrl('/v1/minimax/components'))
      .then(async response => {
        const body = await response.json().catch(() => null) as { roles?: Record<string, Files>; error?: string } | null;
        if (!response.ok || !body?.roles) throw new Error(body?.error || String(response.status));
        setRoles(body.roles);
      })
      .catch((reason: unknown) => setError(reason instanceof Error ? reason.message : String(reason)));
  }, []);

  const partial = Object.keys(value).length > 0 && !completeComponents(value);
  return (
    <div>
      <p className="mb-2 text-[11px] leading-4 text-zinc-500">{text.componentOverrideHint}</p>
      <div className="space-y-2">
        {MINIMAX_ROLES.map(([role, label]) => (
          <div key={role} className="grid grid-cols-[64px_1fr] items-center gap-2">
            <span className="text-[11px] font-semibold text-zinc-500 dark:text-zinc-400">{label}</span>
            <Select<string> value={value[role] ?? ''} label={label} className="min-w-0" inputClassName={CONTROL}
              onChange={file => {
                const next = { ...value };
                if (file) next[role] = file;
                else delete next[role];
                onChange(next);
              }}
              options={[
                { value: '', label: setFiles?.[role] ? `${text.profileDefault} · ${shortName(setFiles[role])}` : text.profileDefault },
                ...(roles?.[role] ?? []).filter(entry => entry.installed).map(entry => ({ value: entry.file, label: shortName(entry.file) })),
              ]} />
          </div>
        ))}
      </div>
      {partial && <p role="alert" className="mt-2 text-[11px] text-amber-600 dark:text-amber-300">{text.componentOverridePartial}</p>}
      {error && <p role="alert" className="mt-2 text-[11px] text-rose-600 dark:text-rose-300">{error}</p>}
    </div>
  );
}
