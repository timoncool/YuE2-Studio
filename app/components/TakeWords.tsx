import { useEffect, useId, useMemo, useRef, useState } from 'react';
import { useI18n } from '../context/I18nContext';
import { karaokeReason, recogniseLyrics } from '../services/karaoke';
import { sungWords, wordMatchPercent } from '../services/takeWordMatch';
import type { Song } from '../types';

const copy = {
  en: ['Compare sung words', 'Uses the karaoke recogniser for both recordings. A cloud recogniser sends both recordings to the provider. The reference is the lyrics of take A.', 'Recognising take', 'Word match', 'No score'],
  ru: ['Сравнить спетые слова', 'Оба дубля обработает распознаватель караоке. При выборе облачного сервиса он получит обе записи. Образец для сравнения — текст дубля A.', 'Распознаётся дубль', 'Совпадение с текстом', 'Нет оценки'],
  zh: ['比较演唱的歌词', '使用卡拉 OK 识别器处理两个版本。云端识别器会将两份录音发送给服务商。参照文本为 A 的歌词。', '正在识别', '歌词匹配度', '无评分'],
  ja: ['歌詞の一致度を比較', 'カラオケの認識器で両方の音源を処理します。クラウド設定時は両方の録音が送信されます。基準はテイク A の歌詞です。', '認識中のテイク', '歌詞の一致度', '評価なし'],
  ko: ['부른 가사 비교', '노래방 인식기로 두 녹음을 처리합니다. 클라우드 인식기는 두 녹음을 서비스에 전송합니다. 기준은 테이크 A의 가사입니다.', '인식 중인 테이크', '가사 일치도', '점수 없음'],
  pt: ['Comparar palavras cantadas', 'O reconhecedor de karaokê processa as duas gravações. Se for usado um serviço na nuvem, ambas serão enviadas ao provedor. A referência é a letra da versão A.', 'Reconhecendo versão', 'Correspondência com a letra', 'Sem pontuação'],
};

export function TakeWords({ left, right }: { left: Song; right: Song }) {
  const { language, t } = useI18n();
  const [compare, hint, working, match, unavailable] = copy[language];
  const descriptionId = useId();
  const [phase, setPhase] = useState<'' | 'A' | 'B'>('');
  const [result, setResult] = useState<(number | null)[] | null>(null);
  const [error, setError] = useState('');
  const request = useRef(0);
  const busy = useRef(false);
  useEffect(() => () => { request.current++; }, []);
  const reference = useMemo(() => sungWords(left.lyrics ?? ''), [left.lyrics]);
  if (!left.audioUrl || !right.audioUrl || !reference.length) return null;

  const run = async () => {
    if (busy.current) return;
    busy.current = true;
    const ticket = ++request.current;
    setError('');
    setResult(null);
    try {
      const scores = [];
      for (const [side, song] of [['A', left], ['B', right]] as const) {
        setPhase(side);
        const heard = await recogniseLyrics({ songId: song.id }).catch((cause: unknown) => {
          if (cause instanceof Error && cause.message === 'karaoke.no-words') return '';
          throw cause;
        });
        if (ticket !== request.current) return;
        scores.push(wordMatchPercent(reference, sungWords(heard)));
      }
      setResult(scores);
    } catch (cause) {
      if (ticket === request.current) {
        const reason = cause instanceof Error ? cause.message : String(cause);
        setError(karaokeReason(t, reason) || unavailable);
      }
    } finally {
      if (ticket === request.current) { busy.current = false; setPhase(''); }
    }
  };

  return <div className="px-2 py-2 text-sm">
    <p id={`${descriptionId}-hint`} className="text-zinc-400">{hint}</p>
    <button type="button" aria-describedby={`${descriptionId}-hint ${descriptionId}-status`} disabled={!!phase} onClick={() => void run()}
      className="my-1 rounded border border-white/15 px-2 py-1 enabled:hover:bg-white/10">{compare}</button>
    <p role="status" id={`${descriptionId}-status`}>{phase ? `${working} ${phase}…` : error || (result &&
      `${match} · ${result.map((score, i) => `${i ? 'B' : 'A'}: ${score === null ? unavailable : score + '%'}`).join(' · ')}`)}</p>
  </div>;
}
