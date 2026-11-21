import type { Language } from './translations';

const assets = {
  'music-train': 'trainer',
  'ace-caption': 'captioner',
  'yue2-convrot-base': 'base',
  'yue2-vae-encoder': 'vae',
  'yue2-semantic-tokenizer': 'tokenizer',
  'yue2-sheetsage': 'score',
  'yue2-lyric-aligner': 'aligner',
  'moss-audio': 'moss',
  'beat-this': 'tempo',
  'vocal-separator': 'separator',
} as const;

type AssetTitle = typeof assets[keyof typeof assets];

const labels: Record<Language, Record<AssetTitle, string>> = {
  en: {
    trainer: 'Trainer',
    captioner: 'Captioner',
    base: 'YuE2 base for training (INT8 ConvRot)',
    vae: 'YuE2 VAE with its encoder',
    tokenizer: 'Semantic tokenizer',
    score: 'SheetSage2 score transcriber',
    aligner: 'MMS forced aligner (lyric timing)',
    moss: 'MOSS-Music audio tower (F16)',
    tempo: 'Beat This! (tempo)',
    separator: 'HT-Demucs 6 stems',
  },
  ru: {
    trainer: 'Тренер',
    captioner: 'Описатель музыки',
    base: 'Базовая модель YuE2 для обучения (INT8 ConvRot)',
    vae: 'YuE2 VAE с энкодером',
    tokenizer: 'Семантический токенизатор',
    score: 'Распознавание партитуры SheetSage2',
    aligner: 'Выравнивание текста песни MMS',
    moss: 'Аудиоэнкодер MOSS-Music (F16)',
    tempo: 'Распознавание темпа Beat This!',
    separator: 'Разделение на 6 стемов HT-Demucs',
  },
  zh: {
    trainer: '训练器',
    captioner: '音乐描述器',
    base: 'YuE2 训练基础模型（INT8 ConvRot）',
    vae: '带编码器的 YuE2 VAE',
    tokenizer: '语义分词器',
    score: 'SheetSage2 乐谱转录',
    aligner: 'MMS 歌词时间对齐',
    moss: 'MOSS-Music 音频编码器（F16）',
    tempo: 'Beat This! 速度识别',
    separator: 'HT-Demucs 六轨人声伴奏分离',
  },
  ja: {
    trainer: 'トレーナー',
    captioner: '音楽キャプション生成器',
    base: '学習用 YuE2 ベースモデル（INT8 ConvRot）',
    vae: 'エンコーダー付き YuE2 VAE',
    tokenizer: 'セマンティックトークナイザー',
    score: 'SheetSage2 楽譜採譜',
    aligner: 'MMS 歌詞のタイミング合わせ',
    moss: 'MOSS-Music 音声エンコーダー（F16）',
    tempo: 'Beat This! テンポ検出',
    separator: 'HT-Demucs 6 ステム分離',
  },
  ko: {
    trainer: '학습 도구',
    captioner: '음악 설명 생성기',
    base: '학습용 YuE2 기본 모델 (INT8 ConvRot)',
    vae: '인코더를 포함한 YuE2 VAE',
    tokenizer: '의미 토크나이저',
    score: 'SheetSage2 악보 채보',
    aligner: 'MMS 가사 시간 정렬',
    moss: 'MOSS-Music 오디오 인코더 (F16)',
    tempo: 'Beat This! 템포 감지',
    separator: 'HT-Demucs 6개 스템 분리',
  },
  pt: {
    trainer: 'Treinador',
    captioner: 'Gerador de descrições musicais',
    base: 'Modelo base YuE2 para treino (INT8 ConvRot)',
    vae: 'VAE YuE2 com codificador',
    tokenizer: 'Tokenizador semântico',
    score: 'Transcritor de partituras SheetSage2',
    aligner: 'Alinhamento temporal da letra com MMS',
    moss: 'Codificador de áudio MOSS-Music (F16)',
    tempo: 'Deteção de tempo com Beat This!',
    separator: 'Separação de 6 pistas HT-Demucs',
  },
};

/** Keep the service labels for unknown IDs and the revision on older package responses. */
export function trainingAssetLabel(language: Language, file: { id: string; label: string }): string {
  if (!Object.hasOwn(assets, file.id)) return file.label;
  const key = assets[file.id as keyof typeof assets];
  const revision = key === 'trainer' || key === 'captioner' ? file.label.match(/ \(HOT-Step [0-9a-f]{8}\)$/i)?.[0] : '';
  return labels[language][key] + (revision ?? '');
}
