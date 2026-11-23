import type { Language } from './translations';

type ToolsPageStrings = {
  title: string; intro: string; track: string; trackHint: string;
  withTrack: string; withTrackHint: string; createNew: string; createNewHint: string;
  hasStems: (count: number) => string; hasMidi: string; restored: string; isStem: string;
  ready: string; needsDownload: (size: string) => string; unavailable: string;
  chooseTrack: string; otherGeneration: string; describeSound: string; chooseVideo: string; chooseFile: string;
  model: string; license: string; nonCommercial: string; inLibrary: string;
  openIn: string; openInHint: string; range: (min: string, max: string) => string;
  fromTrack: string; fromFile: string; whichStems: string; options: string;
  sfxDescription: string; sfxPrompt: string; sfxPromptHint: string; duration: string;
  foleySeconds: string; recognise: string; recogniseIntro: string; recogniseWay: string; wayLocal: string; wayCloud: string;
  language: string; autoLanguage: string; cloudKey: string; configure: string; saveText: string;
  download: (name: string, size: string) => string; downloading: (name: string, done: string, total: string) => string; saveWav: string;
  runsOn: (where: string) => string; midiDeleteTitle: string; midiDeleteMessage: string; midiDelete: string;
};

export const toolsPageStrings: Record<Language, ToolsPageStrings> = {
  en: {
    title: 'Tools', intro: 'Split a finished track, turn it into MIDI, restore it, process it — or make a sound from scratch.',
    track: 'Track', trackHint: 'The tools below work on it', withTrack: 'With a track from the library', withTrackHint: 'Work on the track chosen above.',
    createNew: 'Make something new', createNewHint: 'No track needed: the result appears in the library.',
    hasStems: count => `stems: ${count}`, hasMidi: 'has MIDI', restored: 'restored', isStem: 'this is a stem',
    ready: 'Ready', needsDownload: size => `Needs a download · ${size}`, unavailable: 'Unavailable on this computer',
    chooseTrack: 'Choose a track above', otherGeneration: 'Another generation is running', describeSound: 'Describe the sound', chooseVideo: 'Choose a video', chooseFile: 'Choose a file',
    model: 'Model', license: 'license', nonCommercial: 'non-commercial use only', inLibrary: 'In the queue · it will appear in the library',
    openIn: 'Open the track in…', openInHint: 'Separate windows for the chosen track', range: (min, max) => `from ${min} to ${max}`,
    fromTrack: 'Chosen track', fromFile: 'File from the computer', whichStems: 'Which parts to make', options: 'Options',
    sfxDescription: 'Makes a short sound from a description — steps, rain, a door slam.', sfxPrompt: 'Sound description', sfxPromptHint: 'The model understands English and Chinese.', duration: 'Duration, s',
    foleySeconds: 'How many seconds from the start to voice', recognise: 'Recognise text', recogniseIntro: 'Turns a recording into text — handy when the lyrics exist only as a voice note.',
    recogniseWay: 'Where it runs', wayLocal: 'On this computer', wayCloud: 'In the cloud (OpenRouter)',
    language: 'Language', autoLanguage: 'Detect automatically', cloudKey: 'Add an OpenRouter key to recognise in the cloud.', configure: 'Set up', saveText: 'Save .txt', download: (name, size) => `Download ${name} — ${size}`, downloading: (name, done, total) => `Downloading ${name} · ${done} of ${total}`, saveWav: 'Save WAV', runsOn: where => `runs on ${where}`, midiDeleteTitle: 'Delete this track’s MIDI?', midiDeleteMessage: 'The notes heard and edited for this track are deleted. The track itself stays.', midiDelete: 'Delete MIDI',
  },
  ru: {
    title: 'Инструменты', intro: 'Разобрать готовый трек, перевести его в MIDI, восстановить, обработать — или создать звук с нуля.',
    track: 'Трек', trackHint: 'С ним работают инструменты ниже', withTrack: 'С треком из медиатеки', withTrackHint: 'Работают с треком, выбранным вверху.',
    createNew: 'Создать новое', createNewHint: 'Трек не нужен: результат появится в медиатеке.',
    hasStems: count => `стемы: ${count}`, hasMidi: 'есть MIDI', restored: 'восстановлен', isStem: 'это стем',
    ready: 'Готов', needsDownload: size => `Нужно скачать · ${size}`, unavailable: 'Недоступен на этом компьютере',
    chooseTrack: 'Выберите трек вверху', otherGeneration: 'Идёт другая генерация', describeSound: 'Опишите звук', chooseVideo: 'Выберите видео', chooseFile: 'Выберите файл',
    model: 'Модель', license: 'лицензия', nonCommercial: 'только некоммерческое использование', inLibrary: 'В очереди · появится в медиатеке',
    openIn: 'Открыть трек в…', openInHint: 'Отдельные окна для выбранного трека', range: (min, max) => `от ${min} до ${max}`,
    fromTrack: 'Выбранный трек', fromFile: 'Файл с компьютера', whichStems: 'Какие дорожки сделать', options: 'Параметры',
    sfxDescription: 'Создаёт короткий звук по описанию — шаги, дождь, хлопок двери.', sfxPrompt: 'Описание звука', sfxPromptHint: 'Модель понимает английский и китайский.', duration: 'Длительность, с',
    foleySeconds: 'Сколько секунд от начала озвучить', recognise: 'Распознать текст', recogniseIntro: 'Превращает запись в текст — пригодится, когда слова есть только голосовой заметкой.',
    recogniseWay: 'Где распознавать', wayLocal: 'На этом компьютере', wayCloud: 'В облаке (OpenRouter)',
    language: 'Язык', autoLanguage: 'Определить автоматически', cloudKey: 'Для распознавания в облаке нужен ключ OpenRouter.', configure: 'Настроить', saveText: 'Сохранить .txt', download: (name, size) => `Скачать ${name} — ${size}`, downloading: (name, done, total) => `Скачивается ${name} · ${done} из ${total}`, saveWav: 'Сохранить WAV', runsOn: where => `считает на ${where}`, midiDeleteTitle: 'Удалить MIDI этого трека?', midiDeleteMessage: 'Ноты, распознанные и поправленные для трека, удалятся. Сам трек останется.', midiDelete: 'Удалить MIDI',
  },
  zh: {
    title: '工具', intro: '拆分成品音轨、转成 MIDI、修复、处理——或从零生成声音。',
    track: '音轨', trackHint: '下面的工具处理它', withTrack: '处理资料库中的音轨', withTrackHint: '处理上方选中的音轨。',
    createNew: '新建', createNewHint: '不需要音轨：结果会出现在资料库中。',
    hasStems: count => `分轨：${count}`, hasMidi: '有 MIDI', restored: '已修复', isStem: '这是分轨',
    ready: '就绪', needsDownload: size => `需要下载 · ${size}`, unavailable: '此电脑上不可用',
    chooseTrack: '请在上方选择音轨', otherGeneration: '另一个生成正在进行', describeSound: '请描述声音', chooseVideo: '请选择视频', chooseFile: '请选择文件',
    model: '模型', license: '许可', nonCommercial: '仅限非商业用途', inLibrary: '排队中 · 将出现在资料库中',
    openIn: '在…中打开音轨', openInHint: '为选中音轨打开独立窗口', range: (min, max) => `${min} 到 ${max}`,
    fromTrack: '选中的音轨', fromFile: '电脑上的文件', whichStems: '生成哪些分轨', options: '参数',
    sfxDescription: '根据描述生成短音效——脚步、雨声、关门声。', sfxPrompt: '声音描述', sfxPromptHint: '模型理解英语和中文。', duration: '时长（秒）',
    foleySeconds: '从开头配多少秒', recognise: '识别文字', recogniseIntro: '把录音转成文字——歌词只有语音备忘时很有用。',
    recogniseWay: '在哪里识别', wayLocal: '在本机', wayCloud: '在云端（OpenRouter）',
    language: '语言', autoLanguage: '自动检测', cloudKey: '云端识别需要 OpenRouter 密钥。', configure: '设置', saveText: '保存 .txt', download: (name, size) => `下载 ${name} — ${size}`, downloading: (name, done, total) => `正在下载 ${name} · ${done} / ${total}`, saveWav: '保存 WAV', runsOn: where => `运行于 ${where}`, midiDeleteTitle: '删除此音轨的 MIDI？', midiDeleteMessage: '为此音轨识别和编辑的音符将被删除。音轨本身保留。', midiDelete: '删除 MIDI',
  },
  ja: {
    title: 'ツール', intro: '完成した曲を分離し、MIDI にし、復元し、加工する——またはゼロから音を作ります。',
    track: 'トラック', trackHint: '下のツールはこのトラックを使います', withTrack: 'ライブラリのトラックで', withTrackHint: '上で選んだトラックを使います。',
    createNew: '新しく作る', createNewHint: 'トラック不要：結果はライブラリに入ります。',
    hasStems: count => `ステム：${count}`, hasMidi: 'MIDI あり', restored: '復元済み', isStem: 'これはステムです',
    ready: '準備完了', needsDownload: size => `ダウンロードが必要 · ${size}`, unavailable: 'このコンピューターでは使えません',
    chooseTrack: '上でトラックを選んでください', otherGeneration: '別の生成が実行中です', describeSound: '音を説明してください', chooseVideo: '動画を選んでください', chooseFile: 'ファイルを選んでください',
    model: 'モデル', license: 'ライセンス', nonCommercial: '非商用利用のみ', inLibrary: '待機中 · ライブラリに入ります',
    openIn: 'トラックを開く…', openInHint: '選んだトラックを別ウィンドウで', range: (min, max) => `${min}〜${max}`,
    fromTrack: '選んだトラック', fromFile: 'コンピューターのファイル', whichStems: '作るパート', options: 'オプション',
    sfxDescription: '説明から短い音を作ります——足音、雨、ドアの音。', sfxPrompt: '音の説明', sfxPromptHint: 'モデルは英語と中国語を理解します。', duration: '長さ（秒）',
    foleySeconds: '先頭から何秒に音を付けるか', recognise: 'テキストを認識', recogniseIntro: '録音をテキストにします——歌詞がボイスメモにしかないときに。',
    recogniseWay: '認識する場所', wayLocal: 'このコンピューター', wayCloud: 'クラウド（OpenRouter）',
    language: '言語', autoLanguage: '自動検出', cloudKey: 'クラウドで認識するには OpenRouter キーが必要です。', configure: '設定', saveText: '.txt を保存', download: (name, size) => `${name} をダウンロード — ${size}`, downloading: (name, done, total) => `${name} をダウンロード中 · ${done} / ${total}`, saveWav: 'WAV を保存', runsOn: where => `${where} で計算`, midiDeleteTitle: 'このトラックの MIDI を削除しますか？', midiDeleteMessage: 'このトラック用に採譜・編集した音符が削除されます。トラック自体は残ります。', midiDelete: 'MIDI を削除',
  },
  ko: {
    title: '도구', intro: '완성된 트랙을 분리하고, MIDI로 바꾸고, 복원하고, 가공하거나 — 소리를 처음부터 만듭니다.',
    track: '트랙', trackHint: '아래 도구가 이 트랙을 사용합니다', withTrack: '라이브러리의 트랙으로', withTrackHint: '위에서 고른 트랙을 사용합니다.',
    createNew: '새로 만들기', createNewHint: '트랙 불필요: 결과는 라이브러리에 나타납니다.',
    hasStems: count => `스템: ${count}`, hasMidi: 'MIDI 있음', restored: '복원됨', isStem: '스템입니다',
    ready: '준비됨', needsDownload: size => `다운로드 필요 · ${size}`, unavailable: '이 컴퓨터에서 사용할 수 없음',
    chooseTrack: '위에서 트랙을 고르세요', otherGeneration: '다른 생성이 진행 중입니다', describeSound: '소리를 설명하세요', chooseVideo: '동영상을 고르세요', chooseFile: '파일을 고르세요',
    model: '모델', license: '라이선스', nonCommercial: '비상업적 사용만 가능', inLibrary: '대기 중 · 라이브러리에 나타납니다',
    openIn: '트랙 열기…', openInHint: '고른 트랙을 별도 창에서', range: (min, max) => `${min}~${max}`,
    fromTrack: '고른 트랙', fromFile: '컴퓨터의 파일', whichStems: '만들 파트', options: '옵션',
    sfxDescription: '설명으로 짧은 소리를 만듭니다 — 발소리, 비, 문 닫히는 소리.', sfxPrompt: '소리 설명', sfxPromptHint: '모델은 영어와 중국어를 이해합니다.', duration: '길이(초)',
    foleySeconds: '처음부터 몇 초에 소리를 입힐지', recognise: '텍스트 인식', recogniseIntro: '녹음을 텍스트로 바꿉니다 — 가사가 음성 메모로만 있을 때 유용합니다.',
    recogniseWay: '인식 위치', wayLocal: '이 컴퓨터에서', wayCloud: '클라우드(OpenRouter)',
    language: '언어', autoLanguage: '자동 감지', cloudKey: '클라우드 인식에는 OpenRouter 키가 필요합니다.', configure: '설정', saveText: '.txt 저장', download: (name, size) => `${name} 다운로드 — ${size}`, downloading: (name, done, total) => `${name} 다운로드 중 · ${done} / ${total}`, saveWav: 'WAV 저장', runsOn: where => `${where}에서 실행`, midiDeleteTitle: '이 트랙의 MIDI를 삭제할까요?', midiDeleteMessage: '이 트랙에서 채보하고 편집한 음표가 삭제됩니다. 트랙 자체는 남습니다.', midiDelete: 'MIDI 삭제',
  },
  pt: {
    title: 'Ferramentas', intro: 'Separe uma faixa pronta, transforme-a em MIDI, restaure, processe — ou crie um som do zero.',
    track: 'Faixa', trackHint: 'As ferramentas abaixo trabalham com ela', withTrack: 'Com uma faixa da biblioteca', withTrackHint: 'Trabalham com a faixa escolhida acima.',
    createNew: 'Criar algo novo', createNewHint: 'Não precisa de faixa: o resultado aparece na biblioteca.',
    hasStems: count => `stems: ${count}`, hasMidi: 'tem MIDI', restored: 'restaurada', isStem: 'é um stem',
    ready: 'Pronto', needsDownload: size => `Precisa baixar · ${size}`, unavailable: 'Indisponível neste computador',
    chooseTrack: 'Escolha uma faixa acima', otherGeneration: 'Outra geração está em andamento', describeSound: 'Descreva o som', chooseVideo: 'Escolha um vídeo', chooseFile: 'Escolha um arquivo',
    model: 'Modelo', license: 'licença', nonCommercial: 'apenas uso não comercial', inLibrary: 'Na fila · aparecerá na biblioteca',
    openIn: 'Abrir a faixa em…', openInHint: 'Janelas separadas para a faixa escolhida', range: (min, max) => `de ${min} a ${max}`,
    fromTrack: 'Faixa escolhida', fromFile: 'Arquivo do computador', whichStems: 'Quais partes criar', options: 'Opções',
    sfxDescription: 'Cria um som curto a partir de uma descrição — passos, chuva, uma porta batendo.', sfxPrompt: 'Descrição do som', sfxPromptHint: 'O modelo entende inglês e chinês.', duration: 'Duração, s',
    foleySeconds: 'Quantos segundos desde o início sonorizar', recognise: 'Reconhecer texto', recogniseIntro: 'Transforma uma gravação em texto — útil quando a letra existe só como nota de voz.',
    recogniseWay: 'Onde reconhecer', wayLocal: 'Neste computador', wayCloud: 'Na nuvem (OpenRouter)',
    language: 'Idioma', autoLanguage: 'Detectar automaticamente', cloudKey: 'Para reconhecer na nuvem é preciso uma chave OpenRouter.', configure: 'Configurar', saveText: 'Salvar .txt', download: (name, size) => `Baixar ${name} — ${size}`, downloading: (name, done, total) => `Baixando ${name} · ${done} de ${total}`, saveWav: 'Salvar WAV', runsOn: where => `roda em ${where}`, midiDeleteTitle: 'Excluir o MIDI desta faixa?', midiDeleteMessage: 'As notas reconhecidas e editadas para esta faixa serão excluídas. A faixa em si permanece.', midiDelete: 'Excluir MIDI',
  },
};
