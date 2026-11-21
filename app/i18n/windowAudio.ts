import type { Language } from './translations';

const en = {
  open: 'Record window audio', window: 'Window to record', select: 'Choose a window',
  hint: 'Records audio from the selected app and its child processes for up to 30 seconds.',
  record: 'Start recording', stop: 'Stop and read score',
};
export const windowAudioStrings: Record<Language, typeof en> = {
  en,
  ru: { open: 'Записать звук окна', window: 'Окно для записи', select: 'Выберите окно', hint: 'Записывается звук выбранного приложения и его дочерних процессов, до 30 секунд.', record: 'Начать запись', stop: 'Остановить и прочитать ноты' },
  zh: { open: '录制窗口声音', window: '录制窗口', select: '选择窗口', hint: '录制所选应用及其子进程的声音，最长 30 秒。', record: '开始录制', stop: '停止并识谱' },
  ja: { open: 'ウィンドウの音声を録音', window: '録音するウィンドウ', select: 'ウィンドウを選択', hint: '選択したアプリとその子プロセスの音声を最大30秒録音します。', record: '録音を開始', stop: '停止して採譜' },
  ko: { open: '창 소리 녹음', window: '녹음할 창', select: '창 선택', hint: '선택한 앱과 하위 프로세스의 소리를 최대 30초 동안 녹음합니다.', record: '녹음 시작', stop: '중지하고 채보' },
  pt: { open: 'Gravar áudio da janela', window: 'Janela a gravar', select: 'Escolher janela', hint: 'Grava o áudio da aplicação selecionada e dos seus subprocessos por até 30 segundos.', record: 'Iniciar gravação', stop: 'Parar e transcrever a partitura' },
};
