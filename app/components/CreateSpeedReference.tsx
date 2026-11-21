import benchmark from '../../docs/music-gpu-benchmarks.json';
import { useI18n } from '../context/I18nContext';

const referenceIds = {
  'yue2-quality-q8': [
    'yue2-892b9b0-bench-CUDA0-from-lyrics-cublas-13.5.1.27-30s-summary.json',
    'yue2-892b9b0-bench-Vulkan0-from-lyrics-30s-summary.json',
  ],
  'minimax-minimal': [
    'minimax-46c4c90-rtx-4090/timing-cuda13-minimal/mm3-46c4c90-bench-CUDA0-cublas-13.5.1.27-3.2s-summary.json',
    'minimax-46c4c90-rtx-4090/timing-vulkan-minimal/mm3-46c4c90-bench-Vulkan0-3.2s-summary.json',
  ],
  'minimax-quality-q8': [
    'minimax-46c4c90-rtx-4090/timing-cuda13-quality/mm3-46c4c90-bench-CUDA0-quality-q8-cublas-13.5.1.27-3.2s-summary.json',
    'minimax-46c4c90-rtx-4090/timing-vulkan-quality/mm3-46c4c90-bench-Vulkan0-quality-q8-3.2s-summary.json',
  ],
  'audio-cpp-minimax-q8': [
    'music-gpu-20261121a/minimax_music3-legacy-q8-cuda13/summary.json',
    'music-gpu-20261121a/minimax_music3-legacy-q8-vulkan/summary.json',
  ],
} as const;

const strings = {
  en: { heading: 'Reference speed on RTX 4090 · Windows 11', median: 'Median of three warm runs', unit: 'audio s/s' },
  ru: { heading: 'Замер на RTX 4090 · Windows 11', median: 'Медиана трёх прогретых запусков', unit: 'с аудио/с' },
  zh: { heading: 'RTX 4090 参考速度 · Windows 11', median: '三次热运行的中位数', unit: '音频秒/秒' },
  ja: { heading: 'RTX 4090 の参考速度 · Windows 11', median: 'ウォーム実行 3 回の中央値', unit: '音声秒/秒' },
  ko: { heading: 'RTX 4090 기준 속도 · Windows 11', median: '웜 실행 3회의 중앙값', unit: '오디오 초/초' },
  pt: { heading: 'Velocidade de referência na RTX 4090 · Windows 11', median: 'Mediana de três execuções aquecidas', unit: 's de áudio/s' },
};

export type CreateSpeedProfile = keyof typeof referenceIds;

export function CreateSpeedReference({ profile, id }: { profile: CreateSpeedProfile; id: string }) {
  const { language } = useI18n();
  const labels = strings[language];
  const ids: readonly string[] = referenceIds[profile];
  const rows = benchmark.rows.filter(row => ids.includes(row.summary_id));
  const sample = rows[0];
  if (!sample) return null;
  const format = new Intl.NumberFormat(language, { maximumFractionDigits: 2 });
  return <div id={id} className="space-y-0.5 rounded border border-zinc-200 px-2 py-1.5 text-[11px] text-zinc-600 dark:border-white/10 dark:text-zinc-300">
    <p className="font-medium">{labels.heading}</p>
    <p>{sample.weight_profile} · {sample.workload} · {sample.revision}</p>
    <p>{labels.median} · {rows.map(row => (
      <span key={row.summary_id} className="mr-2 inline-block">{row.backend} {format.format(row.warm_audio_seconds_per_second)} {labels.unit}</span>
    ))}</p>
  </div>;
}
