import React from 'react';
import { useI18n } from '../context/I18nContext';
import { FieldLabel } from './kit/FieldLabel';

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
    examples: { caption: 'e.g. A rainy street at night', music: 'e.g. Slow blues guitar', speech_text: 'e.g. Thanks for stopping by', speech_style: 'e.g. Warm, tired male voice', effects: 'e.g. Coins in a guitar case', environment: 'e.g. Rain, distant traffic' },
  },
  ru: {
    caption: 'Описание сцены', music: 'Музыка', speech_text: 'Текст речи', speech_style: 'Голос и манера речи',
    effects: 'Звуковые эффекты', environment: 'Звуки окружения', speechOmitted: 'Оставьте текст пустым для генерации без речи.',
    examples: { caption: 'Например: ночная улица под дождём', music: 'Например: неспешный блюз на гитаре', speech_text: 'Например: спасибо, что остановились', speech_style: 'Например: тёплый усталый мужской голос', effects: 'Например: монеты падают в чехол', environment: 'Например: дождь, далёкие машины' },
  },
  zh: {
    caption: '场景描述', music: '音乐', speech_text: '台词', speech_style: '声音与表达',
    effects: '音效', environment: '环境声', speechOmitted: '台词留空时不指定语音。',
    examples: { caption: '例如：雨夜的街道', music: '例如：舒缓的布鲁斯吉他', speech_text: '例如：谢谢驻足', speech_style: '例如：温暖而疲惫的男声', effects: '例如：硬币落入琴盒', environment: '例如：雨声，远处车流' },
  },
  ja: {
    caption: 'シーンの説明', music: '音楽', speech_text: '台詞', speech_style: '声と話し方',
    effects: '効果音', environment: '環境音', speechOmitted: '台詞を空欄にすると音声の指示を省きます。',
    examples: { caption: '例：雨の夜の通り', music: '例：ゆったりしたブルースギター', speech_text: '例：立ち止まってくれてありがとう', speech_style: '例：温かく疲れた男性の声', effects: '例：ケースに落ちる硬貨', environment: '例：雨、遠くの車' },
  },
  ko: {
    caption: '장면 설명', music: '음악', speech_text: '대사', speech_style: '목소리와 말투',
    effects: '효과음', environment: '환경음', speechOmitted: '대사를 비워 두면 음성 지시를 생략합니다.',
    examples: { caption: '예: 비 오는 밤거리', music: '예: 느린 블루스 기타', speech_text: '예: 들러 주셔서 고마워요', speech_style: '예: 따뜻하고 지친 남성 목소리', effects: '예: 케이스에 떨어지는 동전', environment: '예: 빗소리, 먼 차 소리' },
  },
  pt: {
    caption: 'Descrição da cena', music: 'Música', speech_text: 'Texto falado', speech_style: 'Voz e interpretação',
    effects: 'Efeitos sonoros', environment: 'Som ambiente', speechOmitted: 'Deixe o texto em branco para omitir a fala.',
    examples: { caption: 'Ex.: rua à noite com chuva', music: 'Ex.: blues lento de violão', speech_text: 'Ex.: obrigado por pararem', speech_style: 'Ex.: voz masculina quente e cansada', effects: 'Ex.: moedas no estojo', environment: 'Ex.: chuva, trânsito ao longe' },
  },
};

export function MiDashengFields({ value, onChange, controlClassName }: { value: MiDashengPrompt; onChange: (value: MiDashengPrompt) => void; controlClassName: string }) {
  const { language } = useI18n();
  const text = labels[language] ?? labels.en;

  return <div className="grid gap-3">
    {fields.map(key => (
      <label key={key} className="grid text-xs text-zinc-700 dark:text-zinc-300">
        <FieldLabel>{text[key]}</FieldLabel>
        <textarea rows={2} value={value[key]} placeholder={text.examples[key]} onChange={event => onChange({ ...value, [key]: event.target.value })}
          className={`${controlClassName} resize-y`} />
        {key === 'speech_text' && <span className="mt-1 text-zinc-500 dark:text-zinc-400">{text.speechOmitted}</span>}
      </label>
    ))}
  </div>;
}
