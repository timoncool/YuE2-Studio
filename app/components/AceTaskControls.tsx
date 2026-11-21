import { useI18n } from '../context/I18nContext';
import { useLibrarySongs } from '../services/studioQueries';

const tasks = ['text2music', 'cover', 'cover-nofsq', 'repaint'] as const;
type AceTask = typeof tasks[number];
export type AceCreateOptions = {
  task_type?: AceTask;
  source_song_id?: string;
  reference_song_id?: string;
  audio_cover_strength?: number;
  cover_noise_strength?: number;
  repainting_start?: number;
  repainting_end?: number;
  guidance_scale?: number;
  adapters?: { name: string; scale: number }[];
};
type AceTaskField = 'audio_cover_strength' | 'cover_noise_strength' | 'repainting_start' | 'repainting_end';
export type AceTaskDraft = { task: string; source: string; reference: string } & Partial<Record<AceTaskField, string>>;
export const newAceTask: AceTaskDraft = { task: 'text2music', source: '', reference: '' };

const coverFields = ['audio_cover_strength', 'cover_noise_strength'] as const;
const repaintFields = ['repainting_start', 'repainting_end'] as const;

const labels = {
  en: { task: 'ACE-Step task', text2music: 'New song', cover: 'Cover', 'cover-nofsq': 'Cover without FSQ', repaint: 'Repaint', source: 'Source song', reference: 'Timbre reference', choose: 'Choose from the library', audio_cover_strength: 'Cover strength', cover_noise_strength: 'Cover noise', repainting_start: 'Repaint start (seconds)', repainting_end: 'Repaint end (seconds)', end: 'Until the end', unsupported: 'Use the saved replay for this task.', invalid: 'Choose a source and use valid values for the selected task.' },
  ru: { task: 'Задача ACE-Step', text2music: 'Новая песня', cover: 'Кавер', 'cover-nofsq': 'Кавер без FSQ', repaint: 'Перерисовка', source: 'Исходная песня', reference: 'Референс тембра', choose: 'Выберите из библиотеки', audio_cover_strength: 'Сила кавера', cover_noise_strength: 'Шум кавера', repainting_start: 'Начало перерисовки, с', repainting_end: 'Конец перерисовки, с', end: 'До конца', unsupported: 'Для этой задачи используйте сохранённый повтор.', invalid: 'Выберите источник и заполните допустимые значения для задачи.' },
  zh: { task: 'ACE-Step 任务', text2music: '新歌曲', cover: '翻唱', 'cover-nofsq': '无 FSQ 翻唱', repaint: '重绘', source: '源歌曲', reference: '音色参考', choose: '从资料库选择', audio_cover_strength: '翻唱强度', cover_noise_strength: '翻唱噪声', repainting_start: '重绘开始（秒）', repainting_end: '重绘结束（秒）', end: '直至结尾', unsupported: '此任务请使用已保存的重放。', invalid: '请选择源歌曲并填写有效的任务参数。' },
  ja: { task: 'ACE-Step のタスク', text2music: '新しい曲', cover: 'カバー', 'cover-nofsq': 'FSQ なしのカバー', repaint: '部分再生成', source: '元の曲', reference: '音色の参照', choose: 'ライブラリから選択', audio_cover_strength: 'カバーの強さ', cover_noise_strength: 'カバーのノイズ', repainting_start: '再生成の開始（秒）', repainting_end: '再生成の終了（秒）', end: '曲の終わりまで', unsupported: 'このタスクでは保存済みの再生成を使ってください。', invalid: '元の曲と有効なタスク設定を指定してください。' },
  ko: { task: 'ACE-Step 작업', text2music: '새 곡', cover: '커버', 'cover-nofsq': 'FSQ 없는 커버', repaint: '구간 재생성', source: '원본 곡', reference: '음색 참조', choose: '라이브러리에서 선택', audio_cover_strength: '커버 강도', cover_noise_strength: '커버 노이즈', repainting_start: '재생성 시작(초)', repainting_end: '재생성 끝(초)', end: '곡 끝까지', unsupported: '이 작업은 저장된 재실행을 사용하세요.', invalid: '원본 곡을 선택하고 유효한 작업 값을 입력하세요.' },
  pt: { task: 'Tarefa do ACE-Step', text2music: 'Nova canção', cover: 'Cover', 'cover-nofsq': 'Cover sem FSQ', repaint: 'Refazer trecho', source: 'Canção de origem', reference: 'Referência de timbre', choose: 'Escolha na biblioteca', audio_cover_strength: 'Intensidade do cover', cover_noise_strength: 'Ruído do cover', repainting_start: 'Início do trecho (segundos)', repainting_end: 'Fim do trecho (segundos)', end: 'Até o fim', unsupported: 'Use a repetição salva para esta tarefa.', invalid: 'Selecione uma origem e use valores válidos para a tarefa.' },
};

