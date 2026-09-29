import React, { useEffect, useState } from 'react';
import { Check, Copy, Trash2 } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import type { TranslationKey } from '../i18n/translations';
import { apiUrl } from '../services/apiBase';

/**
 * The studio's MCP server, for the user connecting an agent to it: whether
 * the window and an agent are connected, the address, and what to paste into
 * Claude Code or any other client. The server runs with the studio itself.
 */

const SERVER_NAME = 'yue2-studio';

interface McpStatus {
  window_open: boolean;
  agent_connected: boolean;
  agent_last_call: string | null;
  agent_seconds_ago: number | null;
  requests_waiting: number;
}


/** free, risky or all - how close the agent is kept. */
const LEASHES: Array<{ value: string; label: TranslationKey }> = [
  { value: 'risky', label: 'agentLeashRisky' },
  { value: 'free', label: 'agentLeashFree' },
  { value: 'all', label: 'agentLeashAll' },
];

function endpoint(): string {
  const url = apiUrl('/mcp');
  return url.startsWith('/') ? `${window.location.origin}${url}` : url;
}

const CopyField: React.FC<{ label: string; value: string; multiline?: boolean }> = ({ label, value, multiline }) => {
  const { t } = useI18n();
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    await navigator.clipboard.writeText(value);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  };
  return (
    <div className="space-y-1">
      <div className="flex items-center justify-between gap-2">
        <span className="text-xs font-medium text-zinc-500 dark:text-zinc-400">{label}</span>
        <button
          type="button"
          onClick={() => void copy().catch((error: Error) => console.error('[ERROR] copy failed:', error))}
          className="inline-flex items-center gap-1 rounded-md border border-zinc-200 px-2 py-1 text-[11px] font-medium text-zinc-600 transition-colors hover:border-pink-400 hover:text-pink-600 dark:border-white/10 dark:text-zinc-300"
        >
          {copied ? <Check size={12} /> : <Copy size={12} />}
          {copied ? t('agentCopied') : t('agentCopy')}
        </button>
      </div>
      <pre className={`overflow-x-auto rounded-lg border border-zinc-200 bg-zinc-50 p-2 font-mono text-[11px] text-zinc-800 dark:border-white/10 dark:bg-black/30 dark:text-zinc-200 ${multiline ? 'whitespace-pre' : 'whitespace-nowrap'}`}>{value}</pre>
    </div>
  );
};

