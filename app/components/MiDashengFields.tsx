import React from 'react';
import { useI18n } from '../context/I18nContext';

const fields = ['caption', 'music', 'speech_text', 'speech_style', 'effects', 'environment'] as const;
export type MiDashengPrompt = Record<(typeof fields)[number], string>;

export const miDashengModel = 'midashenglm_gen';

export function restoreMiDashengPrompt(saved: unknown): MiDashengPrompt {
  const source = saved && typeof saved === 'object' ? saved as Record<string, unknown> : {};
  return Object.fromEntries(fields.map(key => [key, typeof source[key] === 'string' ? source[key] : ''])) as MiDashengPrompt;
}

export function miDashengPromptReady(prompt: MiDashengPrompt): boolean {
  return fields.some(key => key !== 'speech_style' && prompt[key].trim() !== '');
}

const labels = {
  en: {
    caption: 'Scene summary', music: 'Music', speech_text: 'Spoken words', speech_style: 'Voice and delivery',
    effects: 'Sound effects', environment: 'Ambience', speechOmitted: 'Leave spoken words empty to omit speech.',
  },
  ru: {
    caption: 'Описание сцены', music: 'Музыка', speech_text: 'Текст речи', speech_style: 'Голос и манера речи',
    effects: 'Звуковые эффекты', environment: 'Звуки окружения', speechOmitted: 'Оставьте текст пустым для генерации без речи.',
  },
  zh: {
    caption: '场景描述', music: '音乐', speech_text: '台词', speech_style: '声音与表达',
    effects: '音效', environment: '环境声', speechOmitted: '台词留空时不指定语音。',
  },
  ja: {
    caption: 'シーンの説明', music: '音楽', speech_text: '台詞', speech_style: '声と話し方',
    effects: '効果音', environment: '環境音', speechOmitted: '台詞を空欄にすると音声の指示を省きます。',
  },
  ko: {
    caption: '장면 설명', music: '음악', speech_text: '대사', speech_style: '목소리와 말투',
    effects: '효과음', environment: '환경음', speechOmitted: '대사를 비워 두면 음성 지시를 생략합니다.',
  },
  pt: {
    caption: 'Descrição da cena', music: 'Música', speech_text: 'Texto falado', speech_style: 'Voz e interpretação',
    effects: 'Efeitos sonoros', environment: 'Som ambiente', speechOmitted: 'Deixe o texto em branco para omitir a fala.',
  },
};

export function MiDashengFields({ value, onChange }: { value: MiDashengPrompt; onChange: (value: MiDashengPrompt) => void }) {
  const { language } = useI18n();
  const text = labels[language] ?? labels.en;

  return <div className="grid gap-3">
    {fields.map(key => (
      <label key={key} className="grid gap-1 text-xs text-zinc-700 dark:text-zinc-300">
        <span>{text[key]}</span>
        <textarea rows={2} value={value[key]} onChange={event => onChange({ ...value, [key]: event.target.value })}
          className="w-full resize-y rounded-lg border border-zinc-200 bg-zinc-50 px-3 py-2 text-sm outline-hidden focus:border-pink-500 dark:border-white/10 dark:bg-black/25" />
        {key === 'speech_text' && <span className="text-zinc-500">{text.speechOmitted}</span>}
      </label>
    ))}
  </div>;
}
