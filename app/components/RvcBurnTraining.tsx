import { useEffect, useState } from 'react';
import { useI18n } from '../context/I18nContext';
import { getRvcBurnTraining, startRvcBurnTraining, type Dataset, type RvcBurnSettings, type RvcBurnStatus } from '../services/training';

const copy: Record<string, [string, string, string, string, string]> = {
  en: ['Add an isolated voice recording to the dataset.', 'RVC Burn requires an NVIDIA CUDA card.', 'RVC Burn components', 'Install voice', 'Voice in catalogue'],
  ru: ['Добавьте в датасет запись изолированного голоса.', 'Для RVC Burn нужна видеокарта NVIDIA с CUDA.', 'Компоненты RVC Burn', 'Установить голос', 'Голос в каталоге'],
  zh: ['向数据集添加单独的人声录音。', 'RVC Burn 需要支持 CUDA 的 NVIDIA 显卡。', 'RVC Burn 组件', '安装声音', '声音已在目录中'],
  ja: ['データセットに単独の音声録音を追加してください。', 'RVC Burn には NVIDIA CUDA GPU が必要です。', 'RVC Burn コンポーネント', '音声をインストール', '音声カタログに追加済み'],
  ko: ['데이터셋에 분리된 음성 녹음을 추가하세요.', 'RVC Burn에는 NVIDIA CUDA GPU가 필요합니다.', 'RVC Burn 구성 요소', '음성 설치', '카탈로그에 등록됨'],
  pt: ['Adicione uma gravação vocal isolada ao conjunto.', 'RVC Burn requer uma GPU NVIDIA com CUDA.', 'Componentes do RVC Burn', 'Instalar voz', 'Voz no catálogo'],
};
export const rvcBurnText = (language: string) => copy[language] ?? copy.en;

export function RvcBurnTraining({ dataset, busy, card, control, primary, onStarted, onError }: {
  dataset: Dataset; busy: boolean; card: string; control: string; primary: string;
  onStarted: () => void; onError: (error: string) => void;
}) {
  const { t, language } = useI18n();
  const [needsAudio, needsCuda, components] = rvcBurnText(language);
  const [status, setStatus] = useState<RvcBurnStatus>();
  const [settings, setSettings] = useState<RvcBurnSettings>({ epochs: 5, batch_size: 4, learning_rate: 0.0001 });
  const [working, setWorking] = useState(false);
  useEffect(() => {
    let active = true;
    void getRvcBurnTraining().then(value => { if (active) setStatus(value); })
      .catch(error => { if (active) onError(String(error)); });
    return () => { active = false; };
  }, [onError]);
  const songs = dataset.items.some(item => !item.instrumental);
  const valid = Number.isInteger(settings.epochs) && settings.epochs >= 1 && settings.epochs <= 1000
    && Number.isInteger(settings.batch_size) && settings.batch_size >= 1 && settings.batch_size <= 64
    && settings.learning_rate >= 1e-9 && settings.learning_rate <= 0.1;
  async function start() {
    setWorking(true);
    try { await startRvcBurnTraining(dataset.id, dataset.name, settings); onStarted(); }
    catch (error) { onError(String(error)); }
    finally { setWorking(false); }
  }
  const fields = [
    ['epochs', t('epochs'), 1, 1000, 1],
    ['batch_size', t('batchSize'), 1, 64, 1],
    ['learning_rate', t('learningRate'), 1e-9, 0.1, 'any'],
  ] as const;
  return <section className={card}>
    {!songs && <p role="status">{needsAudio}</p>}
    {status?.cuda === false && <p role="status">{needsCuda}</p>}
    {status && status.missing.length > 0 && <p className="break-words text-xs" role="status">
      {components} · <code>{status.folder}</code> · {status.missing.join(', ')}
    </p>}
    <div className="mt-3 grid gap-3 sm:grid-cols-3">
      {fields.map(([key, label, min, max, step]) => <label className="grid gap-1 text-xs" key={key}>
        <span>{label}</span><input type="number" className={control} value={Number.isFinite(settings[key]) ? settings[key] : ''} min={min} max={max} step={step}
          onChange={event => setSettings(current => ({ ...current, [key]: event.target.valueAsNumber }))} />
      </label>)}
    </div>
    <button type="button" className={primary + ' mt-4'} disabled={busy || working || !status?.ready || !songs || !valid}
      onClick={() => void start()}>{t('trainingStart')}</button>
  </section>;
}
