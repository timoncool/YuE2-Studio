import type { Language } from './translations';

type Hints = Record<string, string>;

// What each generation setting gives the listener and when to change it (design-audit rules R15-R16).
const EN: Hints = {
  duration_seconds: 'How long the song is. Leave it empty for the model\'s usual length.',
  steps: 'More steps take longer and polish the sound; fewer are faster and rougher.',
  seed: 'The same number with the same settings repeats the song. Change it for a new take.',
  guidance_scale: 'How closely the music follows the description: higher keeps closer to it, lower lets the model improvise.',
  lm_seed: 'Repeats the song\'s plan, its structure and melody, when the number stays the same.',
  lm_cfg: 'How closely the song\'s plan follows the description: higher keeps closer, lower is freer.',
  lm_top_k: 'How many likely continuations the plan picks from: fewer is safer, more is more varied.',
  dit_cfg: 'How closely the sound follows the plan: higher keeps closer, lower lets it drift.',
  fade_in: 'Seconds of fade-in at the start of the song.',
  fade_out: 'Seconds of fade-out at the end of the song.',
};

const RU: Hints = {
  duration_seconds: 'Длина песни. Пусто — обычная длина для этой модели.',
  steps: 'Больше шагов — дольше и чище звук; меньше — быстрее и грубее.',
  seed: 'То же число с теми же настройками повторит песню. Смените его ради нового дубля.',
  guidance_scale: 'Насколько музыка держится описания: выше — ближе к нему, ниже — модель импровизирует.',
  lm_seed: 'Повторяет план песни — структуру и мелодию, пока число то же.',
  lm_cfg: 'Насколько план песни держится описания: выше — ближе, ниже — свободнее.',
  lm_top_k: 'Из скольких вероятных продолжений выбирает план: меньше — надёжнее, больше — разнообразнее.',
  dit_cfg: 'Насколько звук держится плана: выше — ближе, ниже — свободнее.',
  fade_in: 'Секунды плавного нарастания в начале песни.',
  fade_out: 'Секунды плавного затухания в конце песни.',
};

const ZH: Hints = {
  duration_seconds: '歌曲时长。留空则使用该模型的常规时长。',
  steps: '步数越多越慢、声音越精细；越少越快、越粗糙。',
  seed: '相同数字配合相同设置会重现这首歌。换个数字可得到新版本。',
  guidance_scale: '音乐贴合描述的程度：越高越贴近，越低模型越自由发挥。',
  lm_seed: '数字不变时，重现歌曲的规划，包括结构和旋律。',
  lm_cfg: '歌曲规划贴合描述的程度：越高越贴近，越低越自由。',
  lm_top_k: '规划从多少个可能的续写中选择：越少越稳妥，越多越多样。',
  dit_cfg: '声音贴合规划的程度：越高越贴近，越低越自由。',
  fade_in: '歌曲开头淡入的秒数。',
  fade_out: '歌曲结尾淡出的秒数。',
};

const JA: Hints = {
  duration_seconds: '曲の長さです。空欄ならこのモデルの標準の長さになります。',
  steps: 'ステップが多いほど時間がかかり音が整います。少ないほど速く粗くなります。',
  seed: '同じ数と同じ設定なら同じ曲になります。新しいテイクには数を変えてください。',
  guidance_scale: '音楽が説明にどれだけ沿うか。高いほど忠実に、低いほどモデルが自由に作ります。',
  lm_seed: '数が同じなら、曲の構成とメロディの計画を再現します。',
  lm_cfg: '曲の計画が説明にどれだけ沿うか。高いほど忠実に、低いほど自由になります。',
  lm_top_k: '計画がいくつの候補から選ぶか。少ないほど安定し、多いほど多彩になります。',
  dit_cfg: '音が計画にどれだけ沿うか。高いほど忠実に、低いほど自由になります。',
  fade_in: '曲の冒頭のフェードインの秒数です。',
  fade_out: '曲の終わりのフェードアウトの秒数です。',
};

const KO: Hints = {
  duration_seconds: '곡의 길이입니다. 비워 두면 이 모델의 보통 길이가 됩니다.',
  steps: '단계가 많을수록 오래 걸리고 소리가 다듬어지며, 적을수록 빠르고 거칩니다.',
  seed: '같은 숫자와 같은 설정이면 같은 곡이 나옵니다. 새 테이크를 원하면 숫자를 바꾸세요.',
  guidance_scale: '음악이 설명을 얼마나 따르는지입니다. 높을수록 가깝고, 낮을수록 모델이 자유롭게 만듭니다.',
  lm_seed: '숫자가 같으면 곡의 구조와 멜로디 계획을 그대로 반복합니다.',
  lm_cfg: '곡 계획이 설명을 얼마나 따르는지입니다. 높을수록 가깝고, 낮을수록 자유롭습니다.',
  lm_top_k: '계획이 몇 개의 후보 중에서 고르는지입니다. 적을수록 안정적이고, 많을수록 다양합니다.',
  dit_cfg: '소리가 계획을 얼마나 따르는지입니다. 높을수록 가깝고, 낮을수록 자유롭습니다.',
  fade_in: '곡 시작의 페이드 인 길이(초)입니다.',
  fade_out: '곡 끝의 페이드 아웃 길이(초)입니다.',
};

const PT: Hints = {
  duration_seconds: 'A duração da música. Vazio usa a duração habitual deste modelo.',
  steps: 'Mais passos demoram mais e apuram o som; menos são mais rápidos e mais crus.',
  seed: 'O mesmo número com as mesmas definições repete a música. Mude-o para uma nova versão.',
  guidance_scale: 'Quanto a música segue a descrição: mais alto fica mais perto, mais baixo deixa o modelo improvisar.',
  lm_seed: 'Repete o plano da música, a estrutura e a melodia, enquanto o número for o mesmo.',
  lm_cfg: 'Quanto o plano da música segue a descrição: mais alto fica mais perto, mais baixo é mais livre.',
  lm_top_k: 'Entre quantas continuações prováveis o plano escolhe: menos é mais seguro, mais é mais variado.',
  dit_cfg: 'Quanto o som segue o plano: mais alto fica mais perto, mais baixo é mais livre.',
  fade_in: 'Segundos de entrada gradual no início da música.',
  fade_out: 'Segundos de saída gradual no fim da música.',
};

const BY_LANGUAGE: Record<Language, Hints> = { en: EN, ru: RU, zh: ZH, ja: JA, ko: KO, pt: PT };

export function generationHint(key: string, language: Language): string | undefined {
  return BY_LANGUAGE[language]?.[key] ?? EN[key];
}

export const randomSeedPlaceholder: Record<Language, string> = {
  en: 'Random', ru: 'Случайно', zh: '随机', ja: 'ランダム', ko: '무작위', pt: 'Aleatório',
};
