import { useEffect, useState } from 'react';
import { fetchSa3Training, installSa3Training, startSa3Training, type Dataset, type Sa3TrainingSettings, type Sa3TrainingStatus } from '../services/training';

export const sa3TrainingText: Record<string, [string, string, string, string, string, string, string]> = {
  en: ['Training engine', 'Instrumental songs with a style description are required.', 'An NVIDIA CUDA card is required.', 'HOT-Step CUDA trainer is unavailable.', 'Install training pack', 'Installing training pack…', 'Crop duration (s)'],
  ru: ['Движок обучения', 'Нужны инструментальные треки с описанием стиля.', 'Требуется видеокарта NVIDIA с CUDA.', 'Тренер HOT-Step CUDA недоступен.', 'Установить пакет обучения', 'Устанавливается пакет обучения…', 'Длительность фрагмента (с)'],
  zh: ['训练引擎', '需要带有风格描述的纯音乐曲目。', '需要支持 CUDA 的 NVIDIA 显卡。', 'HOT-Step CUDA 训练器不可用。', '安装训练包', '正在安装训练包…', '片段时长（秒）'],
  ja: ['学習エンジン', 'スタイルを記述したインスト曲が必要です。', 'NVIDIA CUDA 対応 GPU が必要です。', 'HOT-Step CUDA 学習エンジンを利用できません。', '学習パックをインストール', '学習パックをインストール中…', '区間の長さ（秒）'],
  ko: ['학습 엔진', '스타일 설명이 있는 연주곡이 필요합니다.', 'NVIDIA CUDA 그래픽 카드가 필요합니다.', 'HOT-Step CUDA 학습 엔진을 사용할 수 없습니다.', '학습 팩 설치', '학습 팩 설치 중…', '구간 길이(초)'],
  pt: ['Motor de treinamento', 'São necessárias faixas instrumentais com descrição de estilo.', 'É necessária uma GPU NVIDIA com CUDA.', 'O treinador HOT-Step CUDA está indisponível.', 'Instalar pacote de treinamento', 'Instalando pacote de treinamento…', 'Duração do trecho (s)'],
};

const fields = [
  ['steps', 200, 1, 100000, 1],
  ['rank', 8, 1, 128, 1],
  ['alpha', 8, 0.01, 512, 'any'],
  ['learning_rate', 0.0001, 1e-9, 0.1, 'any'],
  ['duration_seconds', 30, 1, 384, 'any'],
  ['seed', 42, 0, 4294967295, 1],
] as const;

export function HotStepSa3Training({ dataset, busy, language, labels, card, control, primary, onStarted, onError }: {
  dataset: Dataset;
  busy: boolean;
  language: string;
  labels: { start: string; steps: string; rank: string; alpha: string; learning_rate: string; seed: string };
  card: string;
  control: string;
  primary: string;
  onStarted: () => void;
  onError: (error: string) => void;
}) {
  const [, needsSongs, needsCuda, unavailable, install, installing, duration_seconds] = sa3TrainingText[language] ?? sa3TrainingText.en;
  const [settings, setSettings] = useState<Partial<Record<keyof Sa3TrainingSettings, string>>>({});
  const [status, setStatus] = useState<Sa3TrainingStatus>();
  const [working, setWorking] = useState(false);
  const [statusFailed, setStatusFailed] = useState(false);
  useEffect(() => {
    let active = true;
    const refresh = async () => {
      try {
        const state = await fetchSa3Training();
        if (active) { setStatus(state); setStatusFailed(false); }
      } catch {
        if (active) setStatusFailed(true);
      }
    };
    void refresh();
    const timer = window.setInterval(() => void refresh(), 1000);
    return () => { active = false; window.clearInterval(timer); };
  }, []);

  const eligible = dataset.items.length > 0 && dataset.items.every(item => item.instrumental && item.style.trim());
  const current = Object.fromEntries(fields.map(([key, fallback]) => [key, Number(settings[key] || fallback)])) as Sa3TrainingSettings;
  const valid = fields.every(([key, , min, max, step]) => current[key] >= min && current[key] <= max && (step !== 1 || Number.isInteger(current[key])));
  const size = status?.missing_bytes ? new Intl.NumberFormat(language, { maximumFractionDigits: 2 }).format(status.missing_bytes / (1024 ** 3)) + ' GiB' : '';
  const progress = status?.download;
  const percent = progress?.total_bytes ? Math.min(100, Math.floor((100 * progress.downloaded_bytes) / progress.total_bytes)) : undefined;

  const perform = async (action: 'start' | 'install') => {
    setWorking(true);
    try {
      if (action === 'start') {
        await startSa3Training(dataset.id, dataset.name, current);
        onStarted();
      } else {
        await installSa3Training();
        setStatus(await fetchSa3Training());
      }
    } catch (error) { onError(error instanceof Error ? error.message : String(error)); }
    finally { setWorking(false); }
  };

  return <section className={card}>
    {!eligible && <p role="status" className="mb-2 text-sm">{needsSongs}</p>}
    {status?.cuda === false && <p role="status" className="mb-2 text-sm">{needsCuda}</p>}
    {(statusFailed || status?.installable === false) && <p role="status" className="mb-2 text-sm">{unavailable}</p>}
    {status?.cuda && status.installable && !status.ready && <button type="button" className={primary}
      disabled={working || status.installing} onClick={() => void perform('install')}>
      {status.installing ? [installing, percent === undefined ? '' : percent + '%'].filter(Boolean).join(' ') : [install, size].filter(Boolean).join(' · ')}
    </button>}
    {progress?.error && <p className="mt-2 text-xs" role="alert">{progress.error}</p>}
    <div className="mt-3 grid gap-3 sm:grid-cols-3">
      {fields.map(([key, fallback, min, max, step]) => <label className="grid gap-1 text-xs" key={key}>
        <span>{key === 'duration_seconds' ? duration_seconds : labels[key]}</span>
        <input type="number" className={control} min={min} max={max} step={step} value={settings[key] ?? fallback}
          onChange={event => setSettings(current => ({ ...current, [key]: event.target.value }))} />
      </label>)}
    </div>
    <button type="button" className={primary + ' mt-4'} disabled={working || busy || !eligible || !valid || !status?.ready || !status.cuda}
      onClick={() => void perform('start')}>{labels.start}</button>
  </section>;
}
