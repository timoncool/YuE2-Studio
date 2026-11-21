import type { YueRequest } from '../types';

export const hotStepSa3Profile = 'stable-audio-3-medium';
export const hotStepHeartmulaProfile = 'heartmula-3b-q8_0';
export type HotStepSa3Draft = NonNullable<YueRequest['hotstep_sa3']>;

const labels: Record<string, [string, string, string, string]> = {
  en: ['Sampler', 'Adapter', 'None', 'Adapter strength'],
  ru: ['Сэмплер', 'Адаптер', 'Без адаптера', 'Сила адаптера'],
  zh: ['采样器', '适配器', '不使用适配器', '适配器强度'],
  ja: ['サンプラー', 'アダプター', 'アダプターなし', 'アダプター強度'],
  ko: ['샘플러', '어댑터', '어댑터 없음', '어댑터 강도'],
  pt: ['Amostrador', 'Adaptador', 'Sem adaptador', 'Intensidade do adaptador'],
};

export function HotStepSa3Controls({ value, onChange, adapters, control, language, hasSampler = true }: {
  value: HotStepSa3Draft;
  onChange: (value: HotStepSa3Draft) => void;
  adapters: string[];
  control: string;
  language: string;
  hasSampler?: boolean;
}) {
  const [sampler, adapter, none, strength] = labels[language] ?? labels.en;
  return <div className="grid grid-cols-2 gap-2">
    {hasSampler && <label className="space-y-1 text-xs">
      <span>{sampler}</span>
      <select className={control} value={value.sampler ?? 'pingpong'}
        onChange={event => onChange({ ...value, sampler: event.target.value === 'euler' ? 'euler' : 'pingpong' })}>
        <option value="pingpong">Ping-pong</option>
        <option value="euler">Euler</option>
      </select>
    </label>}
    {(adapters.length > 0 || value.adapter) && <label className="space-y-1 text-xs">
      <span>{adapter}</span>
      <select className={control} value={value.adapter ?? ''}
        onChange={event => onChange({ ...value, adapter: event.target.value || undefined })}>
        <option value="">{none}</option>
        {value.adapter && !adapters.includes(value.adapter) && <option disabled>{value.adapter}</option>}
        {adapters.map(name => <option key={name}>{name}</option>)}
      </select>
    </label>}
    {value.adapter && <label className="space-y-1 text-xs">
      <span>{strength}</span>
      <input type="number" className={control} value={value.adapter_scale ?? 1} min={0} max={2} step={0.05}
        onChange={event => onChange({ ...value, adapter_scale: event.target.value === '' ? undefined : event.target.valueAsNumber })} />
    </label>}
  </div>;
}
