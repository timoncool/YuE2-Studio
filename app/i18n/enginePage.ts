import type { Language } from './translations';

export const engineUpdateTarget = 'engines:updates';

export const runtimeSourceStrings: Record<Language, readonly [string, string]> = {
  en: ['Managed package', 'Selected local executable'],
  ru: ['Управляемый пакет', 'Выбранный локальный файл'],
  zh: ['托管软件包', '选中的本地可执行文件'],
  ja: ['管理パッケージ', '選択したローカル実行ファイル'],
  ko: ['관리 패키지', '선택한 로컬 실행 파일'],
  pt: ['Pacote gerido', 'Executável local selecionado'],
};

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
export const engineCardStrings: Record<Language, {
  details: string;
  retry: string;
  offline: string;
  modelsReady: string;
  modelsMissing: string;
  models: (missing: number) => string;
  unsupported: string;
}> = {
  en: { details: 'Details', retry: 'Try again', offline: 'Could not check for a newer download list: the site did not answer. The saved list is in use.', modelsReady: 'Models downloaded', modelsMissing: 'Models not downloaded', models: missing => `Models · ${missing} to download`, unsupported: 'Not available' },
  ru: { details: 'Подробно', retry: 'Повторить', offline: 'Не удалось проверить, есть ли список загрузок новее: сайт не ответил. Работает сохранённый список.', modelsReady: 'Модели скачаны', modelsMissing: 'Модели не скачаны', models: missing => `Модели · ${missing} можно скачать`, unsupported: 'Недоступен' },
  zh: { details: '详细信息', retry: '重试', offline: '无法检查是否有更新的下载列表：网站没有响应。正在使用已保存的列表。', modelsReady: '模型已下载', modelsMissing: '模型未下载', models: missing => `模型 · ${missing} 个可下载`, unsupported: '不可用' },
  ja: { details: '詳細', retry: '再試行', offline: '新しいダウンロード一覧を確認できませんでした：サイトが応答しませんでした。保存済みの一覧を使用しています。', modelsReady: 'モデルはダウンロード済み', modelsMissing: 'モデル未ダウンロード', models: missing => `モデル · ${missing} 件ダウンロード可能`, unsupported: '利用不可' },
  ko: { details: '자세히', retry: '다시 시도', offline: '더 새로운 다운로드 목록을 확인하지 못했습니다: 사이트가 응답하지 않았습니다. 저장된 목록을 사용합니다.', modelsReady: '모델 다운로드됨', modelsMissing: '모델 미다운로드', models: missing => `모델 · ${missing}개 다운로드 가능`, unsupported: '사용 불가' },
  pt: { details: 'Detalhes', retry: 'Tentar de novo', offline: 'Não foi possível verificar se há uma lista de downloads mais recente: o site não respondeu. A lista guardada está em uso.', modelsReady: 'Modelos baixados', modelsMissing: 'Modelos não baixados', models: missing => `Modelos · ${missing} para baixar`, unsupported: 'Indisponível' },
};

export const enginePageStrings: Record<Language, readonly [string, string, string, string, string, string, string, string]> = {
  en: ['Engines and updates', 'Active', 'Installed', 'Not installed', 'Checking…', 'Status unavailable', 'Manage models', 'Engine settings'],
  ru: ['Движки и обновления', 'Активен', 'Установлен', 'Не установлен', 'Проверка…', 'Статус недоступен', 'Управление моделями', 'Настройки движка'],
  zh: ['引擎和更新', '运行中', '已安装', '未安装', '正在检查…', '无法获取状态', '管理模型', '引擎设置'],
  ja: ['エンジンと更新', '稼働中', 'インストール済み', '未インストール', '確認中…', '状態を確認できません', 'モデルを管理', 'エンジン設定'],
  ko: ['엔진 및 업데이트', '활성', '설치됨', '미설치', '확인 중…', '상태를 확인할 수 없음', '모델 관리', '엔진 설정'],
  pt: ['Motores e atualizações', 'Ativo', 'Instalado', 'Não instalado', 'A verificar…', 'Estado indisponível', 'Gerir modelos', 'Definições do motor'],
};
