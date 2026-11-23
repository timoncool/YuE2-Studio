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

export const enginePageStrings: Record<Language, readonly [string, string, string, string, string, string, string, string]> = {
  en: ['Engines and updates', 'Active', 'Installed', 'Not installed', 'Checking…', 'Status unavailable', 'Manage models', 'Engine settings'],
  ru: ['Движки и обновления', 'Активен', 'Установлен', 'Не установлен', 'Проверка…', 'Статус недоступен', 'Управление моделями', 'Настройки движка'],
  zh: ['引擎和更新', '运行中', '已安装', '未安装', '正在检查…', '无法获取状态', '管理模型', '引擎设置'],
  ja: ['エンジンと更新', '稼働中', 'インストール済み', '未インストール', '確認中…', '状態を確認できません', 'モデルを管理', 'エンジン設定'],
  ko: ['엔진 및 업데이트', '활성', '설치됨', '미설치', '확인 중…', '상태를 확인할 수 없음', '모델 관리', '엔진 설정'],
  pt: ['Motores e atualizações', 'Ativo', 'Instalado', 'Não instalado', 'A verificar…', 'Estado indisponível', 'Gerir modelos', 'Definições do motor'],
};