function taskFields(task: string): readonly AceTaskField[] {
  if (task === 'repaint') return repaintFields;
  if (task === 'cover' || task === 'cover-nofsq') return coverFields;
  return [];
}

export function restoreAceTask(settings: Record<string, unknown>): AceTaskDraft {
  const text = (value: unknown) => typeof value === 'string' ? value : '';
  const parameters = [...coverFields, ...repaintFields].flatMap(key =>
    typeof settings[key] === 'number' ? [[key, key === 'repainting_end' && settings[key] === -1 ? '' : String(settings[key])]] : []);
  return {
    task: text(settings.task_type) || 'text2music',
    source: text(settings.source_song_id),
    reference: text(settings.reference_song_id),
    ...Object.fromEntries(parameters),
  };
}

export function aceTaskOptions(draft: AceTaskDraft): AceCreateOptions | undefined {
  const task = tasks.find(value => value === draft.task);
  if (!task || (task !== 'text2music' && !draft.source)) return undefined;
  const options: AceCreateOptions = {
    ...(task !== 'text2music' ? { task_type: task, source_song_id: draft.source } : {}),
    ...(draft.reference ? { reference_song_id: draft.reference } : {}),
  };
  for (const key of taskFields(task)) {
    const text = draft[key]?.trim();
    if (!text) continue;
    const value = Number(text);
    if (!Number.isFinite(value) || value < 0 || (task !== 'repaint' && value > 1)) return undefined;
    options[key] = value;
  }
  if (options.repainting_end !== undefined && options.repainting_end <= (options.repainting_start ?? 0)) return undefined;
  return options;
}

export function AceTaskControls({ value, onChange, control, profileReady }: {
  value: AceTaskDraft;
  onChange: (value: AceTaskDraft) => void;
  control: string;
  profileReady: boolean;
}) {
  const { language } = useI18n();
  const text = labels[language];
  const { data } = useLibrarySongs();
  const songs = (data ?? []).filter(song => song.audioUrl && !song.isGenerating);
  const known = tasks.some(task => task === value.task);
  const fields = taskFields(value.task);
  const sourceMissing = value.task !== 'text2music' && !value.source;
  const songSelect = (kind: 'source' | 'reference', required: boolean) => <label className="grid gap-1 text-xs">
    {text[kind]}
    <select className={control} aria-label={text[kind]} value={value[kind]} required={required}
      onChange={event => onChange({ ...value, [kind]: event.target.value })}>
      <option value="">{required ? text.choose : '—'}</option>
      {value[kind] && !songs.some(song => song.id === value[kind]) && <option value={value[kind]} disabled>{value[kind]}</option>}
      {songs.map(song => <option key={song.id} value={song.id}>{song.title}</option>)}
    </select>
  </label>;
  return <div className="space-y-2">
    <label className="grid gap-1 text-xs">
      {text.task}
      <select className={control} aria-label={text.task} value={value.task}
        onChange={event => onChange({ ...value, task: event.target.value })}>
        {!known && <option value={value.task} disabled>{value.task}</option>}
        {tasks.map(task => <option key={task} value={task}>{text[task]}</option>)}
      </select>
    </label>
    {known ? <>
      {value.task !== 'text2music' && songSelect('source', true)}
      {songSelect('reference', false)}
      {fields.length > 0 && <div className="grid grid-cols-2 gap-2">
        {fields.map(key => <label key={key} className="min-w-0 text-xs">
          {text[key]}
          <input aria-label={text[key]} className={control} type="number" min={0}
            max={value.task === 'repaint' ? undefined : 1} step="any"
            placeholder={key === 'repainting_end' ? text.end : key === 'repainting_start' ? '0' : undefined}
            value={value[key] ?? ''} onChange={event => onChange({ ...value, [key]: event.target.value })} />
        </label>)}
      </div>}
      {profileReady && !sourceMissing && !aceTaskOptions(value) && <p role="alert" className="text-xs">{text.invalid}</p>}
    </> : <p role="status" className="text-xs">{text.unsupported}</p>}
  </div>;
}
