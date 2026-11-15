import { ptMidiImportOptions } from './ptExtra';

export const midiImportOptions = {
  en: { grid: 'Timing grid', auto: 'Automatic', voice: 'Voice: additional octaves', instrument: 'Instrument: additional octaves', hint: 'Each score line keeps the top note of overlapping notes. Octaves are applied after automatic range fitting.', keep: 'Keep the edited score for new words', keepHint: 'Allow this tune to be sung after the style or lyrics change.' },
  ru: { grid: 'Сетка времени', auto: 'Автоматически', voice: 'Голос: дополнительные октавы', instrument: 'Инструмент: дополнительные октавы', hint: 'Каждая линия партитуры сохраняет верхнюю ноту наложенных нот. Октавы добавляются после автоматической подгонки диапазона.', keep: 'Сохранить изменённую партитуру для новых слов', keepHint: 'Разрешить петь эту мелодию после изменения стиля или текста.' },
  zh: { grid: '时间网格', auto: '自动', voice: '人声：额外八度', instrument: '乐器：额外八度', hint: '每条乐谱线保留重叠音符中最高的音。额外八度在自动音域调整后应用。', keep: '将编辑的乐谱用于新歌词', keepHint: '允许在更改风格或歌词后继续演唱这段旋律。' },
  ja: { grid: '時間グリッド', auto: '自動', voice: '声：追加オクターブ', instrument: '楽器：追加オクターブ', hint: '各声部は重なった音の最高音を保持します。オクターブは自動音域調整後に適用されます。', keep: '編集した楽譜を新しい歌詞にも使う', keepHint: 'スタイルや歌詞を変えた後も、この旋律を歌唱できます。' },
  ko: { grid: '시간 그리드', auto: '자동', voice: '목소리: 추가 옥타브', instrument: '악기: 추가 옥타브', hint: '각 악보 파트는 겹친 음의 최고음을 유지합니다. 옥타브는 자동 음역 조정 후 적용됩니다.', keep: '편집한 악보를 새 가사에도 사용', keepHint: '스타일이나 가사를 바꾼 후에도 이 멜로디를 부를 수 있습니다.' },
  pt: ptMidiImportOptions,
};
