export type StableAudioOptions = {
  negative_prompt?: string;
  sampler_mode?: 'pingpong' | 'euler';
  apg_scale?: number;
  duration_padding_sec?: number;
};

export type StableAudioDraft = {
  negative_prompt: string;
  sampler_mode: '' | 'pingpong' | 'euler';
  apg_scale: string;
  duration_padding_sec: string;
};

export const emptyStableAudioDraft: StableAudioDraft = {
  negative_prompt: '', sampler_mode: '', apg_scale: '', duration_padding_sec: '',
};

export function stableAudioDraft(options?: StableAudioOptions): StableAudioDraft {
  return {
    negative_prompt: options?.negative_prompt ?? '',
    sampler_mode: options?.sampler_mode ?? '',
    apg_scale: options?.apg_scale?.toString() ?? '',
    duration_padding_sec: options?.duration_padding_sec?.toString() ?? '',
  };
}

export function stableAudioOptions(draft: StableAudioDraft): StableAudioOptions {
  return {
    negative_prompt: draft.negative_prompt.trim() || undefined,
    sampler_mode: draft.sampler_mode || undefined,
    apg_scale: draft.apg_scale === '' ? undefined : Number(draft.apg_scale),
    duration_padding_sec: draft.duration_padding_sec === '' ? undefined : Number(draft.duration_padding_sec),
  };
}

const labels = {
  en: ['Negative prompt', 'Sampler', 'APG scale', 'Duration padding (s)', 'Default', 'Enter a music prompt.'],
  ru: ['Негативный промпт', 'Сэмплер', 'Масштаб APG', 'Запас длительности (с)', 'По умолчанию', 'Введите описание музыки.'],
  zh: ['负面提示词', '采样器', 'APG 系数', '额外时长（秒）', '默认', '请输入音乐提示词。'],
  ja: ['ネガティブプロンプト', 'サンプラー', 'APG スケール', '追加の長さ（秒）', '既定値', '音楽のプロンプトを入力してください。'],
  ko: ['네거티브 프롬프트', '샘플러', 'APG 스케일', '추가 길이(초)', '기본값', '음악 프롬프트를 입력하세요.'],
  pt: ['Prompt negativo', 'Amostrador', 'Escala APG', 'Duração adicional (s)', 'Padrão', 'Digite um prompt musical.'],
} as const;

function text(language: string) {
  return labels[language as keyof typeof labels] ?? labels.en;
}

export function stableAudioPromptRequired(language: string) {
  return text(language)[5];
}

export function StableAudioControls({value, onChange, language}: {
  value: StableAudioDraft;
  onChange: (draft: StableAudioDraft) => void;
  language: string;
}) {
  const [negative, sampler, apg, padding, defaultValue] = text(language);
  const input = 'w-full rounded border border-white/10 bg-black/20 px-2 py-1';
  return <div className="grid grid-cols-2 gap-2 text-xs">
    <label className="col-span-2 grid gap-1">{negative}
      <input className={input} value={value.negative_prompt}
        onChange={e => onChange({...value, negative_prompt: e.target.value})} />
    </label>
    <label className="grid gap-1">{sampler}
      <select className={input} value={value.sampler_mode}
        onChange={e => onChange({...value, sampler_mode: e.target.value as StableAudioDraft['sampler_mode']})}>
        <option value="">{defaultValue}</option>
        <option value="pingpong">Ping-pong</option>
        <option value="euler">Euler</option>
      </select>
    </label>
    <label className="grid gap-1">{apg}
      <input type="number" className={input} step="any" placeholder="1" value={value.apg_scale}
        onChange={e => onChange({...value, apg_scale: e.target.value})} />
    </label>
    <label className="grid gap-1">{padding}
      <input type="number" className={input} step="any" min={0} placeholder="6" value={value.duration_padding_sec}
        onChange={e => onChange({...value, duration_padding_sec: e.target.value})} />
    </label>
  </div>;
}
