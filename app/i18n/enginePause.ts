import type { Language } from './translations';

const en = { pause: 'Pause YuE2 engine', resume: 'Resume YuE2 engine', paused: 'YuE2 engine paused', failed: 'Could not change the YuE2 engine state.' };
export const enginePauseStrings: Record<Language, typeof en> = {
  en,
  ru: { pause: 'Приостановить движок YuE2', resume: 'Возобновить движок YuE2', paused: 'Движок YuE2 на паузе', failed: 'Не удалось изменить состояние движка YuE2.' },
  zh: { pause: '暂停 YuE2 引擎', resume: '恢复 YuE2 引擎', paused: 'YuE2 引擎已暂停', failed: '无法更改 YuE2 引擎状态。' },
  ja: { pause: 'YuE2 エンジンを一時停止', resume: 'YuE2 エンジンを再開', paused: 'YuE2 エンジンは一時停止中', failed: 'YuE2 エンジンの状態を変更できませんでした。' },
  ko: { pause: 'YuE2 엔진 일시 정지', resume: 'YuE2 엔진 다시 시작', paused: 'YuE2 엔진 일시 정지됨', failed: 'YuE2 엔진 상태를 변경할 수 없습니다.' },
  pt: { pause: 'Pausar motor YuE2', resume: 'Retomar motor YuE2', paused: 'Motor YuE2 em pausa', failed: 'Não foi possível alterar o estado do motor YuE2.' },
};
