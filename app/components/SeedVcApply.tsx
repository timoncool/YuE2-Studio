import { useEffect, useRef, useState } from 'react';
import { FileAudio, Loader2, Wand2 } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { apiUrl } from '../services/apiBase';
import { responseJson } from '../services/rvcVoices';
import type { RvcSourceSong } from './RvcApplyVoice';
import { AudioCppModelInstall } from './HeartMuLaControls';
import { PrimaryButton, SecondaryButton } from './kit/Buttons';
import { FieldLabel } from './kit/FieldLabel';
import { ActionReason } from './kit/ToolCard';

const URL = '/v1/seed-vc';
const accompaniment = ['drums', 'bass', 'other', 'guitar', 'piano'];

const strings = {
  en: ['Voice sample (1–25 seconds)', 'Separate the song into vocals and accompaniment first.', 'Replace the voice', 'Converting the vocal…', 'Listen to the new take', 'Check availability',
    'From a voice sample', 'Any voice from a short recording, speech or singing', 'Made for singing: keeps the melody and the words · English, Chinese, Cantonese', 'SoulX-Singer needs the libsoxr runtime: Settings - Models', 'Choose a recording'],
  ru: ['Образец голоса (1–25 секунд)', 'Сначала разделите песню на вокал и аккомпанемент.', 'Заменить голос', 'Преобразуем вокал…', 'Слушать новый дубль', 'Проверить готовность',
    'По образцу голоса', 'Любой голос по короткой записи — речь или пение', 'Создан для пения: держит мелодию и слова · английский, китайский, кантонский', 'SoulX-Singer нужна библиотека libsoxr: Настройки — Модели', 'Выбрать запись'],
  ko: ['음성 샘플 (1–25초)', '먼저 곡을 보컬과 반주로 분리하세요.', '목소리 바꾸기', '보컬 변환 중…', '새 버전 듣기', '사용 가능 여부 확인',
    '음성 샘플로', '짧은 녹음으로 어떤 목소리든 — 말이나 노래', '노래용: 멜로디와 가사를 유지 · 영어, 중국어, 광둥어', 'SoulX-Singer에는 libsoxr 런타임이 필요합니다: 설정 - 모델', '녹음 선택'],
  zh: ['声音样本（1–25 秒）', '先将歌曲分离为人声和伴奏。', '替换声音', '正在转换人声…', '试听新版本', '检查可用性',
    '按声音样本', '用一段短录音换成任何声音——说话或唱歌', '专为歌唱：保留旋律和歌词 · 英语、中文、粤语', 'SoulX-Singer 需要 libsoxr 运行库：设置 - 模型', '选择录音'],
  ja: ['音声サンプル（1～25 秒）', '先に曲をボーカルと伴奏に分離してください。', '声を置き換える', 'ボーカルを変換中…', '新しいテイクを試聴', '利用可否を確認',
    '声のサンプルから', '短い録音からどんな声でも——話し声でも歌声でも', '歌のために作られたモデル：メロディと歌詞を保つ · 英語、中国語、広東語', 'SoulX-Singer には libsoxr ランタイムが必要です：設定 - モデル', '録音を選ぶ'],
  pt: ['Amostra de voz (1–25 segundos)', 'Separe primeiro a voz do acompanhamento.', 'Trocar a voz', 'A converter a voz…', 'Ouvir a nova versão', 'Verificar disponibilidade',
    'A partir de uma amostra', 'Qualquer voz a partir de uma gravação curta, fala ou canto', 'Feito para canto: mantém a melodia e a letra · inglês, chinês, cantonês', 'O SoulX-Singer precisa do runtime libsoxr: Definições - Modelos', 'Escolher gravação'],
};

interface Setup {
  ready: boolean;
  model_ready: boolean;
  runtime_ready: boolean;
  model_path: string;
  model_source_url: string;
  bytes: number;
  missing_bytes: number;
  installable: boolean;
  soulx?: { id: string; ready: boolean; model_ready: boolean; soxr_ready: boolean; bytes: number };
}
interface Take { song_id: string; audio_url: string }
export type Converter = 'seed_vc' | 'soulx_singer_svc';

/** The models of the voice replacement form, as its model cards name them: an RVC voice, or a voice from a sample. */
export function voiceModels(language: keyof typeof strings): { id: 'rvc' | Converter; name: string; hint: string }[] {
  const [, , , , , , , seedHint, soulxHint] = strings[language];
  const rvc = rvcStrings[language];
  return [
    { id: 'rvc', name: 'RVC', hint: rvc },
    { id: 'seed_vc', name: 'Seed-VC', hint: seedHint },
    { id: 'soulx_singer_svc', name: 'SoulX-Singer', hint: soulxHint },
  ];
}

