import { useEffect, useState } from 'react';
import { apiUrl } from '../services/apiBase';

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
const installLabels: Record<string, readonly [string, string, string]> = {
  en: ['Install HeartMuLa', 'Installing HeartMuLa…', 'HeartMuLa installation failed.'],
  ru: ['Установить HeartMuLa', 'Установка HeartMuLa…', 'Не удалось установить HeartMuLa.'],
  zh: ['安装 HeartMuLa', '正在安装 HeartMuLa…', '无法安装 HeartMuLa。'],
  ja: ['HeartMuLa をインストール', 'HeartMuLa をインストール中…', 'HeartMuLa をインストールできませんでした。'],
  ko: ['HeartMuLa 설치', 'HeartMuLa 설치 중…', 'HeartMuLa를 설치하지 못했습니다.'],
  pt: ['Instalar HeartMuLa', 'Instalando HeartMuLa…', 'Não foi possível instalar HeartMuLa.'],
};

export function AudioCppModelInstall({ model, bytes, licenseUrl, language, onInstalled }: {
  model: 'heartmula' | 'stable_audio' | 'stable-audio-3-medium' | 'heartmula-3b-q8_0' | 'hotstep-minimax-music3' | 'hotstep-ace-step-1.5' | 'soulx_singer' | 'soulx_singer_svc';
  bytes: number;
  licenseUrl?: string;
  language: string;
  onInstalled: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [percent, setPercent] = useState<number>();
  const hotstepHeart = model === 'heartmula-3b-q8_0';
  const hotstepMinimax = model === 'hotstep-minimax-music3';
  const hotstepAce = model === 'hotstep-ace-step-1.5';
  const hotstep = model === 'stable-audio-3-medium' || hotstepHeart || hotstepMinimax || hotstepAce;
  const soulx = model === 'soulx_singer' ? 'SoulX-Singer SVS' : model === 'soulx_singer_svc' ? 'SoulX-Singer SVC' : undefined;
  const name = soulx ?? (hotstepAce ? 'ACE-Step 1.5 Turbo (HOT-Step)' : hotstepMinimax ? 'MiniMax Music3 (HOT-Step)' : hotstepHeart ? 'HeartMuLa 3B (HOT-Step)' : hotstep ? 'Stable Audio 3 Medium (HOT-Step)' : model === 'stable_audio' ? 'Stable Audio 3' : 'HeartMuLa');
  const [install, installing, failed] = (installLabels[language] ?? installLabels.en).map(label => label.replace('HeartMuLa', name));
  const size = bytes > 0 ? ` · ${new Intl.NumberFormat(language, { maximumFractionDigits: 2 }).format(bytes / (1024 ** 3))} GiB` : '';

  async function status(signal?: AbortSignal) {
    const response = await fetch(apiUrl(hotstep ? '/v1/music/native/hotstep/profiles' : '/v1/music/audio-cpp/models'), { signal });
    if (!response.ok || signal?.aborted) return;
    const data = await response.json();
    if (signal?.aborted) return;
    const row = (hotstep ? data.profiles : data.models)?.find((item: {id: string}) => item.id === model);
    if (!row) return;
    setBusy(Boolean(row.installing));
    const download = row.download;
    setPercent(row.installing && download && !download.done && download.total_bytes > 0
      ? Math.min(100, Math.floor(100 * download.downloaded_bytes / download.total_bytes)) : undefined);
    if (!row.installing && download?.error) setError(download.error);
    if (!row.installing && row.ready) onInstalled();
  }

  useEffect(() => {
    const controller = new AbortController();
    const poll = () => void status(controller.signal).catch(() => {});
    poll();
    const timer = window.setInterval(poll, 1_000);
    return () => { controller.abort(); window.clearInterval(timer); };
  }, [model, onInstalled]);

  async function start() {
    setBusy(true);
    setError('');
    try {
      const route = hotstep ? `/v1/music/native/hotstep/${hotstepAce ? 'ace-step-1.5' : hotstepMinimax ? 'minimax-music3' : hotstepHeart ? 'heartmula' : 'stable-audio-3'}/install` : '/v1/music/audio-cpp/models/install';
      const result = await fetch(apiUrl(route), {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ model }),
      });
      const body = await result.json();
      if (!result.ok || (body.installing !== true && body.ready !== true)) throw new Error(body.error || failed);
      await status();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : failed);
      setBusy(false);
    }
  }

  return <div className="space-y-1 text-xs">
    <button type="button" className="text-pink-600 underline disabled:opacity-50 dark:text-pink-300"
      disabled={busy} onClick={() => void start()}>
      {busy ? installing + (percent === undefined ? '' : ` ${percent}%`) : install + size}
    </button>
    {!busy && error && <p role="alert">{error}</p>}
    {licenseUrl && <a href={licenseUrl} target="_blank" rel="noreferrer" className="block underline">Stability AI Community License</a>}
  </div>;
}