export const AgentPanel: React.FC = () => {
  const { t } = useI18n();
  const [status, setStatus] = useState<McpStatus | null>(null);
  const [failed, setFailed] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    const load = () =>
      fetch(apiUrl('/mcp/status'))
        .then(async (response) => {
          if (!response.ok) throw new Error(`HTTP ${response.status}`);
          return response.json() as Promise<McpStatus>;
        })
        .then((body) => {
          if (!alive) return;
          setStatus(body);
          setFailed(null);
        })
        .catch((error: Error) => alive && setFailed(error.message));
    void load();
    const timer = window.setInterval(() => void load(), 3000);
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, []);

  const [leash, setLeash] = useState<string>('risky');
  const [trackAllow, setTrackAllow] = useState(false);
  const [forbidden, setForbidden] = useState<string[]>([]);

  // What the person decided about the agent: read when the page opens, so the panel
  // always tells the truth about what the agent may do without asking.
  useEffect(() => {
    let alive = true;
    const load = () =>
      Promise.all([
        fetch(apiUrl('/v1/agent/leash')),
        fetch(apiUrl('/v1/agent/allowance')),
        fetch(apiUrl('/v1/agent/forbidden')),
      ])
        .then(async ([leashResponse, allowanceResponse, forbiddenResponse]) => {
          if (!leashResponse.ok || !allowanceResponse.ok || !forbiddenResponse.ok) return;
          const leashBody = (await leashResponse.json()) as { leash?: string };
          const allowanceBody = (await allowanceResponse.json()) as { allow?: boolean };
          const forbiddenBody = (await forbiddenResponse.json()) as { forbidden?: string[] };
          if (!alive) return;
          setLeash(leashBody.leash ?? 'risky');
          setTrackAllow(Boolean(allowanceBody.allow));
          setForbidden(Array.isArray(forbiddenBody.forbidden) ? forbiddenBody.forbidden : []);
        })
        .catch((error: Error) => console.error('[ERROR] agent settings failed:', error));
    void load();
    const timer = window.setInterval(() => void load(), 3000);
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, []);

  // The leash is the user's, not the agent's: only this panel sets it.
  const chooseLeash = (value: string) => {
    setLeash(value);
    void fetch(apiUrl('/v1/agent/leash'), {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ leash: value }),
    }).catch((error: Error) => console.error('[ERROR] leash failed:', error));
  };

  // The one switch for tracks, and the list nothing may talk the studio out of.
  const allowTracks = (allow: boolean) => {
    setTrackAllow(allow);
    void fetch(apiUrl('/v1/agent/allowance'), {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ allow }),
    }).catch((error: Error) => console.error('[ERROR] allowance failed:', error));
  };

  const forgetForbidden = (name: string) => {
    const left = forbidden.filter((tool) => tool !== name);
    setForbidden(left);
    void fetch(apiUrl('/v1/agent/forbidden'), {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ forbidden: left }),
    }).catch((error: Error) => console.error('[ERROR] forgetting failed:', error));
  };

  const url = endpoint();
  const config = JSON.stringify({ mcpServers: { [SERVER_NAME]: { type: 'streamable-http', url } } }, null, 2);
  const dot = (on: boolean) => <span className={`h-2 w-2 shrink-0 rounded-full ${on ? 'bg-emerald-500' : 'bg-zinc-400'}`} />;

  return (
    <div className="max-w-2xl space-y-4">
      <p className="text-sm leading-6 text-zinc-600 dark:text-zinc-300">{t('agentIntro')}</p>

      <div className="space-y-1.5 rounded-xl border border-zinc-200 p-3 text-xs dark:border-white/10">
        <div className="flex items-center gap-2 text-zinc-700 dark:text-zinc-200">
          {dot(Boolean(status?.agent_connected))}
          {status?.agent_last_call
            ? `${t('agentSeen')}: ${status.agent_last_call} · ${status.agent_seconds_ago ?? 0} ${t('agentSecondsAgo')}`
            : t('agentNone')}
        </div>
        <div className="flex items-center gap-2 text-zinc-700 dark:text-zinc-200">
          {dot(Boolean(status?.window_open))}
          {status?.window_open ? t('agentWindowOn') : t('agentWindowOff')}
        </div>
        {Boolean(status?.requests_waiting) && (
          <div className="flex items-center gap-2 text-pink-600 dark:text-pink-300">
            {dot(true)}
            {t('agentRequests')}: {status?.requests_waiting}
          </div>
        )}
        {failed && <div className="text-red-600 dark:text-red-300">{failed}</div>}
      </div>

      <CopyField label={t('agentAddress')} value={url} />
      <CopyField label="Claude Code" value={`claude mcp add --transport http ${SERVER_NAME} ${url}`} />
      <CopyField label={t('agentConfig')} value={config} multiline />
      <p className="text-xs leading-5 text-zinc-500 dark:text-zinc-400">{t('agentAssistantHint')}</p>

      {/* How close the agent is kept: the person decides, and the agent asks. */}
      <div className="space-y-2 rounded-xl border border-zinc-200 p-3 dark:border-white/10">
        <div className="text-xs font-medium text-zinc-500 dark:text-zinc-400">{t('agentLeashTitle')}</div>
        <div className="flex flex-wrap gap-1">
          {LEASHES.map((option) => (
            <button
              key={option.value}
              type="button"
              onClick={() => chooseLeash(option.value)}
              className={`rounded-md border px-2 py-1 text-[11px] font-medium transition-colors ${leash === option.value ? 'border-pink-400 text-pink-600 dark:text-pink-300' : 'border-zinc-200 text-zinc-600 hover:border-pink-400 dark:border-white/10 dark:text-zinc-300'}`}
            >
              {t(option.label)}
            </button>
          ))}
        </div>
      </div>

      {/* The one switch for tracks: on, the agent makes them without asking. */}
      <div className="space-y-2 rounded-xl border border-zinc-200 p-3 dark:border-white/10">
        <div className="flex items-center gap-2">
          <input
            id="agent-track-allow"
            type="checkbox"
            checked={trackAllow}
            onChange={(event) => allowTracks(event.target.checked)}
            className="h-3.5 w-3.5 shrink-0 accent-pink-500"
          />
          <label htmlFor="agent-track-allow" className="text-xs text-zinc-700 dark:text-zinc-200">{t('agentAllowanceTitle')}</label>
        </div>
      </div>

      {/* Nothing here may be talked out of, whatever the agent asks. */}
      <div className="space-y-1.5 rounded-xl border border-zinc-200 p-3 dark:border-white/10">
        <div className="text-xs font-medium text-zinc-500 dark:text-zinc-400">{t('agentForbiddenTitle')}</div>
        {forbidden.length === 0 && (
          <p className="text-xs leading-5 text-zinc-500 dark:text-zinc-400">{t('agentForbiddenEmpty')}</p>
        )}
        {forbidden.map((tool) => (
          <div key={tool} className="flex items-center gap-2 text-xs text-zinc-700 dark:text-zinc-200">
            <span className="min-w-0 truncate font-mono">{tool}</span>
            <button
              type="button"
              onClick={() => forgetForbidden(tool)}
              title={t('agentPermissionsForget')}
              className="ml-auto shrink-0 rounded-md p-1 text-zinc-400 transition-colors hover:bg-zinc-100 hover:text-rose-600 dark:hover:bg-white/10 dark:hover:text-rose-400"
            >
              <Trash2 size={13} />
            </button>
          </div>
        ))}
      </div>
    </div>
  );
};
