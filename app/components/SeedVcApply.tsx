import { useEffect, useState } from 'react';
import { Loader2, Wand2 } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { apiUrl } from '../services/apiBase';
import { responseJson } from '../services/rvcVoices';
import type { RvcSourceSong } from './RvcApplyVoice';
import { AudioCppModelInstall } from './HeartMuLaControls';
import { PrimaryButton, SecondaryButton } from './kit/Buttons';
import { SoundSource, type SoundPick } from './kit/SoundSource';
import { RefTrim } from './kit/RefTrim';
import { soundSourceStrings } from '../i18n/soundSource';
import { ActionReason } from './kit/ToolCard';

const URL = '/v1/seed-vc';
const accompaniment = ['drums', 'bass', 'other', 'guitar', 'piano'];

const strings = {
  en: ['Voice sample', 'Separate the song into vocals and accompaniment first.', 'Replace the voice', 'Converting the vocal…', 'Listen to the new take', 'Check availability',
    'From a voice sample', 'Any voice from a short recording, speech or singing', 'Made for singing: keeps the melody and the words · English, Chinese, Cantonese', 'SoulX-Singer needs the libsoxr runtime: Settings - Models', '10–25 seconds of one voice without music — speech or singing.'],
  ru: ['Образец голоса', 'Сначала разделите песню на вокал и аккомпанемент.', 'Заменить голос', 'Преобразуем вокал…', 'Слушать новый дубль', 'Проверить готовность',
    'По образцу голоса', 'Любой голос по короткой записи — речь или пение', 'Создан для пения: держит мелодию и слова · английский, китайский, кантонский', 'SoulX-Singer нужна библиотека libsoxr: Настройки — Модели', '10–25 секунд одного голоса без музыки — речь или пение.'],
  ko: ['음성 샘플', '먼저 곡을 보컬과 반주로 분리하세요.', '목소리 바꾸기', '보컬 변환 중…', '새 버전 듣기', '사용 가능 여부 확인',
    '음성 샘플로', '짧은 녹음으로 어떤 목소리든 — 말이나 노래', '노래용: 멜로디와 가사를 유지 · 영어, 중국어, 광둥어', 'SoulX-Singer에는 libsoxr 런타임이 필요합니다: 설정 - 모델', '음악 없이 한 사람의 목소리 10–25초 — 말이나 노래.'],
  zh: ['声音样本', '先将歌曲分离为人声和伴奏。', '替换声音', '正在转换人声…', '试听新版本', '检查可用性',
    '按声音样本', '用一段短录音换成任何声音——说话或唱歌', '专为歌唱：保留旋律和歌词 · 英语、中文、粤语', 'SoulX-Singer 需要 libsoxr 运行库：设置 - 模型', '10–25 秒、只有一个人的声音、没有音乐——说话或唱歌都可以。'],
  ja: ['音声サンプル', '先に曲をボーカルと伴奏に分離してください。', '声を置き換える', 'ボーカルを変換中…', '新しいテイクを試聴', '利用可否を確認',
    '声のサンプルから', '短い録音からどんな声でも——話し声でも歌声でも', '歌のために作られたモデル：メロディと歌詞を保つ · 英語、中国語、広東語', 'SoulX-Singer には libsoxr ランタイムが必要です：設定 - モデル', '音楽なしで一人の声を 10〜25 秒——話し声でも歌声でも。'],
  pt: ['Amostra de voz', 'Separe primeiro a voz do acompanhamento.', 'Trocar a voz', 'A converter a voz…', 'Ouvir a nova versão', 'Verificar disponibilidade',
    'A partir de uma amostra', 'Qualquer voz a partir de uma gravação curta, fala ou canto', 'Feito para canto: mantém a melodia e a letra · inglês, chinês, cantonês', 'O SoulX-Singer precisa do runtime libsoxr: Definições - Modelos', '10–25 segundos de uma só voz sem música — fala ou canto.'],
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
interface Sample { upload_id: string; from: 'vocals' | 'track'; start: number; end: number; seconds: number; total_seconds: number }
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
  const [sampleLabel, , convert, converting, listen, check, , , , soxrMissing, sampleHint] = strings[language];
  const take = soundSourceStrings[language];
  const [setup, setSetup] = useState<Setup>();
  const [busy, setBusy] = useState(false);
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState('');
  const [result, setResult] = useState<Take>();
  const [pick, setPick] = useState<SoundPick | null>(null);
  const [sample, setSample] = useState<Sample | null>(null);
  const [preparing, setPreparing] = useState(false);
  const [trimming, setTrimming] = useState(false);
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

  async function prepare(songId: string, piece?: [number, number]) {
    setPreparing(true);
    setError('');
    try {
      setSample(await responseJson<Sample>(await fetch(apiUrl('/v1/processing/reference/from-song'), {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ song_id: songId, ...(piece ? { start: piece[0], end: piece[1] } : {}) }),
      })));
      setTrimming(false);
    } catch (failure) {
      setSample(null);
      setError(String(failure));
    } finally {
      setPreparing(false);
    }
  }

  useEffect(() => {
    setSample(null);
    setTrimming(false);
    if (pick) void prepare(pick.songId);
  }, [pick?.songId]);

  async function apply() {
    if (!sample || !hasStems || !ready) return;
    setBusy(true);
    setError('');
    setResult(undefined);
    try {
      setResult(await responseJson<Take>(await fetch(apiUrl(URL), {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ song_id: song.id, upload_id: sample.upload_id, model: converter }),
      })));
      onSaved?.();
    } catch (failure) {
      setError(String(failure));
    } finally {
      setBusy(false);
    }
  }

  const time = (seconds: number) => `${Math.floor(seconds / 60)}:${String(Math.floor(seconds % 60)).padStart(2, '0')}`;
  const length = sample ? time(sample.seconds) : '';
  const pieceUrl = sample && pick ? apiUrl(sample.from === 'vocals'
    ? `/v1/library/songs/${encodeURIComponent(pick.songId)}/stems/vocals`
    : `/v1/library/media/${encodeURIComponent(pick.songId)}`) : '';

  return (
    <div className="grid gap-3 text-xs">
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
        <SoundSource place="voice-sample" label={sampleLabel} hint={sampleHint} defaultSource="mic" disabled={busy}
          need={{ min: 1, good: 10, max: 25 }} value={pick} onChange={setPick}
          titleFor={() => `${sampleLabel} · ${new Date().toLocaleString(language, { day: '2-digit', month: '2-digit', hour: '2-digit', minute: '2-digit' })}`} />
        {preparing && <p role="status" className="flex items-center gap-2 text-[13px] text-zinc-600 dark:text-zinc-400"><Loader2 size={13} className="animate-spin" />{take.preparing}</p>}
        {sample && !preparing && (
          <div className="grid gap-2">
            <p className="text-[13px] leading-[18px] text-zinc-600 dark:text-zinc-400">
              {sample.total_seconds <= sample.seconds + 0.05 ? take.whole('0:25')
                : sample.from === 'vocals' ? take.tookVocals(length, time(sample.start), time(sample.end))
                  : take.tookPiece(length, time(sample.start), time(sample.end))}
              {sample.total_seconds > sample.seconds + 0.05 && !trimming && <>{' '}
                <button type="button" className="font-semibold text-pink-600 underline-offset-2 hover:underline dark:text-pink-300" onClick={() => setTrimming(true)}>{take.changePiece}</button></>}
            </p>
            {sample.from === 'track' && pick?.origin === 'library' && <p className="text-[13px] leading-[18px] text-amber-700 dark:text-amber-300">{take.noVocals}</p>}
            {trimming && pieceUrl && <RefTrim url={pieceUrl} limit={25} min={1} initial={[sample.start, sample.end]} busy={preparing}
              onApply={(start, end) => pick && void prepare(pick.songId, [start, end])} />}
          </div>
        )}
        <div className="flex flex-wrap items-center justify-end gap-2">
          <PrimaryButton disabled={busy || !sample || !hasStems || !ready} onClick={() => void apply()}>
            {busy ? <Loader2 size={14} className="animate-spin" /> : <Wand2 size={14} />}{busy ? converting : convert}
          </PrimaryButton>
        </div>
        {error && <p role="alert" className="rounded-lg bg-rose-500/10 px-3 py-2 text-xs text-rose-700 wrap-anywhere dark:text-rose-300">{error}</p>}
        {result && <audio controls preload="none" aria-label={listen} src={apiUrl(result.audio_url)} />}
    </div>
  );
}
