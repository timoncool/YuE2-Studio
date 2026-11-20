import type { Language } from './translations';

const en = {
  title: 'Songbook', transpose: 'Transpose (semitones)', capo: 'Capo fret', instrument: 'Chord shapes',
  guitar: 'Guitar', ukulele: 'Ukulele', numbered: 'Numbered notation (jianpu)', print: 'Print A4',
  close: 'Close songbook', key: 'Key', unavailable: 'This score could not be opened.',
  lyricLanguage: 'Lyrics language', approximate: 'Estimated lyric alignment',
  unaligned: 'Lyrics without score alignment', noShape: 'No supported chord shape for',
};

export const songbookStrings: Record<Language, typeof en> = {
  en,
  ru: {
    title: 'Песенник', transpose: 'Транспозиция (полутоны)', capo: 'Лад каподастра', instrument: 'Аппликатуры',
    guitar: 'Гитара', ukulele: 'Укулеле', numbered: 'Цифровая нотация (цзяньпу)', print: 'Печать A4',
    close: 'Закрыть песенник', key: 'Тональность', unavailable: 'Не удалось открыть партитуру.',
    lyricLanguage: 'Язык текста', approximate: 'Примерная привязка слов к нотам',
    unaligned: 'Текст без привязки к нотам', noShape: 'Нет подходящей аппликатуры для',
  },
  zh: {
    title: '歌谱', transpose: '移调（半音）', capo: '变调夹品位', instrument: '和弦指法',
    guitar: '吉他', ukulele: '尤克里里', numbered: '简谱', print: 'A4 打印',
    close: '关闭歌谱', key: '调号', unavailable: '无法打开此乐谱。',
    lyricLanguage: '歌词语言', approximate: '歌词与音符的对齐仅供参考',
    unaligned: '未与音符对齐的歌词', noShape: '暂无以下和弦的指法：',
  },
  ja: {
    title: '歌集', transpose: '移調（半音）', capo: 'カポのフレット', instrument: 'コードの押さえ方',
    guitar: 'ギター', ukulele: 'ウクレレ', numbered: '数字譜（簡譜）', print: 'A4で印刷',
    close: '歌集を閉じる', key: '調', unavailable: '楽譜を開けませんでした。',
    lyricLanguage: '歌詞の言語', approximate: '歌詞と音符のおおよその対応',
    unaligned: '音符に割り当てられていない歌詞', noShape: '対応する押さえ方がありません',
  },
  ko: {
    title: '노래책', transpose: '조옮김 (반음)', capo: '카포 프렛', instrument: '코드 운지',
    guitar: '기타', ukulele: '우쿨렐레', numbered: '숫자보 (간보)', print: 'A4 인쇄',
    close: '노래책 닫기', key: '조', unavailable: '악보를 열 수 없습니다.',
    lyricLanguage: '가사 언어', approximate: '추정된 가사와 음표의 대응',
    unaligned: '음표에 맞춰지지 않은 가사', noShape: '지원하는 코드 운지가 없습니다',
  },
  pt: {
    title: 'Cancioneiro', transpose: 'Transpor (semitons)', capo: 'Casa do capo', instrument: 'Posições dos acordes',
    guitar: 'Guitarra', ukulele: 'Ukulele', numbered: 'Notação numérica (jianpu)', print: 'Imprimir em A4',
    close: 'Fechar cancioneiro', key: 'Tonalidade', unavailable: 'Não foi possível abrir a partitura.',
    lyricLanguage: 'Idioma da letra', approximate: 'Alinhamento estimado entre letra e notas',
    unaligned: 'Letra sem alinhamento com as notas', noShape: 'Sem posição compatível para',
  },
};
