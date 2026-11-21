import type { Language } from './translations';

const en = {
  compare: 'Compare takes',
  choose: 'Mark as preferred take',
  preferred: 'Preferred take',
  openInLibrary: 'Open in Library',
  pausePlayer: 'Pause the library player to compare takes.',
};

export const preferredTakeStrings: Record<Language, typeof en> = {
  en,
  ru: { openInLibrary: 'Открыть в библиотеке', compare: 'Сравнить дубли', choose: 'Отметить как лучший дубль', preferred: 'Лучший дубль', pausePlayer: 'Приостановите плеер библиотеки, чтобы сравнить дубли.' },
  zh: { openInLibrary: '在媒体库中打开', compare: '比较录音版本', choose: '标为首选版本', preferred: '首选版本', pausePlayer: '请暂停媒体库播放器以比较录音版本。' },
  ja: { openInLibrary: 'ライブラリで開く', compare: 'テイクを比較', choose: '優先テイクに設定', preferred: '優先テイク', pausePlayer: 'テイクを比較するにはライブラリの再生を一時停止してください。' },
  ko: { openInLibrary: '라이브러리에서 열기', compare: '테이크 비교', choose: '선호 테이크로 표시', preferred: '선호 테이크', pausePlayer: '테이크를 비교하려면 라이브러리 재생을 일시 중지하세요.' },
  pt: { openInLibrary: 'Abrir na biblioteca', compare: 'Comparar versões', choose: 'Marcar como versão preferida', preferred: 'Versão preferida', pausePlayer: 'Coloque o leitor da biblioteca em pausa para comparar versões.' },
};
