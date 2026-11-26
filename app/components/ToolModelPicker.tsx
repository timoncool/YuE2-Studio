import React from 'react';
import { Check } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { toolsPageStrings } from '../i18n/toolsPage';
import { formatSize } from '../services/fileSize';
import { FieldLabel } from './kit/FieldLabel';
import { ToolDevice, type ToolId } from './ToolDevice';

export type ToolModel<T extends string> = {
  id: T;
  name: string;
  /** What this model does differently from the others, in a few words. */
  hint?: string;
  bytes?: number;
  installed?: boolean;
};

/**
 * The first block of every tool: which model it runs and where it computes. One layout for all of them, so a tool with
 * one model today shows where a second one will appear, and the same choice looks the same in every card.
 */
export function ToolModelPicker<T extends string>({ tool, models, value, onChange, disabled, cardAvailable, onDeviceChange, showDevice = true, note }: {
  tool: ToolId;
  models: ToolModel<T>[];
  value: T | undefined;
  onChange?: (id: T) => void;
  disabled?: boolean;
  cardAvailable?: boolean;
  onDeviceChange?: () => void;
  /** False for a model that has one place to compute and says so itself (RVC's engines name their card). */
  showDevice?: boolean;
  /** A word about the choice for what is chosen to work on, under the models. */
  note?: React.ReactNode;
}) {
  const { t, language } = useI18n();
  const page = toolsPageStrings[language];
  const columns = models.length === 3 ? 'sm:grid-cols-3' : models.length >= 2 ? 'sm:grid-cols-2' : '';
  return (
    <div className="grid gap-3">
      <div role="radiogroup" aria-label={page.model} className="grid">
        <FieldLabel>{page.model}</FieldLabel>
        <div className={`grid gap-2 ${columns}`}>
          {models.map(model => {
            const on = model.id === value;
            const size = model.installed ? t('midiInstalled') : model.bytes ? formatSize(model.bytes, language) : '';
            return (
              <button key={model.id} type="button" role="radio" aria-checked={on} disabled={disabled}
                onClick={() => !on && onChange?.(model.id)}
                className={`rounded-lg border px-3 py-2.5 text-left disabled:cursor-not-allowed disabled:opacity-60 ${on ? 'border-pink-400 bg-pink-500/10' : 'border-zinc-200 hover:border-pink-300 dark:border-white/10'}`}>
                <span className="block text-sm font-semibold text-zinc-900 dark:text-white">{model.name}</span>
                {model.hint && <span className="block text-[13px] leading-[18px] text-zinc-600 dark:text-zinc-400">{model.hint}</span>}
                {size && (
                  <span className="mt-1 flex items-center gap-1 text-[12px] leading-4 text-zinc-500 dark:text-zinc-400">
                    {model.installed && <Check size={12} className="text-emerald-500" aria-hidden />}{size}
                  </span>
                )}
              </button>
            );
          })}
        </div>
      </div>
      {note}
      {showDevice && <ToolDevice tool={tool} disabled={disabled} cardAvailable={cardAvailable} onChange={onDeviceChange} />}
    </div>
  );
}
