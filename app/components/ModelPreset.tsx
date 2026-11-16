import React, { useState } from 'react';
import { CheckCircle2, Loader2, Wand2 } from 'lucide-react';
import { useI18n } from '../context/I18nContext';

/** Settings - Models: Ruach Studio's fine-tune and decoder, set up in one step. `inUse` is what the service says of the preset. */
export const ModelPreset: React.FC<{ inUse: boolean; disabled: boolean; onDone: () => void }> = ({ inUse, disabled, onDone }) => {
  const { t } = useI18n();
  const tt = t as unknown as (key: string) => string;
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const use = async () => {
    if (starting) return;
    setStarting(true);
    setError(null);
    try {
      const response = await fetch('/setup/preset', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ id: 'ruach' }),
      });
      if (!response.ok) {
        const body = await response.json().catch(() => null) as { error?: string } | null;
        setError(body?.error || `${response.status} ${response.statusText}`);
        return;
      }
      onDone();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setStarting(false);
    }
  };

  return (
    <div className="mt-4 rounded-lg border border-zinc-200 p-3 dark:border-white/10">
      <p className="text-[11px] font-semibold uppercase tracking-wide text-zinc-500 dark:text-zinc-400">{tt('presetRuachTitle')}</p>
      <p className="mt-1 text-[11px] leading-5 text-zinc-500 dark:text-zinc-400">{tt('presetRuachHint')}</p>
      <div className="mt-2 flex flex-wrap items-center gap-2">
        {inUse ? (
          <span className="inline-flex items-center gap-1.5 rounded-lg bg-emerald-500/10 px-3 py-2 text-xs font-semibold text-emerald-600 dark:text-emerald-300">
            <CheckCircle2 size={14} />{tt('presetRuachOn')}
          </span>
        ) : (
          <button
            type="button"
            onClick={() => void use()}
            disabled={disabled || starting}
            className="inline-flex items-center gap-1.5 rounded-lg bg-pink-600 px-3 py-2 text-xs font-semibold text-white hover:bg-pink-500 disabled:opacity-50"
          >
            {starting ? <Loader2 size={14} className="animate-spin" /> : <Wand2 size={14} />}{tt('presetRuachUse')}
          </button>
        )}
        <span className="text-[11px] text-zinc-500 dark:text-zinc-400">{tt('presetRuachBack')}</span>
      </div>
      {error && <p className="mt-2 text-xs text-rose-600 dark:text-rose-300">{error}</p>}
    </div>
  );
};
