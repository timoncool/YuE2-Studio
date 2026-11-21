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

export const enginePageStrings: Record<Language, readonly [string, string, string, string, string, string, string, string]> = {
  en: ['Engines and updates', 'Active', 'Installed', 'Not installed', 'Checking…', 'Status unavailable', 'Manage models', 'Engine settings'],
  ru: ['Движки и обновления', 'Активен', 'Установлен', 'Не установлен', 'Проверка…', 'Статус недоступен', 'Управление моделями', 'Настройки движка'],
  zh: ['引擎和更新', '运行中', '已安装', '未安装', '正在检查…', '无法获取状态', '管理模型', '引擎设置'],
  ja: ['エンジンと更新', '稼働中', 'インストール済み', '未インストール', '確認中…', '状態を確認できません', 'モデルを管理', 'エンジン設定'],
  ko: ['엔진 및 업데이트', '활성', '설치됨', '미설치', '확인 중…', '상태를 확인할 수 없음', '모델 관리', '엔진 설정'],
  pt: ['Motores e atualizações', 'Ativo', 'Instalado', 'Não instalado', 'A verificar…', 'Estado indisponível', 'Gerir modelos', 'Definições do motor'],
};
