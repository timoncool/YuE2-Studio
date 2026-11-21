export interface HeartMuLaOptions {
  temperature?: number;
  top_k?: number;
  codec_duration_sec?: number;
  codec_guidance_scale?: number;
}

type Setting = keyof HeartMuLaOptions;
export type HeartMuLaDraft = Record<Setting, string>;

export const emptyHeartMuLaDraft: HeartMuLaDraft = {
  temperature: '', top_k: '', codec_duration_sec: '', codec_guidance_scale: '',
};

const fields: readonly {name: Setting; placeholder: string; step: string}[] = [
  {name: 'temperature', placeholder: '1', step: 'any'},
  {name: 'top_k', placeholder: '50', step: '1'},
  {name: 'codec_duration_sec', placeholder: '29.76', step: 'any'},
  {name: 'codec_guidance_scale', placeholder: '1.25', step: 'any'},
];

const labels: Record<string, readonly string[]> = {
  en: ['Temperature', 'Top-k', 'Codec chunk length (s)', 'Codec guidance'],
  ru: ['Температура', 'Top-k', 'Длина сегмента кодека (с)', 'CFG кодека'],
  zh: ['温度', 'Top-k', '编解码分段时长（秒）', '编解码引导强度'],
  ja: ['温度', 'Top-k', 'コーデックの区間長（秒）', 'コーデックのガイダンス'],
  ko: ['온도', 'Top-k', '코덱 구간 길이(초)', '코덱 가이던스'],
  pt: ['Temperatura', 'Top-k', 'Duração do trecho do codec (s)', 'Orientação do codec'],
};

export function heartMuLaOptions(draft: HeartMuLaDraft): HeartMuLaOptions | undefined {
  const settings: HeartMuLaOptions = {};
  for (const {name} of fields) {
    const value = draft[name].trim();
    if (value) settings[name] = Number(value);
  }
  return Object.keys(settings).length ? settings : undefined;
}

export function heartMuLaDraft(options?: HeartMuLaOptions): HeartMuLaDraft {
  const draft = {...emptyHeartMuLaDraft};
  for (const {name} of fields) {
    if (options?.[name] !== undefined) draft[name] = String(options[name]);
  }
  return draft;
}

export function HeartMuLaControls({values, onChange, language = 'en'}: {
  values: HeartMuLaDraft;
  onChange: (values: HeartMuLaDraft) => void;
  language?: string;
}) {
  const names = labels[language] ?? labels.en;
  return (
    <div className="grid grid-cols-2 gap-2">
      {fields.map(({name, placeholder, step}, index) => (
        <label className="flex min-w-0 flex-col gap-1 text-xs" key={name}>
          <span>{names[index]}</span>
          <input
            className="w-full rounded border border-white/10 bg-black/20 px-2 py-1"
            type="number"
            min={name === 'top_k' ? 1 : 0}
            step={step}
            placeholder={placeholder}
            value={values[name]}
            onChange={event => onChange({...values, [name]: event.target.value})}
          />
        </label>
      ))}
    </div>
  );
}
