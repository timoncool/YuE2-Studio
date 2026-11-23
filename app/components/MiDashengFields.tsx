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
    examples: { caption: 'A rainy night street, a guitarist busks under an awning', music: 'Slow fingerpicked blues guitar', speech_text: 'Thanks for stopping by, friends', speech_style: 'Warm, tired male voice, close to the mic', effects: 'Coins dropping into a guitar case', environment: 'Steady rain, distant traffic' },
  },
  ru: {
    caption: 'Описание сцены', music: 'Музыка', speech_text: 'Текст речи', speech_style: 'Голос и манера речи',
    effects: 'Звуковые эффекты', environment: 'Звуки окружения', speechOmitted: 'Оставьте текст пустым для генерации без речи.',
    examples: { caption: 'Дождливая ночная улица, под навесом играет уличный гитарист', music: 'Неспешный блюз на гитаре пальцами', speech_text: 'Спасибо, что остановились, друзья', speech_style: 'Тёплый усталый мужской голос, близко к микрофону', effects: 'Монеты падают в чехол гитары', environment: 'Ровный дождь, далёкий шум машин' },
  },
  zh: {
    caption: '场景描述', music: '音乐', speech_text: '台词', speech_style: '声音与表达',
    effects: '音效', environment: '环境声', speechOmitted: '台词留空时不指定语音。',
    examples: { caption: '雨夜的街道，一位吉他手在雨棚下街头演奏', music: '舒缓的指弹布鲁斯吉他', speech_text: '谢谢大家驻足', speech_style: '温暖而疲惫的男声，贴近麦克风', effects: '硬币落入吉他盒', environment: '持续的雨声，远处的车流' },
  },
  ja: {
    caption: 'シーンの説明', music: '音楽', speech_text: '台詞', speech_style: '声と話し方',
    effects: '効果音', environment: '環境音', speechOmitted: '台詞を空欄にすると音声の指示を省きます。',
    examples: { caption: '雨の夜の通り、軒下でギタリストが路上演奏している', music: 'ゆったりしたフィンガーピッキングのブルースギター', speech_text: '立ち止まってくれてありがとう', speech_style: '温かく少し疲れた男性の声、マイクに近い', effects: 'ギターケースに落ちる硬貨', environment: '降り続く雨、遠くの車の音' },
  },
  ko: {
    caption: '장면 설명', music: '음악', speech_text: '대사', speech_style: '목소리와 말투',
    effects: '효과음', environment: '환경음', speechOmitted: '대사를 비워 두면 음성 지시를 생략합니다.',
    examples: { caption: '비 오는 밤거리, 차양 아래에서 기타리스트가 버스킹한다', music: '느린 핑거피킹 블루스 기타', speech_text: '들러 주셔서 고마워요, 여러분', speech_style: '따뜻하고 지친 남성 목소리, 마이크 가까이', effects: '기타 케이스에 떨어지는 동전', environment: '꾸준한 빗소리, 먼 차 소리' },
  },
  pt: {
    caption: 'Descrição da cena', music: 'Música', speech_text: 'Texto falado', speech_style: 'Voz e interpretação',
    effects: 'Efeitos sonoros', environment: 'Som ambiente', speechOmitted: 'Deixe o texto em branco para omitir a fala.',
    examples: { caption: 'Uma rua numa noite de chuva, um guitarrista toca sob um toldo', music: 'Blues lento de violão dedilhado', speech_text: 'Obrigado por pararem, amigos', speech_style: 'Voz masculina quente e cansada, perto do microfone', effects: 'Moedas caindo no estojo do violão', environment: 'Chuva constante, trânsito ao longe' },
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
        {key === 'speech_text' && <span className="text-zinc-500">{text.speechOmitted}</span>}
      </label>
    ))}
  </div>;
}
