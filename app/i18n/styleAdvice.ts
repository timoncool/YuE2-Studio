import type { StyleRule } from '../services/songWriting';
import type { Language } from './translations';

export const styleAdvice: Record<Language, Record<StyleRule | 'title', string>> = {
  en: {
    title: 'Style suggestions', language: 'Start with the language of the vocals, or “instrumental”.',
    tempo: 'For a specific tempo, end with a tag such as “120 BPM”.', length: 'Try 12–50 words for a complete style.',
    numbers: 'Describe the sound of measurements or numbers in musical terms.', negative: 'Name the sound you want, such as “solo piano”.',
    instruction: 'Turn instructions into short musical descriptors.', artist: 'Describe the artist’s sound or delivery.',
    genres: 'Choose a main genre so the arrangement has a clear direction.', quality: 'Name the instrument or texture that creates this sound.',
    instruments: 'Choose four to six instruments to bring forward.', voice: 'Describe the register, timbre or delivery of the voice.',
    lyrics: 'Move lyric lines and section labels to Lyrics.',
  },
  ru: {
    title: 'Подсказки по стилю', language: 'Начните с языка вокала или слова «instrumental».',
    tempo: 'Если важен темп, в конце укажите, например, «120 BPM».', length: 'Попробуйте описать стиль в 12–50 словах.',
    numbers: 'Передайте звучание чисел и измерений музыкальными терминами.', negative: 'Опишите желаемое звучание, например «solo piano».',
    instruction: 'Замените указания короткими музыкальными описаниями.', artist: 'Опишите звучание или манеру исполнения артиста.',
    genres: 'Выберите основной жанр для аранжировки.', quality: 'Назовите инструмент или фактуру, которые дают это звучание.',
    instruments: 'Выберите четыре–шесть ведущих инструментов.', voice: 'Опишите регистр, тембр или манеру вокала.',
    lyrics: 'Перенесите слова песни и метки секций в поле «Текст».',
  },
  pt: {
    title: 'Sugestões de estilo', language: 'Comece pelo idioma da voz ou por “instrumental”.',
    tempo: 'Para definir o andamento, termine com “120 BPM”, por exemplo.', length: 'Experimente descrever o estilo em 12–50 palavras.',
    numbers: 'Descreva medidas e números em termos musicais.', negative: 'Indique o som desejado, como “solo piano”.',
    instruction: 'Transforme as instruções em descrições musicais curtas.', artist: 'Descreva o som ou a interpretação do artista.',
    genres: 'Escolha um gênero principal para o arranjo.', quality: 'Indique o instrumento ou a textura que produz esse som.',
    instruments: 'Destaque de quatro a seis instrumentos.', voice: 'Descreva o registro, o timbre ou a interpretação vocal.',
    lyrics: 'Mova os versos e as marcações de seção para a letra.',
  },
  ja: {
    title: 'スタイルのヒント', language: '冒頭に歌唱言語、または「instrumental」を指定します。',
    tempo: 'テンポを指定するには末尾に「120 BPM」などと入力します。', length: 'スタイルは 12～50 語を目安に記述します。',
    numbers: '数値や単位で表した音を音楽用語で説明します。', negative: '「solo piano」のように、求める音を指定します。',
    instruction: '指示文を短い音楽表現に置き換えます。', artist: 'アーティストの音色や奏法を記述します。',
    genres: '編曲の中心となるジャンルを選びます。', quality: '音を生む楽器や質感を指定します。',
    instruments: '主要な楽器を 4～6 種類に絞ります。', voice: '歌声の音域、声質、歌い方を記述します。',
    lyrics: '歌詞とセクション名は歌詞欄に移します。',
  },
  ko: {
    title: '스타일 작성 팁', language: '보컬 언어나 “instrumental”로 시작하세요.',
    tempo: '템포를 지정하려면 끝에 “120 BPM”과 같이 적으세요.', length: '전체 스타일을 12~50단어로 설명해 보세요.',
    numbers: '수치와 단위 대신 소리의 특징을 음악 용어로 표현하세요.', negative: '“solo piano”처럼 원하는 소리를 적으세요.',
    instruction: '지시문을 짧은 음악적 표현으로 바꾸세요.', artist: '아티스트의 음색이나 연주 방식을 설명하세요.',
    genres: '편곡의 중심이 될 장르를 선택하세요.', quality: '원하는 소리를 내는 악기나 질감을 적으세요.',
    instruments: '강조할 악기를 4~6개 고르세요.', voice: '보컬의 음역, 음색이나 창법을 설명하세요.',
    lyrics: '가사와 구간 표시는 가사 입력란으로 옮기세요.',
  },
  zh: {
    title: '风格建议', language: '开头写演唱语言，纯音乐可写“instrumental”。',
    tempo: '指定速度时，可在结尾写上“120 BPM”。', length: '试着用 12–50 个词完整描述风格。',
    numbers: '用音乐术语描述数字或计量单位所代表的声音。', negative: '写出想要的声音，例如“solo piano”。',
    instruction: '把指令改写成简短的音乐描述。', artist: '描述这位音乐人的音色或演奏方式。',
    genres: '选择一种主要流派作为编曲方向。', quality: '写出产生这种声音的乐器或质感。',
    instruments: '选出四至六种主要乐器。', voice: '描述人声的音域、音色或演唱方式。',
    lyrics: '将歌词和段落标记移到歌词栏。',
  },
};
