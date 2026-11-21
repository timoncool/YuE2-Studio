import { useState } from 'react';
import { useI18n } from '../context/I18nContext';
import { backingHarmony, failed, type BackingHarmonyAnalysis, type BackingSpan } from '../services/scoreApi';

const labels = {
  en: ['Analyze backing harmony', 'Use key {key}', 'Suggested chords', 'Repeated sections', 'Uncertain time ranges are omitted. Review the results against the recording.'],
  ru: ['Разобрать гармонию аккомпанемента', 'Использовать тональность {key}', 'Предполагаемые аккорды', 'Повторяющиеся секции', 'Неуверенные интервалы пропущены. Сверьте результат с записью.'],
  pt: ['Analisar a harmonia do acompanhamento', 'Usar a tonalidade {key}', 'Acordes sugeridos', 'Secções repetidas', 'Os intervalos incertos foram omitidos. Confirme o resultado com a gravação.'],
  zh: ['分析伴奏和声', '使用调性 {key}', '建议和弦', '重复段落', '已省略不确定的时间段。请对照录音确认结果。'],
  ja: ['伴奏の和声を分析', '調 {key} を使う', '推定コード', '繰り返し区間', '不確かな区間は省略しています。録音と照らし合わせて確認してください。'],
  ko: ['반주 화성 분석', '{key} 조성 사용', '추천 코드', '반복 구간', '불확실한 구간은 생략했습니다. 녹음과 결과를 비교하세요.'],
};

function Spans({ title, spans }: { title: string; spans: BackingSpan[] }) {
  if (spans.length === 0) return null;
  return <details className="text-xs">
    <summary>{title}</summary>
    <ul className="max-h-28 overflow-auto tabular-nums">
      {spans.map(span => <li key={span.start_seconds}>{span.start_seconds.toFixed(1)}–{span.end_seconds.toFixed(1)} s · {span.label}</li>)}
    </ul>
  </details>;
}

export function BackingHarmony({ songId, onApplyKey }: { songId: string; onApplyKey: (key: string) => void }) {
  const { language } = useI18n();
  const [analyze, useKey, chords, sections, review] = labels[language];
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<BackingHarmonyAnalysis>();
  const [error, setError] = useState('');
  const detect = async () => {
    setBusy(true);
    setResult(undefined);
    setError('');
    try {
      const answer = await backingHarmony(songId);
      if (failed(answer)) setError(answer.error);
      else setResult(answer.analysis);
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setBusy(false);
    }
  };
  const key = result?.key;
  const buttonClass = 'rounded-md border border-zinc-200 px-2 py-1 text-xs disabled:opacity-50 dark:border-white/10';
  return <div className="my-2 space-y-1">
    <div className="flex flex-wrap gap-2">
      <button type="button" aria-busy={busy} disabled={busy} className={buttonClass} onClick={() => void detect()}>{analyze}</button>
      {key && <button type="button" className={buttonClass} onClick={() => onApplyKey(key)}>{useKey.replace('{key}', key)}</button>}
    </div>
    {result && <>
      <p className="text-[11px] text-zinc-500">{review}</p>
      <Spans title={chords} spans={result.chords} />
      <Spans title={sections} spans={result.sections} />
    </>}
    {error && <p role="alert" className="text-xs text-red-600 dark:text-red-300">{error}</p>}
  </div>;
}
