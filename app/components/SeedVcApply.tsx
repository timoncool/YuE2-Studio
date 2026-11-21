import { useId, useState } from 'react';
import { useI18n } from '../context/I18nContext';
import { apiUrl } from '../services/apiBase';
import { responseJson } from '../services/rvcVoices';
import type { RvcSourceSong } from './RvcApplyVoice';

const URL = '/v1/seed-vc';
const accompaniment = ['drums', 'bass', 'other', 'guitar', 'piano'];

const strings = {
  en: ['Voice sample (1–25 seconds)', 'Separate the song into vocals and accompaniment first.', 'Get the Seed-VC model', 'Place the downloaded file here', 'Install audio.cpp in the separation settings.', 'Convert with Seed-VC', 'Converting the vocal…', 'Listen to the new take', 'Check availability'],
  ru: ['Образец голоса (1–25 секунд)', 'Сначала разделите песню на вокал и аккомпанемент.', 'Скачать модель Seed-VC', 'Поместите скачанный файл по этому пути', 'Установите audio.cpp в настройках разделения.', 'Заменить голос через Seed-VC', 'Преобразуем вокал…', 'Слушать новый дубль', 'Проверить готовность'],
  ko: ['음성 샘플 (1–25초)', '먼저 곡을 보컬과 반주로 분리하세요.', 'Seed-VC 모델 받기', '다운로드한 파일을 여기에 넣으세요', '분리 설정에서 audio.cpp를 설치하세요.', 'Seed-VC로 변환', '보컬 변환 중…', '새 버전 듣기', '사용 가능 여부 확인'],
  zh: ['声音样本（1–25 秒）', '先将歌曲分离为人声和伴奏。', '获取 Seed-VC 模型', '将下载的文件放到此处', '在分离设置中安装 audio.cpp。', '使用 Seed-VC 转换', '正在转换人声…', '试听新版本', '检查可用性'],
  ja: ['音声サンプル（1～25 秒）', '先に曲をボーカルと伴奏に分離してください。', 'Seed-VC モデルを取得', 'ダウンロードしたファイルをここに置いてください', '分離設定で audio.cpp をインストールしてください。', 'Seed-VC で変換', 'ボーカルを変換中…', '新しいテイクを試聴', '利用可否を確認'],
  pt: ['Amostra de voz (1–25 segundos)', 'Separe primeiro a voz do acompanhamento.', 'Obter o modelo Seed-VC', 'Coloque aqui o ficheiro descarregado', 'Instale o audio.cpp nas definições de separação.', 'Converter com Seed-VC', 'A converter a voz…', 'Ouvir a nova versão', 'Verificar disponibilidade'],
};

interface Setup {
  ready: boolean;
  model_ready: boolean;
  runtime_ready: boolean;
  model_path: string;
  model_url: string;
  model_source_url: string;
  bytes: number;
}
interface Take { song_id: string; audio_url: string }

export function SeedVcApply({ song, control, onSaved }: { song: RvcSourceSong; control: string; onSaved?: () => void }) {
  const { language } = useI18n();
  const [sample, separate, model, place, runtime, convert, converting, listen, check] = strings[language];
  const inputId = useId();
  const [file, setFile] = useState<File>();
  const [setup, setSetup] = useState<Setup>();
  const [busy, setBusy] = useState(false);
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState('');
  const [take, setTake] = useState<Take>();
  const hasStems = song.stems.includes('vocals')
    && (song.stems.includes('instrumental') || accompaniment.every(stem => song.stems.includes(stem)));

  async function refresh() {
    setChecking(true);
    setError('');
    try {
      setSetup(await responseJson<Setup>(await fetch(apiUrl(URL))));
    } catch (failure) {
      setError(String(failure));
    } finally {
      setChecking(false);
    }
  }

  async function apply() {
    if (!file || !hasStems || !setup?.ready) return;
    setBusy(true);
    setError('');
    setTake(undefined);
    try {
      const form = new FormData();
      form.append('audio', file);
      const reference = await responseJson<{ upload_id: string }>(
        await fetch(apiUrl('/v1/processing/reference'), { method: 'POST', body: form }),
      );
      setTake(await responseJson<Take>(await fetch(apiUrl(URL), {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ song_id: song.id, upload_id: reference.upload_id }),
      })));
      onSaved?.();
    } catch (failure) {
      setError(String(failure));
    } finally {
      setBusy(false);
    }
  }

  return (
    <details className="mt-4 border-t border-zinc-200 pt-3 dark:border-white/10" onToggle={event => {
      if (event.currentTarget.open && !setup && !checking) void refresh();
    }}>
      <summary className="cursor-pointer text-sm font-semibold">Seed-VC</summary>
      <div className="mt-2 space-y-2 text-xs">
        {!hasStems && <p>{separate}</p>}
        {setup && !setup.model_ready && (
          <div>
            <a className="underline" href={setup.model_url} target="_blank" rel="noreferrer">
              {model} ({new Intl.NumberFormat(language).format(setup.bytes)} B)
            </a>
            <a className="ml-2 underline" href={setup.model_source_url} target="_blank" rel="noreferrer">GPL-3.0</a>
            <p>{place} <code className="break-all">{setup.model_path}</code></p>
          </div>
        )}
        {setup && !setup.runtime_ready && <p>{runtime}</p>}
        {!setup?.ready && <button type="button" className={control} disabled={checking} onClick={() => void refresh()}>{check}</button>}
        <label htmlFor={inputId} className="block">{sample}</label>
        <input id={inputId} className={control} type="file" accept="audio/*,.wav,.flac,.mp3,.m4a,.ogg,.aiff"
          disabled={busy} onChange={event => setFile(event.target.files?.[0])} />
        <button type="button" className={control} disabled={busy || !file || !hasStems || !setup?.ready} onClick={() => void apply()}>
          {busy ? converting : convert}
        </button>
        {error && <p role="alert">{error}</p>}
        {take && <audio controls preload="none" aria-label={listen} src={apiUrl(take.audio_url)} />}
      </div>
    </details>
  );
}
