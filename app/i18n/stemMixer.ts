/** Strings of the stem mixer: a song's stems played together, each with mute, solo, volume and pan. Engine-neutral. */

const en = {
  stemMixPlay: 'Play the stems together',
  stemMixPause: 'Pause the stems',
  stemMixLoop: 'Loop',
  stemMixFrom: 'from',
  stemMixTo: 'to',
  stemMixLoopStart: 'Loop start, seconds',
  stemMixLoopEnd: 'Loop end, seconds',
  stemMixStartHere: 'Start here',
  stemMixEndHere: 'End here',
  stemMixMute: 'Mute',
  stemMixSolo: 'Solo',
  stemMixVolume: 'Volume',
  stemMixPan: 'Pan',
  stemMixSave: 'Save this stem',
  stemMixNote: 'What is set here is for listening: the stem files are saved exactly as separated.',
};

type Strings = typeof en;

const ru: Strings = {
  stemMixPlay: 'Играть стемы вместе',
  stemMixPause: 'Пауза',
  stemMixLoop: 'Петля',
  stemMixFrom: 'от',
  stemMixTo: 'до',
  stemMixLoopStart: 'Начало петли, секунды',
  stemMixLoopEnd: 'Конец петли, секунды',
  stemMixStartHere: 'Начало здесь',
  stemMixEndHere: 'Конец здесь',
  stemMixMute: 'Заглушить',
  stemMixSolo: 'Соло',
  stemMixVolume: 'Громкость',
  stemMixPan: 'Панорама',
  stemMixSave: 'Сохранить этот стем',
  stemMixNote: 'Всё, что выставлено здесь, — для прослушивания: файлы стемов сохраняются ровно такими, как их разделили.',
};

const zh: Strings = {
  stemMixPlay: '同时播放各分轨',
  stemMixPause: '暂停',
  stemMixLoop: '循环',
  stemMixFrom: '从',
  stemMixTo: '到',
  stemMixLoopStart: '循环起点（秒）',
  stemMixLoopEnd: '循环终点（秒）',
  stemMixStartHere: '从这里开始',
  stemMixEndHere: '到这里结束',
  stemMixMute: '静音',
  stemMixSolo: '独奏',
  stemMixVolume: '音量',
  stemMixPan: '声像',
  stemMixSave: '保存这条分轨',
  stemMixNote: '这里的设置只用于试听：分轨文件按分离时的原样保存。',
};

const ja: Strings = {
  stemMixPlay: 'ステムを一緒に再生',
  stemMixPause: '一時停止',
  stemMixLoop: 'ループ',
  stemMixFrom: '開始',
  stemMixTo: '終了',
  stemMixLoopStart: 'ループの開始（秒）',
  stemMixLoopEnd: 'ループの終了（秒）',
  stemMixStartHere: 'ここから',
  stemMixEndHere: 'ここまで',
  stemMixMute: 'ミュート',
  stemMixSolo: 'ソロ',
  stemMixVolume: '音量',
  stemMixPan: 'パン',
  stemMixSave: 'このステムを保存',
  stemMixNote: 'ここでの設定は試聴用です。ステムのファイルは分離したままの状態で保存されます。',
};

const ko: Strings = {
  stemMixPlay: '스템을 함께 재생',
  stemMixPause: '일시정지',
  stemMixLoop: '반복',
  stemMixFrom: '시작',
  stemMixTo: '끝',
  stemMixLoopStart: '반복 시작(초)',
  stemMixLoopEnd: '반복 끝(초)',
  stemMixStartHere: '여기서 시작',
  stemMixEndHere: '여기서 끝',
  stemMixMute: '음소거',
  stemMixSolo: '솔로',
  stemMixVolume: '볼륨',
  stemMixPan: '팬',
  stemMixSave: '이 스템 저장',
  stemMixNote: '여기서 정한 값은 듣기 위한 것입니다. 스템 파일은 분리된 그대로 저장됩니다.',
};

const pt: Strings = {
  stemMixPlay: 'Tocar as pistas em conjunto',
  stemMixPause: 'Pausar',
  stemMixLoop: 'Ciclo',
  stemMixFrom: 'de',
  stemMixTo: 'até',
  stemMixLoopStart: 'Início do ciclo, segundos',
  stemMixLoopEnd: 'Fim do ciclo, segundos',
  stemMixStartHere: 'Começar aqui',
  stemMixEndHere: 'Acabar aqui',
  stemMixMute: 'Silenciar',
  stemMixSolo: 'Solo',
  stemMixVolume: 'Volume',
  stemMixPan: 'Panorâmica',
  stemMixSave: 'Guardar esta pista',
  stemMixNote: 'O que se define aqui é para ouvir: os ficheiros das pistas são guardados exatamente como foram separados.',
};

export const stemMixerStrings = { en, ru, zh, ja, ko, pt };
