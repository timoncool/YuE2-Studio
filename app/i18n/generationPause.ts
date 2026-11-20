import type { Language } from './translations';

const en = {
  label: 'Pause between generations',
  none: 'No pause',
  fixed: 'Fixed',
  random: 'Random range',
  minutes: 'Pause in minutes',
  minimum: 'Minimum pause in minutes',
  maximum: 'Maximum pause in minutes',
  unit: 'min',
  invalid: 'Enter a valid pause of zero or more minutes, with the minimum no higher than the maximum.',
};

export const generationPauseStrings: Record<Language, typeof en> = {
  en,
  ru: { label: 'Пауза между генерациями', none: 'Без паузы', fixed: 'Фиксированная', random: 'Случайный интервал', minutes: 'Пауза в минутах', minimum: 'Минимальная пауза в минутах', maximum: 'Максимальная пауза в минутах', unit: 'мин', invalid: 'Укажите паузу от нуля минут. Минимум должен быть не больше максимума.' },
  zh: { label: '生成间隔', none: '无间隔', fixed: '固定间隔', random: '随机范围', minutes: '间隔分钟数', minimum: '最短间隔分钟数', maximum: '最长间隔分钟数', unit: '分钟', invalid: '请输入非负间隔。最短间隔不能超过最长间隔。' },
  ja: { label: '生成の間隔', none: '間隔なし', fixed: '固定', random: 'ランダムな範囲', minutes: '間隔（分）', minimum: '最短間隔（分）', maximum: '最長間隔（分）', unit: '分', invalid: '0分以上の間隔を入力してください。最短間隔は最長間隔以下にしてください。' },
  ko: { label: '생성 사이 대기 시간', none: '대기 없음', fixed: '고정', random: '무작위 범위', minutes: '대기 시간(분)', minimum: '최소 대기 시간(분)', maximum: '최대 대기 시간(분)', unit: '분', invalid: '0분 이상의 대기 시간을 입력하세요. 최소값은 최대값보다 클 수 없습니다.' },
  pt: { label: 'Pausa entre gerações', none: 'Sem pausa', fixed: 'Fixa', random: 'Intervalo aleatório', minutes: 'Pausa em minutos', minimum: 'Pausa mínima em minutos', maximum: 'Pausa máxima em minutos', unit: 'min', invalid: 'Indique uma pausa de zero ou mais minutos. O mínimo não pode superar o máximo.' },
};
