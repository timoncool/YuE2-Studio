export const musicSourceStrings = {
  en: {
    engine: 'Music engine', local: 'Local YuE2', model: 'OpenRouter music model', choose: 'Select a model',
    loading: 'Loading OpenRouter models…', empty: 'No OpenRouter music models are available.',
    catalogError: 'Could not load OpenRouter music models.', refresh: 'Refresh models',
    keyMissing: 'Add an OpenRouter API key to generate music.', openCloudSettings: 'Open cloud settings',
    openResources: 'Install local models',
  },
  ru: {
    engine: 'Музыкальный движок', local: 'Локальный YuE2', model: 'Музыкальная модель OpenRouter', choose: 'Выберите модель',
    loading: 'Загрузка моделей OpenRouter…', empty: 'Нет доступных музыкальных моделей OpenRouter.',
    catalogError: 'Не удалось загрузить музыкальные модели OpenRouter.', refresh: 'Обновить список моделей',
    keyMissing: 'Добавьте API-ключ OpenRouter для генерации музыки.', openCloudSettings: 'Открыть настройки облака',
    openResources: 'Установить локальные модели',
  },
  zh: {
    engine: '音乐引擎', local: '本地 YuE2', model: 'OpenRouter 音乐模型', choose: '选择模型',
    loading: '正在加载 OpenRouter 模型…', empty: '当前没有可用的 OpenRouter 音乐模型。',
    catalogError: '无法加载 OpenRouter 音乐模型。', refresh: '刷新模型列表',
    keyMissing: '请添加 OpenRouter API 密钥以生成音乐。', openCloudSettings: '打开云端设置',
    openResources: '安装本地模型',
  },
  ja: {
    engine: '音楽エンジン', local: 'ローカル YuE2', model: 'OpenRouter の音楽モデル', choose: 'モデルを選択',
    loading: 'OpenRouter のモデルを読み込み中…', empty: '利用できる OpenRouter の音楽モデルはありません。',
    catalogError: 'OpenRouter の音楽モデルを読み込めませんでした。', refresh: 'モデル一覧を更新',
    keyMissing: '音楽を生成するには OpenRouter の API キーを追加してください。', openCloudSettings: 'クラウド設定を開く',
    openResources: 'ローカルモデルをインストール',
  },
  ko: {
    engine: '음악 엔진', local: '로컬 YuE2', model: 'OpenRouter 음악 모델', choose: '모델 선택',
    loading: 'OpenRouter 모델을 불러오는 중…', empty: '사용할 수 있는 OpenRouter 음악 모델이 없습니다.',
    catalogError: 'OpenRouter 음악 모델을 불러오지 못했습니다.', refresh: '모델 목록 새로고침',
    keyMissing: '음악을 생성하려면 OpenRouter API 키를 추가하세요.', openCloudSettings: '클라우드 설정 열기',
    openResources: '로컬 모델 설치',
  },
  pt: {
    engine: 'Motor de música', local: 'YuE2 local', model: 'Modelo musical do OpenRouter', choose: 'Selecione um modelo',
    loading: 'Carregando modelos do OpenRouter…', empty: 'Nenhum modelo musical do OpenRouter está disponível.',
    catalogError: 'Não foi possível carregar os modelos musicais do OpenRouter.', refresh: 'Atualizar modelos',
    keyMissing: 'Adicione uma chave de API do OpenRouter para gerar música.', openCloudSettings: 'Abrir configurações da nuvem',
    openResources: 'Instalar modelos locais',
  },
};

export const nativeMusicSourceStrings = {
  en: {
    model: 'Local model', loading: 'Checking model availability…', error: 'Could not check model availability.',
    engineMissing: 'This engine is not installed.', empty: 'No profiles are available.', unavailable: 'Unavailable',
    chooseLegacyProfile: 'Choose a profile to create a new take. This song used these model files.',
    settings: 'Generation settings', default: 'Engine default', duration: 'Duration (seconds)', steps: 'Steps',
    seed: 'Seed', lmSeed: 'Language model seed', scoreKept: 'The draft keeps your ABC score. This model does not use it.',
  },
  ru: {
    model: 'Локальная модель', loading: 'Проверяем доступность моделей…', error: 'Не удалось проверить доступность моделей.',
    engineMissing: 'Этот движок не установлен.', empty: 'Нет доступных профилей.', unavailable: 'Недоступна',
    chooseLegacyProfile: 'Выберите профиль для нового дубля. В исходной песне использовались эти файлы моделей.',
    settings: 'Настройки генерации', default: 'По умолчанию в движке', duration: 'Длительность (с)', steps: 'Шаги',
    seed: 'Сид', lmSeed: 'Сид языковой модели', scoreKept: 'Партитура ABC сохранена в черновике. Эта модель её не использует.',
  },
  zh: {
    model: '本地模型', loading: '正在检查模型是否可用…', error: '无法检查模型是否可用。',
    engineMissing: '尚未安装此引擎。', empty: '没有可用的配置。', unavailable: '不可用',
    chooseLegacyProfile: '请选择一个配置来创建新版本。原曲使用了以下模型文件。',
    settings: '生成设置', default: '引擎默认值', duration: '时长（秒）', steps: '步数',
    seed: '种子', lmSeed: '语言模型种子', scoreKept: 'ABC 乐谱保留在草稿中。此模型不使用它。',
  },
  ja: {
    model: 'ローカルモデル', loading: 'モデルの利用可否を確認中…', error: 'モデルの利用可否を確認できませんでした。',
    engineMissing: 'このエンジンはインストールされていません。', empty: '利用できるプロファイルがありません。', unavailable: '利用不可',
    chooseLegacyProfile: '新しいテイクを作成するにはプロファイルを選択してください。元の曲で使用したモデルファイルは以下のとおりです。',
    settings: '生成設定', default: 'エンジンの既定値', duration: '長さ（秒）', steps: 'ステップ',
    seed: 'シード', lmSeed: '言語モデルのシード', scoreKept: 'ABC 楽譜は下書きに残ります。このモデルは楽譜を使いません。',
  },
  ko: {
    model: '로컬 모델', loading: '모델 사용 가능 여부를 확인하는 중…', error: '모델 사용 가능 여부를 확인하지 못했습니다.',
    engineMissing: '이 엔진이 설치되지 않았습니다.', empty: '사용할 수 있는 프로필이 없습니다.', unavailable: '사용 불가',
    chooseLegacyProfile: '새 테이크를 만들려면 프로필을 선택하세요. 원래 곡에는 다음 모델 파일이 사용되었습니다.',
    settings: '생성 설정', default: '엔진 기본값', duration: '길이(초)', steps: '단계',
    seed: '시드', lmSeed: '언어 모델 시드', scoreKept: 'ABC 악보는 초안에 저장됩니다. 이 모델은 악보를 사용하지 않습니다.',
  },
  pt: {
    model: 'Modelo local', loading: 'Verificando a disponibilidade dos modelos…', error: 'Não foi possível verificar a disponibilidade dos modelos.',
    engineMissing: 'Este motor não está instalado.', empty: 'Nenhum perfil disponível.', unavailable: 'Indisponível',
    chooseLegacyProfile: 'Escolha um perfil para criar uma nova versão. A faixa original usou estes arquivos de modelo.',
    settings: 'Configurações de geração', default: 'Padrão do motor', duration: 'Duração (segundos)', steps: 'Etapas',
    seed: 'Semente', lmSeed: 'Semente do modelo de linguagem', scoreKept: 'A partitura ABC fica no rascunho. Este modelo não a utiliza.',
  },
};