const rvcStrings = {
  en: 'A voice from your installed ones: from the catalogue or trained by you',
  ru: 'Голос из установленных: из каталога или обученный вами',
  zh: '已安装的声音：来自目录或你自己训练的',
  ja: 'インストール済みの声：カタログから、または自分で学習したもの',
  ko: '설치된 목소리: 카탈로그에서 또는 직접 학습한 목소리',
  pt: 'Uma voz das instaladas: do catálogo ou treinada por si',
};

/** Replacing a separated vocal with the voice of an uploaded sample, by the model chosen above it in the form. */
export function SeedVcApply({ song, converter, onSaved }: { song: RvcSourceSong; converter: Converter; onSaved?: () => void }) {
  const { language } = useI18n();
  const [sample, separate, convert, converting, listen, check, , , , soxrMissing, chooseRecording] = strings[language];
  const picker = useRef<HTMLInputElement>(null);
  const [file, setFile] = useState<File>();
  const [setup, setSetup] = useState<Setup>();
  const [busy, setBusy] = useState(false);
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState('');
  const [take, setTake] = useState<Take>();
  const hasStems = song.stems.includes('vocals')
    && (song.stems.includes('instrumental') || accompaniment.every(stem => song.stems.includes(stem)));
  const soulx = converter === 'soulx_singer_svc';
  const ready = soulx ? Boolean(setup?.soulx?.ready) : Boolean(setup?.ready);

  useEffect(() => { void refresh(); }, []);

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
    if (!file || !hasStems || !ready) return;
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
        body: JSON.stringify({ song_id: song.id, upload_id: reference.upload_id, model: converter }),
      })));
      onSaved?.();
    } catch (failure) {
      setError(String(failure));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="grid gap-3 text-xs">
        {!hasStems && <p className="text-[13px] text-zinc-600 dark:text-zinc-400">{separate}</p>}
        {setup && !soulx && (!setup.model_ready || !setup.runtime_ready) && setup.installable && (
          <AudioCppModelInstall model="seed_vc" bytes={setup.missing_bytes} fit
            target={{ catalogue: URL, installPath: `${URL}/install`, label: 'Seed-VC' }}
            licenseUrl={setup.model_source_url} language={language} onInstalled={() => void refresh()} />
        )}
        {setup && soulx && !setup.soulx?.model_ready && (
          <AudioCppModelInstall model="soulx_singer_svc" bytes={setup.soulx?.bytes ?? 0} fit language={language} onInstalled={() => void refresh()} />
        )}
        {setup && soulx && setup.soulx?.model_ready && !setup.soulx.soxr_ready && <ActionReason>{soxrMissing}</ActionReason>}
        {!setup && <SecondaryButton disabled={checking} onClick={() => void refresh()}>{check}</SecondaryButton>}
        <div className="grid">
          <FieldLabel>{sample}</FieldLabel>
          <div className="flex min-w-0 items-center gap-2">
            <SecondaryButton disabled={busy} aria-label={`${sample}: ${file?.name ?? chooseRecording}`} onClick={() => picker.current?.click()}>
              <FileAudio size={13} aria-hidden />{chooseRecording}
            </SecondaryButton>
            {file && <span className="min-w-0 truncate text-zinc-600 dark:text-zinc-300" title={file.name}>{file.name}</span>}
          </div>
          <input ref={picker} type="file" className="hidden" accept="audio/*,.wav,.flac,.mp3,.m4a,.ogg,.aiff" aria-label={sample}
            onChange={event => { setFile(event.target.files?.[0]); event.target.value = ''; }} />
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <PrimaryButton disabled={busy || !file || !hasStems || !ready} onClick={() => void apply()}>
            {busy ? <Loader2 size={14} className="animate-spin" /> : <Wand2 size={14} />}{busy ? converting : convert}
          </PrimaryButton>
        </div>
        {error && <p role="alert" className="rounded-lg bg-rose-500/10 px-3 py-2 text-xs text-rose-700 wrap-anywhere dark:text-rose-300">{error}</p>}
        {take && <audio controls preload="none" aria-label={listen} src={apiUrl(take.audio_url)} />}
    </div>
  );
}
