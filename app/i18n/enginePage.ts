import type { Language } from './translations';

export const engineUpdateTarget = 'engines:updates';

export const showUnsupportedEngines: Record<Language, string> = {
  en: 'Show unsupported engines',
  ru: 'Показать несовместимые движки',
  zh: '显示不兼容的引擎',
  ja: '非対応のエンジンを表示',
  ko: '지원되지 않는 엔진 표시',
  pt: 'Mostrar motores incompatíveis',
};


/** The component catalogue line: the list number and date, when it was last checked, and a failed check. */
export const componentCatalogueStrings: Record<Language, {
  list: (sequence: number, date: string) => string;
  builtIn: string;
  checked: (when: string) => string;
  failed: (reason: string) => string;
}> = {
  en: {
    list: (sequence, date) => `Download list no. ${sequence} of ${date}`,
    builtIn: 'the one built into this version',
    checked: when => `checked ${when}`,
    failed: reason => `A newer list could not be checked: ${reason}. The saved list is in use.`,
  },
  ru: {
    list: (sequence, date) => `Список загрузок № ${sequence} от ${date}`,
    builtIn: 'встроен в эту версию',
    checked: when => `проверен ${when}`,
    failed: reason => `Не удалось проверить, есть ли список новее: ${reason}. Работает сохранённый.`,
  },
  zh: {
    list: (sequence, date) => `下载列表 第 ${sequence} 版（${date}）`,
    builtIn: '本版本内置',
    checked: when => `检查于 ${when}`,
    failed: reason => `无法检查是否有更新的列表：${reason}。正在使用已保存的列表。`,
  },
  ja: {
    list: (sequence, date) => `ダウンロード一覧 No.${sequence}（${date}）`,
    builtIn: 'このバージョンに内蔵',
    checked: when => `${when} に確認`,
    failed: reason => `新しい一覧を確認できませんでした: ${reason}。保存済みの一覧を使用しています。`,
  },
  ko: {
    list: (sequence, date) => `다운로드 목록 ${sequence}번 (${date})`,
    builtIn: '이 버전에 내장',
    checked: when => `${when} 확인`,
    failed: reason => `더 새로운 목록을 확인하지 못했습니다: ${reason}. 저장된 목록을 사용합니다.`,
  },
  pt: {
    list: (sequence, date) => `Lista de downloads n.º ${sequence} de ${date}`,
    builtIn: 'incluída nesta versão',
    checked: when => `verificada ${when}`,
    failed: reason => `Não foi possível verificar se há uma lista mais recente: ${reason}. A lista guardada está em uso.`,
  },
};

/** The engine cards: their details, the models line, and a catalogue check that could not reach the site. */
export const engineCardStrings: Record<Language, { retry: string; offline: string; unsupported: string }> = {
  en: { retry: 'Try again', offline: 'Could not check for a newer download list: the site did not answer. The saved list is in use.', unsupported: 'Not available' },
  ru: { retry: 'Повторить', offline: 'Не удалось проверить, есть ли список загрузок новее: сайт не ответил. Работает сохранённый список.', unsupported: 'Недоступен' },
  zh: { retry: '重试', offline: '无法检查是否有更新的下载列表：网站没有响应。正在使用已保存的列表。', unsupported: '不可用' },
  ja: { retry: '再試行', offline: '新しいダウンロード一覧を確認できませんでした：サイトが応答しませんでした。保存済みの一覧を使用しています。', unsupported: '利用不可' },
  ko: { retry: '다시 시도', offline: '더 새로운 다운로드 목록을 확인하지 못했습니다: 사이트가 응답하지 않았습니다. 저장된 목록을 사용합니다.', unsupported: '사용 불가' },
  pt: { retry: 'Tentar de novo', offline: 'Não foi possível verificar se há uma lista de downloads mais recente: o site não respondeu. A lista guardada está em uso.', unsupported: 'Indisponível' },
};

export const enginePageStrings: Record<Language, readonly [string, string, string]> = {
  en: ['Models and engines', 'Checking…', 'Status unavailable'],
  ru: ['Модели и движки', 'Проверка…', 'Статус недоступен'],
  zh: ['模型和引擎', '正在检查…', '无法获取状态'],
  ja: ['モデルとエンジン', '確認中…', '状態を確認できません'],
  ko: ['모델과 엔진', '확인 중…', '상태를 확인할 수 없음'],
  pt: ['Modelos e motores', 'A verificar…', 'Estado indisponível'],
};
