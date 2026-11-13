import type { Language } from './translations';
export const songTokenStrings = {
  en: { tokens: 'tokens', tokenLoading: 'Reading the checkpoint tokenizer…' },
  ru: { tokens: 'токенов', tokenLoading: 'Чтение токенизатора модели…' },
  zh: { tokens: '词元', tokenLoading: '正在读取模型词元…' },
  ja: { tokens: 'トークン', tokenLoading: 'モデルのトークナイザーを読み込み中…' },
  ko: { tokens: '토큰', tokenLoading: '모델 토크나이저를 읽는 중…' },
};
const en = { parts: 'Edit style parts', sections: 'Edit sections', add: 'Add', duplicate: 'Duplicate', remove: 'Delete', up: 'Move up', down: 'Move down', tag: 'Section tag', words: 'Words', language: 'Vocal language', auto: 'Automatic', lines: 'Sung lines', instructions: 'Additional instructions', descriptor: 'Style descriptor' };
export const songWritingStrings: Record<Language, typeof en> = {
  en,
  ru: { parts: 'Стиль по частям', sections: 'Редактор секций', add: 'Добавить', duplicate: 'Дублировать', remove: 'Удалить', up: 'Выше', down: 'Ниже', tag: 'Название секции', words: 'Слова', language: 'Язык вокала', auto: 'Автоматически', lines: 'Строки вокала', instructions: 'Дополнительные указания', descriptor: 'Описание звучания' },
  zh: { parts: '编辑风格元素', sections: '编辑段落', add: '添加', duplicate: '复制', remove: '删除', up: '上移', down: '下移', tag: '段落标签', words: '歌词', language: '演唱语言', auto: '自动', lines: '演唱行数', instructions: '附加说明', descriptor: '风格描述' },
  ja: { parts: 'スタイル要素を編集', sections: 'セクションを編集', add: '追加', duplicate: '複製', remove: '削除', up: '上へ', down: '下へ', tag: 'セクション名', words: '歌詞', language: '歌唱言語', auto: '自動', lines: '歌詞の行数', instructions: '追加の指示', descriptor: '音楽スタイル' },
  ko: { parts: '스타일 요소 편집', sections: '섹션 편집', add: '추가', duplicate: '복제', remove: '삭제', up: '위로', down: '아래로', tag: '섹션 이름', words: '가사', language: '보컬 언어', auto: '자동', lines: '가사 줄 수', instructions: '추가 지시', descriptor: '스타일 설명' },
};
export const songDetailStrings = {
  en: { tempo: 'Tempo', voice: 'Voices', preset: 'Section preset', cycle: 'Next section tag', lineEditor: 'Edit lines and letter case', addLine: 'Add line', duplicateStanza: 'Duplicate stanza', deleteStanza: 'Delete stanza', flipCase: 'Flip letter case', sound: 'Sound / genre', caseHint: 'Click a letter to change its case. The model receives the edited letters.' },
  ru: { tempo: 'Темп', voice: 'Голоса', preset: 'Тип секции', cycle: 'Следующий тип секции', lineEditor: 'Строки и регистр букв', addLine: 'Добавить строку', duplicateStanza: 'Дублировать строфу', deleteStanza: 'Удалить строфу', flipCase: 'Сменить регистр буквы', sound: 'Звучание / жанр', caseHint: 'Нажмите букву, чтобы сменить регистр. Модель получает изменённые буквы.' },
  zh: { tempo: '速度', voice: '人声', preset: '段落类型', cycle: '下一个段落类型', lineEditor: '编辑歌词行和字母大小写', addLine: '添加行', duplicateStanza: '复制诗节', deleteStanza: '删除诗节', flipCase: '切换字母大小写', sound: '音色 / 风格', caseHint: '点击字母切换大小写。模型接收修改后的文字。' },
  ja: { tempo: 'テンポ', voice: '歌声', preset: 'セクションの種類', cycle: '次のセクション名', lineEditor: '行と文字の大小を編集', addLine: '行を追加', duplicateStanza: '詩節を複製', deleteStanza: '詩節を削除', flipCase: '大文字・小文字を切替', sound: 'サウンド / ジャンル', caseHint: '文字をクリックして大文字・小文字を切り替えます。変更後の文字がモデルに送られます。' },
  ko: { tempo: '템포', voice: '목소리', preset: '섹션 유형', cycle: '다음 섹션 유형', lineEditor: '줄과 대소문자 편집', addLine: '줄 추가', duplicateStanza: '연 복제', deleteStanza: '연 삭제', flipCase: '대소문자 전환', sound: '사운드 / 장르', caseHint: '글자를 클릭해 대소문자를 바꿉니다. 수정된 글자가 모델에 전달됩니다.' },
};
/** One line under each field of the style's parts: what it sets, what Automatic leaves to the model, what to type. */
export const songPartHints = {
  en: {
    languageHint: 'The language the lyrics are sung in. Automatic: the model hears it from the lyrics.',
    tempoHint: 'Beats per minute, written into the style. Automatic: the model picks a tempo for the genre; set it when the song needs a given speed.',
    voiceHint: 'Who sings, in English words: "sweet female vocal", "raspy male voice". Automatic: the model takes the voice from the genre.',
    soundHint: 'One more part of the style: a genre, a mood or an instrument, in English words the model knows ("lo-fi", "warm piano").',
  },
  ru: {
    languageHint: 'Язык, на котором поются слова. Автоматически: модель поймёт его по тексту песни.',
    tempoHint: 'Удары в минуту, вписываются в стиль. Автоматически: модель подберёт темп под жанр; задайте, если песне нужна определённая скорость.',
    voiceHint: 'Кто поёт, английскими словами: "sweet female vocal", "raspy male voice". Автоматически: модель возьмёт голос из жанра.',
    soundHint: 'Ещё одна часть стиля: жанр, настроение или инструмент — английскими словами, которые модель знает ("lo-fi", "warm piano").',
  },
  zh: {
    languageHint: '歌词演唱所用的语言。自动：模型根据歌词判断。',
    tempoHint: '每分钟拍数，会写入风格。自动：模型按曲风选择速度；歌曲需要特定快慢时再设置。',
    voiceHint: '谁来唱，用英文词描述："sweet female vocal"（甜美女声）、"raspy male voice"（沙哑男声）。自动：模型根据曲风决定人声。',
    soundHint: '风格的又一个元素：曲风、情绪或乐器，用模型认识的英文词（"lo-fi"、"warm piano" 温暖钢琴）。',
  },
  ja: {
    languageHint: '歌詞を歌う言語。自動：モデルが歌詞から判断します。',
    tempoHint: '1分あたりの拍数で、スタイルに書き込まれます。自動：モデルがジャンルに合うテンポを選びます。速さを決めたいときに設定します。',
    voiceHint: '誰が歌うかを英語で："sweet female vocal"（甘い女性ボーカル）、"raspy male voice"（かすれた男性の声）。自動：モデルがジャンルから声を決めます。',
    soundHint: 'スタイルの要素をもう一つ：ジャンル、雰囲気、楽器を、モデルが知っている英語で（"lo-fi"、"warm piano"）。',
  },
  ko: {
    languageHint: '가사를 부르는 언어. 자동: 모델이 가사를 보고 판단합니다.',
    tempoHint: '분당 박자 수로, 스타일에 적힙니다. 자동: 모델이 장르에 맞는 템포를 고릅니다. 정해진 빠르기가 필요할 때 설정하세요.',
    voiceHint: '누가 부르는지 영어로: "sweet female vocal"(달콤한 여성 보컬), "raspy male voice"(거친 남성 목소리). 자동: 모델이 장르에서 목소리를 정합니다.',
    soundHint: '스타일 요소 하나 더: 장르, 분위기, 악기를 모델이 아는 영어로("lo-fi", "warm piano").',
  },
};
