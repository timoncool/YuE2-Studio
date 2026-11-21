import { useState } from 'react';
import { useI18n } from '../context/I18nContext';
import { backingTempo, failed } from '../services/scoreApi';

const labels = {
  en: ['Measure backing tempo', 'Use {bpm} BPM', 'Check the estimate against the recording. Use a matching score for harmony and sections.'],
  ru: ['Измерить темп аккомпанемента', 'Использовать {bpm} BPM', 'Сверьте оценку с записью. Гармонию и секции задаёт подходящая партитура.'],
  pt: ['Medir o tempo do acompanhamento', 'Usar {bpm} BPM', 'Confirme a estimativa com a gravação. Use uma partitura correspondente para a harmonia e as secções.'],
  zh: ['测量伴奏速度', '使用 {bpm} BPM', '请对照录音确认速度。用相匹配的乐谱指定和声及段落。'],
  ja: ['伴奏のテンポを測定', '{bpm} BPM を使う', '録音で推定値を確認してください。和声と構成には対応する楽譜を使います。'],
  ko: ['반주 템포 측정', '{bpm} BPM 사용', '녹음과 추정치를 비교하세요. 화성과 구간은 일치하는 악보로 지정합니다.'],
};

export function BackingTempo({ songId, onApply }: { songId: string; onApply: (bpm: number) => void }) {
  const { language } = useI18n();
  const [measure, apply, hint] = labels[language];
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ bpm: number } | { error: string }>();
  const detect = async () => {
    setBusy(true);
    setResult(undefined);
    try {
      const answer = await backingTempo(songId);
      setResult(failed(answer) ? { error: answer.error } : { bpm: answer.bpm });
    } catch (error) {
      setResult({ error: error instanceof Error ? error.message : String(error) });
    } finally {
      setBusy(false);
    }
  };
  const buttonClass = 'rounded-md border border-zinc-200 px-2 py-1 text-xs disabled:opacity-50 dark:border-white/10';
  return <div className="my-2 space-y-1">
    <div className="flex flex-wrap gap-2">
      <button type="button" aria-busy={busy} disabled={busy} className={buttonClass} onClick={() => void detect()}>{measure}</button>
      {result && 'bpm' in result && (
        <button type="button" className={buttonClass} onClick={() => onApply(result.bpm)}>{apply.replace('{bpm}', String(result.bpm))}</button>
      )}
    </div>
    <p className="text-[11px] text-zinc-500">{hint}</p>
    {result && 'error' in result && <p role="alert" className="text-xs text-red-600 dark:text-red-300">{result.error}</p>}
  </div>;
}
