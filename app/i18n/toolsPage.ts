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
  navStems: string; navMidi: string; navRestore: string; navRecognise: string; navProcess: string; navEditor: string;
  effectsTitle: string; navEffects: string; foleyTitle: string; navFoley: string; navMidiEditor: string; editorStems: string;
  source: string; cloudFileOnly: string;
  separationModel: string; sepSix: string; sepSixHint: string; sepVocal: string; sepVocalHint: string;
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
    language: 'Language', autoLanguage: 'Detect automatically', cloudKey: 'Add an OpenRouter key to recognise in the cloud.', configure: 'Set up', saveText: 'Save .txt', download: (name, size) => `Download ${name} — ${size}`, downloading: (name, done, total) => `Downloading ${name} · ${done} of ${total}`, saveWav: 'Save WAV', runsOn: where => `runs on ${where}`, midiDeleteTitle: 'Delete this track’s MIDI?', midiDeleteMessage: 'The notes heard and edited for this track are deleted. The track itself stays.', midiDelete: 'Delete MIDI', navStems: 'Vocals, drums, bass and more apart', navMidi: 'Notes of every instrument', navRestore: 'Clean up an MP3', navRecognise: 'Lyrics from a recording', navProcess: 'Clean and master', navEditor: 'Trim, fade, normalise', effectsTitle: 'Sound effects', navEffects: 'A short sound from a description', foleyTitle: 'Sound for video', navFoley: 'Sound from the picture', navMidiEditor: 'Play, draw and edit notes', editorStems: 'Its stems open in the editor from the Stems tool.', source: 'What to recognise', cloudFileOnly: 'In the cloud only a file from the computer: the recording is sent to OpenRouter.', separationModel: 'Separation model', sepSix: 'Six parts', sepSixHint: 'Drums, bass, vocals, guitar, piano, the rest · HT-Demucs', sepVocal: 'Vocals and backing', sepVocalHint: 'The cleanest voice and the mix without it · Mel-Band RoFormer',
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
    language: 'Язык', autoLanguage: 'Определить автоматически', cloudKey: 'Для распознавания в облаке нужен ключ OpenRouter.', configure: 'Настроить', saveText: 'Сохранить .txt', download: (name, size) => `Скачать ${name} — ${size}`, downloading: (name, done, total) => `Скачивается ${name} · ${done} из ${total}`, saveWav: 'Сохранить WAV', runsOn: where => `считает на ${where}`, midiDeleteTitle: 'Удалить MIDI этого трека?', midiDeleteMessage: 'Ноты, распознанные и поправленные для трека, удалятся. Сам трек останется.', midiDelete: 'Удалить MIDI', navStems: 'Вокал, ударные, бас и другое отдельно', navMidi: 'Ноты каждого инструмента', navRestore: 'Почистить MP3', navRecognise: 'Слова из записи', navProcess: 'Почистить и отмастерить', navEditor: 'Обрезать, фейд, нормализация', effectsTitle: 'Звуковые эффекты', navEffects: 'Короткий звук по описанию', foleyTitle: 'Звук для видео', navFoley: 'Звук по картинке', navMidiEditor: 'Сыграть, нарисовать, править ноты', editorStems: 'Его стемы открываются в редакторе из инструмента «Разделить на стемы».', source: 'Что распознать', cloudFileOnly: 'В облако уходит только файл с компьютера: запись отправляется в OpenRouter.', separationModel: 'Модель разделения', sepSix: 'Шесть дорожек', sepSixHint: 'Ударные, бас, вокал, гитара, пианино, остальное · HT-Demucs', sepVocal: 'Вокал и минус', sepVocalHint: 'Самый чистый голос и аккомпанемент без него · Mel-Band RoFormer',
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
    language: '语言', autoLanguage: '自动检测', cloudKey: '云端识别需要 OpenRouter 密钥。', configure: '设置', saveText: '保存 .txt', download: (name, size) => `下载 ${name} — ${size}`, downloading: (name, done, total) => `正在下载 ${name} · ${done} / ${total}`, saveWav: '保存 WAV', runsOn: where => `运行于 ${where}`, midiDeleteTitle: '删除此音轨的 MIDI？', midiDeleteMessage: '为此音轨识别和编辑的音符将被删除。音轨本身保留。', midiDelete: '删除 MIDI', navStems: '人声、鼓、贝斯等分开', navMidi: '每件乐器的音符', navRestore: '清理 MP3', navRecognise: '从录音提取歌词', navProcess: '清理和母带处理', navEditor: '剪切、淡入淡出、标准化', effectsTitle: '音效', navEffects: '根据描述生成短音效', foleyTitle: '为视频配声', navFoley: '根据画面生成声音', navMidiEditor: '演奏、绘制和编辑音符', editorStems: '它的分轨可在“分离音轨”工具中用编辑器打开。', source: '识别什么', cloudFileOnly: '云端只能识别电脑上的文件：录音会发送到 OpenRouter。', separationModel: '分离模型', sepSix: '六条音轨', sepSixHint: '鼓、贝斯、人声、吉他、钢琴、其余 · HT-Demucs', sepVocal: '人声与伴奏', sepVocalHint: '最干净的人声和去掉人声的伴奏 · Mel-Band RoFormer',
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
    language: '言語', autoLanguage: '自動検出', cloudKey: 'クラウドで認識するには OpenRouter キーが必要です。', configure: '設定', saveText: '.txt を保存', download: (name, size) => `${name} をダウンロード — ${size}`, downloading: (name, done, total) => `${name} をダウンロード中 · ${done} / ${total}`, saveWav: 'WAV を保存', runsOn: where => `${where} で計算`, midiDeleteTitle: 'このトラックの MIDI を削除しますか？', midiDeleteMessage: 'このトラック用に採譜・編集した音符が削除されます。トラック自体は残ります。', midiDelete: 'MIDI を削除', navStems: 'ボーカル・ドラム・ベースなどを分離', navMidi: '各楽器の音符', navRestore: 'MP3 をきれいに', navRecognise: '録音から歌詞', navProcess: 'クリーンアップとマスタリング', navEditor: 'トリム、フェード、ノーマライズ', effectsTitle: '効果音', navEffects: '説明から短い音', foleyTitle: '動画の効果音', navFoley: '映像から音', navMidiEditor: '演奏・描画・音符の編集', editorStems: 'ステムは「ステムに分離」ツールからエディターで開けます。', source: '認識するもの', cloudFileOnly: 'クラウドではコンピューターのファイルのみ：録音は OpenRouter に送られます。', separationModel: '分離モデル', sepSix: '6 パート', sepSixHint: 'ドラム、ベース、ボーカル、ギター、ピアノ、その他 · HT-Demucs', sepVocal: 'ボーカルと伴奏', sepVocalHint: '最もきれいな声と声を抜いた伴奏 · Mel-Band RoFormer',
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
    language: '언어', autoLanguage: '자동 감지', cloudKey: '클라우드 인식에는 OpenRouter 키가 필요합니다.', configure: '설정', saveText: '.txt 저장', download: (name, size) => `${name} 다운로드 — ${size}`, downloading: (name, done, total) => `${name} 다운로드 중 · ${done} / ${total}`, saveWav: 'WAV 저장', runsOn: where => `${where}에서 실행`, midiDeleteTitle: '이 트랙의 MIDI를 삭제할까요?', midiDeleteMessage: '이 트랙에서 채보하고 편집한 음표가 삭제됩니다. 트랙 자체는 남습니다.', midiDelete: 'MIDI 삭제', navStems: '보컬, 드럼, 베이스 등을 분리', navMidi: '악기별 음표', navRestore: 'MP3 정리', navRecognise: '녹음에서 가사', navProcess: '정리와 마스터링', navEditor: '자르기, 페이드, 노멀라이즈', effectsTitle: '효과음', navEffects: '설명으로 짧은 소리', foleyTitle: '동영상 효과음', navFoley: '화면으로 소리', navMidiEditor: '연주, 그리기, 음표 편집', editorStems: '스템은 「스템 분리」 도구에서 편집기로 엽니다.', source: '인식할 대상', cloudFileOnly: '클라우드에서는 컴퓨터의 파일만: 녹음이 OpenRouter로 전송됩니다.', separationModel: '분리 모델', sepSix: '6개 파트', sepSixHint: '드럼, 베이스, 보컬, 기타, 피아노, 나머지 · HT-Demucs', sepVocal: '보컬과 반주', sepVocalHint: '가장 깨끗한 목소리와 목소리를 뺀 반주 · Mel-Band RoFormer',
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
    language: 'Idioma', autoLanguage: 'Detectar automaticamente', cloudKey: 'Para reconhecer na nuvem é preciso uma chave OpenRouter.', configure: 'Configurar', saveText: 'Salvar .txt', download: (name, size) => `Baixar ${name} — ${size}`, downloading: (name, done, total) => `Baixando ${name} · ${done} de ${total}`, saveWav: 'Salvar WAV', runsOn: where => `roda em ${where}`, midiDeleteTitle: 'Excluir o MIDI desta faixa?', midiDeleteMessage: 'As notas reconhecidas e editadas para esta faixa serão excluídas. A faixa em si permanece.', midiDelete: 'Excluir MIDI', navStems: 'Voz, bateria, baixo e mais separados', navMidi: 'Notas de cada instrumento', navRestore: 'Limpar um MP3', navRecognise: 'Letra de uma gravação', navProcess: 'Limpar e masterizar', navEditor: 'Cortar, fade, normalizar', effectsTitle: 'Efeitos sonoros', navEffects: 'Um som curto por descrição', foleyTitle: 'Som para vídeo', navFoley: 'Som a partir da imagem', navMidiEditor: 'Tocar, desenhar e editar notas', editorStems: 'Os stems abrem no editor pela ferramenta de stems.', source: 'O que reconhecer', cloudFileOnly: 'Na nuvem só um arquivo do computador: a gravação vai para o OpenRouter.', separationModel: 'Modelo de separação', sepSix: 'Seis partes', sepSixHint: 'Bateria, baixo, voz, guitarra, piano, o resto · HT-Demucs', sepVocal: 'Voz e acompanhamento', sepVocalHint: 'A voz mais limpa e o acompanhamento sem ela · Mel-Band RoFormer',
  },
};
