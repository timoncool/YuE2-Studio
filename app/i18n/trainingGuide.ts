import type { Language } from './translations';
import { ptTrainingGuide } from './ptExtra';

/**
 * The training guide of YuE2 Studio, per language. It follows the studio's own
 * training pipeline and the notes of the HOT-Step trainer it runs.
 */

export interface GuideSection {
  title: string;
  text?: string[];
  steps?: string[];
  list?: string[];
  checklist?: string[];
  examples?: { label?: string; body: string }[];
}

export interface Guide {
  title: string;
  intro?: string;
  expandAll: string;
  collapseAll: string;
  close: string;
  resize: string;
  sections: GuideSection[];
}

const ru: Guide = {
  title: 'Справка по обучению LoRA',
  intro: 'LoRA — небольшое дополнение к модели, которое учится звучать как ваши песни: манера, вокал, аранжировки, продакшн. Окно можно двигать за заголовок и растягивать за правый нижний угол.',
  expandAll: 'Раскрыть всё',
  collapseAll: 'Свернуть всё',
  close: 'Закрыть',
  resize: 'Потяните, чтобы изменить размер',
  sections: [
    {
      title: "Порядок работы",
      steps: [
        "Перетащите папку с песнями одного исполнителя или стиля в зону на странице «Обучение» или выберите папку и файлы кнопками. Студия сама создаст набор с именем папки. Подходят WAV, MP3, FLAC, OGG, M4A, AIFF; альбом одним файлом с .cue режется на песни, текст из .txt или .lrc рядом берётся как есть.",
        "Шаг «1 · Песни»: подготовка начинается сама. Студия берёт тексты из баз, отделяет вокал и распознаёт только те, что не нашлись, слушает каждую песню и пишет её стиль с измеренным темпом. У каждой песни свой статус, общий ход — сверху.",
        "Каждая песня хранит, на чём остановилась. Если студию закрыть или она упадёт посреди подготовки, после запуска работа продолжится сама с того же места, сделанное не повторяется. У песни с ошибкой есть кнопка «Повторить», у недоделанной — «Доделать»: они доделывают только эту песню.",
        "Проверьте результат: нажмите на песню — она раскроется с плеером, стилем и текстом. Поправьте, что нужно; «Описать заново» и «Найти заново» переделывают только эту песню.",
        "Шаг «2 · Обучение»: название LoRA, слово-триггер (студия сама делает редкое слово из названия набора, например nrmnkhffn для «Нейромонах Феофан»; его можно поменять или стереть), проверка готовности и кнопка «Обучить». Файлы обучения (около 8.6 ГБ, один раз) скачиваются здесь же. Нужна NVIDIA RTX 30-й серии или новее и около 11 ГБ видеопамяти.",
        "Можно не ждать: пока идёт подготовка, поставьте галочку «Начать обучение самому, когда все песни будут готовы».",
        "Шаг «3 · Результат»: послушайте чекпоинты и нажмите «В LoRA» под лучшим — он появится на странице LoRA. Пока идёт обучение, генерация, ассистент, караоке и разделение на дорожки недоступны.",
      ],
    },
    {
      title: "Автоописание песен",
      text: [
        "Необязательный пакет (около 10.5 ГБ), скачивается один раз карточкой «Автоописание песен» на шаге «Песни». Без него студия всё равно распознает тексты, а стиль придётся написать самому.",
        "Каждая модель загружается один раз на весь набор и выгружается, когда её этап закончен: базы текстов, затем вокал и распознавание для ненайденного, затем прослушивание, затем ассистент. На видеокарте всегда одна модель. Поэтому набор из пятидесяти песен готовится не в пятьдесят раз дольше одной.",
      ],
      list: [
        "MOSS-Music-8B — модель, которая слушает песню и описывает, что в ней звучит: жанр, вокал, инструменты, настроение, продакшн. Тот же способ использует автор тренера HOT-Step: описание по звуку заметно точнее, чем по названию.",
        "Beat This! — нейросеть, которая находит удары в записи; по ним студия считает темп. На песнях Монеточки совпала с Tunebat и SongBPM до 1 BPM.",
        "Ассистент студии собирает из этого одну строку стиля в формате YuE2 и ставит в конец измеренный темп. MOSS сам ошибается в темпе, поэтому его число всегда заменяется измеренным. Тональность в стиль YuE2 не пишется: её выбирает сама модель, когда пишет партитуру.",
        "MOSS слушает до 7 минут песни, и чем дольше, тем больше видеопамяти: от 11 ГБ за минуту до 18 ГБ за семь. На карте поменьше он слушает середину песни столько, сколько помещается; на карте меньше 12 ГБ — свою сборку поменьше, Q4_K_M; на карте меньше 8 ГБ или без CUDA 13 — на процессоре, несколько минут на песню. На RTX 4090 одна песня описывается за 6–7 секунд. Всё работает на вашем компьютере, ничего никуда не отправляется.",
        "Результат всегда проверяйте: модель может ошибиться с жанром или инструментами — поправьте строку руками.",
      ],
    },
    {
      title: 'Какие песни брать',
      list: [
        'Один исполнитель или один узкий стиль. Сборная солянка учит «ничему конкретному».',
        'Обычно берут от 5 до 20 песен. Автор тренера не заметил надёжной разницы между 10 и 20 треками; плотные по тексту или разностилевые альбомы учатся тяжелее.',
        'Ровное качество записей: студийные версии, без концертных шумов, радио-джинглов и обрезанных фрагментов.',
        'Целые песни: тренер сам режет их на кусочки по 10 секунд.',
        'Не добавляйте одну и ту же песню дважды (оригинал и ремастер) — это перекос в её сторону.',
      ],
    },
    {
      title: 'Стиль — что писать',
      text: [
        'Одно предложение на английском, примерно 35–70 слов, в таком порядке: язык → жанр и эпоха → вокал → инструменты → настроение → продакшн → темп «N BPM».',
        'Не пишите имя исполнителя, название песни, тональность и размер, не цитируйте текст песни. Для инструментала вместо языка пишите «instrumental».',
        'BPM лучше взять из анализатора или сервиса с базой треков, а не на глаз: неверный темп учит неверному.',
        'Слово-триггер в стиль писать не нужно — студия добавит его сама.',
      ],
      examples: [
        {
          label: 'Пример',
          body: 'Russian-language 2010s indie pop with bright synth pop touches, young female lead vocal with a playful ironic delivery and light doubled harmonies, analog synths, drum machine and clean electric guitar, carefree yet slightly melancholic mood, crisp modern bedroom-pop production with a warm low end, 120 BPM',
        },
      ],
    },
    {
      title: "Текст песни",
      list: [
        "Точно те слова, что поются, — без аккордов, ссылок и примечаний. Текст с сайтов обязательно сверьте с записью.",
        "Размечайте части: [Verse 1], [Chorus], [Verse 2], [Bridge], [Outro] — каждая часть с новой строки.",
        "Файл .txt или .lrc с тем же именем, что и аудио, подхватывается при добавлении; таймкоды из .lrc убираются.",
        "Если текста нет, студия сама ищет его в открытых базах текстов — LRCLIB, QQ Music, Kugou — по исполнителю, названию и длительности (исполнитель и название берутся из тегов файла или из имени и папок). Только если ни одна база песню не знает, отделяется вокал и текст распознаёт Whisper — это заметно менее точно. Над текстом видно, откуда он взят. Результат всегда проверяйте.",
        "Если распознаватель не услышал слов, песня помечается инструменталом. Если это ошибка, снимите галочку «Инструментал» и нажмите «Найти заново».",
      ],
    },
    {
      title: "Что студия делает сама",
      list: [
        "Переводит всё в WAV 48 кГц и режет на 10-секундные кусочки.",
        "Строит партитуру каждой песни (SheetSage2).",
        "Подставляет слово-триггер в обучение, а при генерации — в стиль, когда вы выбираете эту LoRA.",
        "Сохраняет чекпоинт каждые 10 шагов и в конце обучения.",
        "В рецепте «Как раньше» ещё отделяет вокал у песен с текстом, привязывает слова к времени по вокалу (модель учит, где какое слово поётся) и сама останавливает обучение, когда LoRA достаточно похожа (по метрике KL).",
      ],
    },
    {
      title: "Что нужно сделать самому",
      list: [
        "Подобрать песни и проверить их качество.",
        "Проверить стили и тексты, которые написала студия: модель и распознавание ошибаются. Без пакета автоописания стиль пишется вручную.",
        "Без пакета автоописания — узнать темп (BPM); с ним студия измеряет его сама.",
        "Выбрать чекпоинт на слух.",
      ],
    },
    {
      title: "Настройки запуска",
      text: [
        "По умолчанию — рецепт base-matched, основной у автора тренера HOT-Step с 27 сентября 2026: LoRA учится так же, как по техническому отчёту модели обучали саму базовую модель. Без причины настройки лучше не трогать.",
      ],
      list: [
        "Рецепт задаёт объём: «Быстро» — 50 шагов по 4 песни, «Сбалансированно» — 100 шагов по 4 песни (по умолчанию: у автора тренера на целом альбоме сходство вышло на максимум к сотому шагу), «Тщательно» — 200 шагов по 8 песен, примерно вчетверо дольше.",
        "Внутри рецепта: AdamW со скоростью 1e-4 и коротким разгоном, к концу скорость плавно снижается до 10%; композиция учится с весом 0.25 и только на том, что модель генерирует сама; в 10% шагов нет стиля, в 10% — текста, в 10% — обоих; звуковая половина видит песню целиком. Тайминги текста не нужны — отделение вокала и выравнивание пропускаются, громкость записей остаётся как есть.",
        "LoKr 64 / фактор 4 / alpha 256 — лёгкий адаптер (около 106 МБ); можно переключить на LoRA.",
        "«Как раньше: стоп по KL» — прежний рецепт студии: Prodigy, остановка на KL = 1.4 (сходство начинается около 1.25, около 1.9 модель портится), предел 750 шагов, чекпоинт каждые 50 шагов, тайминги текста (нужен разделитель вокала), остановку можно переключить на «по эпохам».",
        "Запуск, начатый в прошлой версии студии, дообучается тем же рецептом, каким начинался.",
      ],
    },
    {
      title: 'Выбор чекпоинта и генерация',
      list: [
        'Выбирайте на слух, а не по графику ошибки: сгенерируйте одну и ту же песню с разными чекпоинтами.',
        'Нажмите «В LoRA» под нужным шагом — LoRA появится на странице LoRA с именем «запуск · шаг».',
        'При выборе LoRA на странице «Создать» триггер подставится сам.',
        'У LoRA две силы: композиция (AR) и звук (NAR), по умолчанию 1 и 1. Автор тренера на слух получал лучший звук при NAR около 2.',
      ],
    },
    {
      title: "Если что-то не так",
      list: [
        "Песни зацикливаются, нет концовки, вокал разваливается — LoRA перетренирована: возьмите более ранний чекпоинт или уменьшите силу.",
        "LoRA почти ничего не меняет — возьмите более поздний чекпоинт или рецепт «Тщательно», проверьте стиль и тексты, добавьте песен.",
        "В рецепте «Как раньше» слова «съезжают» с музыки — проверьте тексты: лишние строки и повторы, которых нет в записи, сбивают тайминги.",
        "Обучение «Как раньше» не стартует с ошибкой про разделитель — установите разделитель вокала или выключите тайминги текста.",
      ],
    },
    {
      title: 'Чеклист перед запуском',
      checklist: [
        'Песни одного исполнителя или стиля, 5–20 штук, хорошего качества.',
        'Нет дублей и обрезков.',
        'У каждой песни стиль одним предложением на английском, с BPM, без имени и названия.',
        'Тексты сверены с записью и размечены [Verse] / [Chorus].',
        'Инструменталы отмечены галочкой, у песен с вокалом галочка снята.',
        'Нет жёлтых треугольников.',
        'Задано редкое слово-триггер.',
        'Свободно около 11 ГБ видеопамяти, генерация остановлена.',
      ],
    },
  ],
};

const en: Guide = {
  title: 'LoRA training guide',
  intro: 'A LoRA is a small add-on to the model that learns to sound like your songs: the manner, the vocals, the arrangements, the production. Drag this window by its title and resize it from the bottom-right corner.',
  expandAll: 'Expand all',
  collapseAll: 'Collapse all',
  close: 'Close',
  resize: 'Drag to resize',
  sections: [
    {
      title: "Workflow",
      steps: [
        "Drop a folder of songs by one artist or in one style onto the Training page, or pick a folder or files with the buttons. The studio makes a dataset named after the folder. WAV, MP3, FLAC, OGG and M4A work; an album in one file with a .cue is cut into its songs, lyrics in a .txt or .lrc beside a song are taken as they are.",
        "Step 1 · Songs: preparation starts by itself. The studio takes the lyrics from the databases, separates the vocals and recognises only the songs they do not know, listens to every song and writes its style with the measured tempo. Every song shows its own status, the overall progress is on top.",
        "Every song keeps where it stopped. If the studio is closed or crashes during preparation, the work carries on by itself from the same place at the next start, and nothing done is done again. A song that failed has a Retry button, an unfinished one Finish this song: they finish just that song.",
        "Check the result: click a song and it opens with a player, its style and its lyrics. Fix what needs fixing; Describe again and Find again redo just that song.",
        "Step 2 · Training: the LoRA name, a trigger word (the studio makes a rare word from the dataset name, such as nrmnkhffn for \"Нейромонах Феофан\"; change or clear it as you like), the readiness check and the Train button. The training files (about 8.6 GB, once) download right there. It needs an NVIDIA RTX 30-series card or newer and about 11 GB of video memory.",
        "No need to wait: while preparation runs, tick \"Start training by itself when every song is ready\".",
        "Step 3 · Result: listen to the checkpoints and press To LoRA under the best one; it appears on the LoRA page. While training runs, generation, the assistant, karaoke and stem separation are unavailable.",
      ],
    },
    {
      title: "Auto-describing songs",
      text: [
        "An optional pack (about 10.5 GB), downloaded once from the Auto-describe songs card on the Songs step. Without it the studio still recognises the lyrics, and you write the style yourself.",
        "Each model loads once for the whole dataset and is let go when its stage is done: the lyric databases, then vocals and recognition for what they miss, then listening, then the assistant. The card holds one model at a time. So a dataset of fifty songs does not take fifty times as long as one.",
      ],
      list: [
        "MOSS-Music-8B is a model that listens to a song and describes what is in it: genre, vocals, instruments, mood, production. The author of the HOT-Step trainer does it the same way: a description by ear is far more accurate than one by title.",
        "Beat This! is a network that finds the beats in a recording; the studio works out the tempo from them. On Monetochka's songs it matched Tunebat and SongBPM within 1 BPM.",
        "The studio's assistant turns all this into one YuE2 style line and puts the measured tempo at its end. MOSS gets the tempo wrong on its own, so its number is always replaced with the measured one. The key is not written into a YuE2 style: the model picks it itself when it writes the score.",
        "MOSS hears up to 7 minutes of a song, and the longer it hears, the more video memory it takes: from 11 GB for a minute to 18 GB for seven. On a smaller card it hears as much of the middle of the song as fits; on a card under 12 GB its smaller Q4_K_M build; on a card under 8 GB or without CUDA 13 it runs on the processor, a few minutes a song. On an RTX 4090 a song is described in 6–7 seconds. Everything runs on your computer, nothing is sent anywhere.",
        "Always check the result: the model can get the genre or instruments wrong; fix the line by hand.",
      ],
    },
    {
      title: 'Which songs to use',
      list: [
        'One artist or one narrow style. A mixed bag teaches nothing in particular.',
        'Usually 5 to 20 songs. The trainer\'s author saw no reliable difference between 10 and 20 tracks; lyric-dense or mixed-style albums are harder to learn.',
        'Even recording quality: studio versions, no live noise, radio jingles or cut-off fragments.',
        'Whole songs: the trainer cuts them into 10-second pieces itself.',
        'Do not add the same song twice (original and remaster) — it tilts the LoRA towards it.',
      ],
    },
    {
      title: 'Style — what to write',
      text: [
        'One English sentence of about 35–70 words, in this order: language → genre and era → vocals → instruments → mood → production → tempo as "N BPM".',
        'Do not name the artist or the song, do not give the key or the time signature, do not quote the lyrics. For an instrumental write "instrumental" in place of the language.',
        'Take the BPM from an analyser or a track database rather than guessing: a wrong tempo teaches the wrong thing.',
        'Do not write the trigger word in the style — the studio adds it itself.',
      ],
      examples: [
        {
          label: 'Example',
          body: 'Russian-language 2010s indie pop with bright synth pop touches, young female lead vocal with a playful ironic delivery and light doubled harmonies, analog synths, drum machine and clean electric guitar, carefree yet slightly melancholic mood, crisp modern bedroom-pop production with a warm low end, 120 BPM',
        },
      ],
    },
    {
      title: "Lyrics",
      list: [
        "Exactly the words that are sung, without chords, links or notes. Always check lyrics from websites against the recording.",
        "Mark the parts: [Verse 1], [Chorus], [Verse 2], [Bridge], [Outro], each part on a new line.",
        "A .txt or .lrc with the same name as the audio is taken when the song is added; the time stamps of an .lrc are removed.",
        "Where there are no lyrics, the studio looks them up in open lyrics databases - LRCLIB, QQ Music, Kugou - by artist, title and length (artist and title come from the file's tags, or its name and folders). Only when no database knows the song are the vocals separated and the words recognised by Whisper, which is far less accurate. Above the lyrics it says where they came from. Always check the result.",
        "If the recogniser hears no words, the song is marked instrumental. If that is wrong, untick Instrumental and press Find again.",
      ],
    },
    {
      title: "What the studio does itself",
      list: [
        "Converts everything to 48 kHz WAV and cuts it into 10-second pieces.",
        "Builds a score for every song (SheetSage2).",
        "Puts the trigger word into training, and into the style when you pick this LoRA for a song.",
        "Saves a checkpoint every 10 steps and at the end of training.",
        "Under the As before recipe it also separates the vocals of songs with lyrics, ties the words to time on the vocals (the model learns where each word is sung), and stops training by itself once the LoRA is close enough (by the KL metric).",
      ],
    },
    {
      title: "What you have to do yourself",
      list: [
        "Choose the songs and check their quality.",
        "Check the styles and lyrics the studio wrote: the model and the recognition make mistakes. Without the auto-describe pack the style is written by hand.",
        "Without the auto-describe pack, find the tempo (BPM); with it the studio measures it itself.",
        "Choose a checkpoint by ear.",
      ],
    },
    {
      title: "Run settings",
      text: [
        "The default is the base-matched recipe, the main one of the HOT-Step trainer author since 27 September 2026: the LoRA learns the way the base model itself was trained, per the model technical report. Leave the settings alone unless you have a reason.",
      ],
      list: [
        "The recipe sets the size: Fast is 50 steps of 4 songs, Balanced 100 steps of 4 songs (the default: on a full album the trainer author heard likeness peak by step 100), Thorough 200 steps of 8 songs, about four times as long.",
        "Inside the recipe: AdamW at 1e-4 with a short warmup, easing down to 10% of the rate by the end; the composition learns at weight 0.25 and only on what the model generates itself; 10% of steps leave out the style, 10% the lyrics, 10% both; the sound half sees the whole song. No lyric timing is needed, so vocal separation and alignment are skipped, and the recordings keep their loudness.",
        "LoKr 64 / factor 4 / alpha 256 — a light adapter (about 106 MB); LoRA can be picked instead.",
        "As before: stop by KL is the studio previous recipe: Prodigy, stop at KL = 1.4 (likeness starts around 1.25, the model degrades around 1.9), a 750-step cap, a checkpoint every 50 steps, lyric timing (needs the vocal separator); the stop can be switched to by epochs.",
        "A run started in an earlier version of the studio trains further with the recipe it started with.",
      ],
    },
    {
      title: 'Choosing a checkpoint and generating',
      list: [
        'Choose by ear, not by the loss chart: generate the same song with different checkpoints.',
        'Press "To LoRA" under the step you want — the LoRA appears on the LoRA page as "run · step".',
        'When you pick the LoRA on the Create page, the trigger is added by itself.',
        'The LoRA has two strengths: composition (AR) and sound (NAR), 1 and 1 by default. The trainer\'s author heard the best sound with NAR around 2.',
      ],
    },
    {
      title: "When something is wrong",
      list: [
        "Songs loop, have no ending, the vocal falls apart — the LoRA is overtrained: take an earlier checkpoint or lower the strength.",
        "The LoRA changes almost nothing — take a later checkpoint or the Thorough recipe, check the styles and lyrics, add songs.",
        "Under As before the words drift off the music — check the lyrics: extra lines and repeats that are not in the recording throw the timing off.",
        "As before training will not start with an error about the separator — install the vocal separator or turn lyric timing off.",
      ],
    },
    {
      title: 'Checklist before a run',
      checklist: [
        'Songs of one artist or style, 5–20 of them, of good quality.',
        'No duplicates or fragments.',
        'Every song has a one-sentence English style with BPM, without the artist or the title.',
        'Lyrics checked against the recording and marked [Verse] / [Chorus].',
        'Instrumentals ticked, songs with vocals unticked.',
        'No yellow triangles.',
        'A rare trigger word is set.',
        'About 11 GB of VRAM free, generation stopped.',
      ],
    },
  ],
};

const zh: Guide = {
  title: 'LoRA 训练指南',
  intro: 'LoRA 是模型的一个小附加件，它学习你的歌曲的声音：演唱方式、人声、编曲和制作。可以拖动标题栏移动窗口，拖动右下角调整大小。',
  expandAll: '全部展开',
  collapseAll: '全部收起',
  close: '关闭',
  resize: '拖动以调整大小',
  sections: [
    {
      title: "操作流程",
      steps: [
        "把同一艺人或同一风格的歌曲文件夹拖到“训练”页面，或用按钮选择文件夹和文件。工作室会以文件夹名自动建立数据集。支持 WAV、MP3、FLAC、OGG、M4A、AIFF；带 .cue 的整轨专辑会切成单曲，旁边 .txt 或 .lrc 中的歌词直接采用。",
        "第 1 步“歌曲”：准备会自动开始。工作室先从歌词库取词，只对找不到的歌分离人声并识别，聆听每首歌，并用测得的速度写出风格。每首歌都有自己的状态，整体进度在上方。",
        "每首歌都会记住进行到哪一步。如果准备过程中关闭工作室或它崩溃，下次启动时会从同一处自动继续，已完成的不会重做。出错的歌有“重试”按钮，未完成的有“完成这首”按钮，只处理这一首。",
        "检查结果：点击一首歌，它会展开播放器、风格和歌词。按需修改；“重新描述”和“重新查找”只重做这一首。",
        "第 2 步“训练”：LoRA 名称、触发词（工作室会用数据集名称生成一个少见的词，可以修改或清空）、就绪检查和“训练”按钮。训练文件（约 8.6 GB，只需一次）就在这里下载。需要 NVIDIA RTX 30 系列或更新的显卡和约 11 GB 显存。",
        "不必等待：准备进行时勾选“所有歌曲就绪后自动开始训练”。",
        "第 3 步“结果”：试听检查点，在最好的那个下面点“加入 LoRA”，它会出现在 LoRA 页面。训练期间无法生成、使用助手、卡拉OK和分轨。",
      ],
    },
    {
      title: "自动描述歌曲",
      text: [
        "可选的包（约 10.5 GB），在“歌曲”步骤的“自动描述歌曲”卡片中下载一次。没有它，工作室仍会识别歌词，但风格需要你自己写。",
        "每个模型对整个数据集只加载一次，并在其阶段结束后释放：先查歌词库，再对找不到的歌分离人声并识别，然后聆听，最后是助手。显卡上始终只有一个模型。因此五十首歌的数据集并不需要单首的五十倍时间。",
      ],
      list: [
        "MOSS-Music-8B 会聆听歌曲并描述其中的内容：流派、人声、乐器、情绪、制作。HOT-Step 训练器作者也这样做：按声音描述比按标题准确得多。",
        "Beat This! 是在录音中寻找节拍的网络，工作室据此计算速度。在 Monetochka 的歌曲上，它与 Tunebat 和 SongBPM 相差不到 1 BPM。",
        "工作室的助手把这些整理成一行 YuE2 风格，并在末尾写上测得的速度。MOSS 自己给出的速度常常出错，所以总是替换为测得的数值。调性不写进 YuE2 风格：模型在写乐谱时自己决定。",
        "MOSS 最多聆听歌曲的 7 分钟，听得越长占用的显存越多：1 分钟约 11 GB，7 分钟约 18 GB。显存较小的显卡只聆听歌曲中间能容纳的部分；低于 12 GB 的显卡使用较小的 Q4_K_M 版本；低于 8 GB 或不支持 CUDA 13 的显卡改用处理器运行，每首歌需要几分钟。在 RTX 4090 上每首歌约 6–7 秒。一切都在你的电脑上运行，不会发送到任何地方。",
        "请务必检查结果：模型可能弄错流派或乐器，请手动修改。",
      ],
    },
    {
      title: '选择哪些歌曲',
      list: [
        '同一艺人或同一窄风格。大杂烩学不到任何具体的东西。',
        '通常 5 到 20 首。训练器作者没有发现 10 首和 20 首之间有可靠差别；歌词密集或风格混杂的专辑更难学。',
        '录音质量一致：录音室版本，不要现场噪音、电台片头或截断的片段。',
        '使用完整歌曲：训练器会自己切成 10 秒的片段。',
        '不要把同一首歌加两次（原版和重制版）——会让 LoRA 偏向它。',
      ],
    },
    {
      title: '风格——写什么',
      text: [
        '一句英文，约 35–70 个词，按此顺序：语言 → 流派和年代 → 人声 → 乐器 → 情绪 → 制作 → 速度“N BPM”。',
        '不要写艺人名、歌名、调性和拍号，不要引用歌词。纯音乐在语言的位置写“instrumental”。',
        'BPM 最好来自分析工具或曲库，不要凭感觉：错误的速度会教错东西。',
        '不要在风格里写触发词——工作室会自动添加。',
      ],
      examples: [
        {
          label: '示例',
          body: 'Russian-language 2010s indie pop with bright synth pop touches, young female lead vocal with a playful ironic delivery and light doubled harmonies, analog synths, drum machine and clean electric guitar, carefree yet slightly melancholic mood, crisp modern bedroom-pop production with a warm low end, 120 BPM',
        },
      ],
    },
    {
      title: "歌词",
      list: [
        "准确写出唱的词，不要和弦、链接或注释。网站上的歌词务必对照录音核对。",
        "标注段落：[Verse 1]、[Chorus]、[Verse 2]、[Bridge]、[Outro]，每段另起一行。",
        "与音频同名的 .txt 或 .lrc 会在添加时读取；.lrc 中的时间标记会被去掉。",
        "没有歌词时，工作室会按艺人、标题和时长在公开歌词库（LRCLIB、QQ 音乐、酷狗）中查找（艺人和标题取自文件标签，或文件名和文件夹）。只有所有歌词库都不认识这首歌时，才会分离人声并用 Whisper 识别，准确度明显更低。歌词上方会显示来源。请务必检查结果。",
        "如果识别器没听到歌词，这首歌会被标为纯音乐。如果不对，取消“纯音乐”并点“重新查找”。",
      ],
    },
    {
      title: "工作室自动完成的事",
      list: [
        "把所有音频转为 48 kHz WAV 并切成 10 秒的片段。",
        "为每首歌生成乐谱（SheetSage2）。",
        "把触发词放进训练；生成时选择这个 LoRA，它也会自动加到风格里。",
        "每 10 步以及训练结束时保存检查点。",
        "在“沿用旧法”配方中，还会分离有歌词歌曲的人声，按人声把歌词对齐到时间（模型学习每个词在哪里唱），并在 LoRA 足够相似（按 KL 指标）时自动停止。",
      ],
    },
    {
      title: "需要你自己做的事",
      list: [
        "挑选歌曲并检查音质。",
        "检查工作室写出的风格和歌词：模型和识别都会出错。没有自动描述包时需要手写风格。",
        "没有自动描述包时要自己查速度（BPM）；有了它工作室会自己测量。",
        "凭耳朵选择检查点。",
      ],
    },
    {
      title: "训练设置",
      text: [
        "默认是 base-matched 配方，自 2026 年 9 月 27 日起是 HOT-Step 训练器作者的主要配方：按模型技术报告中训练基础模型本身的方式训练 LoRA。没有理由时最好不要改动设置。",
      ],
      list: [
        "配方决定训练量：快速为 50 步、每步 4 首；均衡为 100 步、每步 4 首（默认：训练器作者在整张专辑上听到相似度在第 100 步左右达到最佳）；精细为 200 步、每步 8 首，耗时约四倍。",
        "配方内部：AdamW，学习率 1e-4，短暂预热，到结束时平滑降到 10%；作曲部分以 0.25 的权重、只在模型自己生成的内容上学习；10% 的步骤去掉风格，10% 去掉歌词，10% 两者都去掉；声音部分看到整首歌。不需要歌词时间对齐，因此跳过人声分离和对齐，录音保持原有响度。",
        "LoKr 64 / 因子 4 / alpha 256——轻量适配器（约 106 MB）；也可以改用 LoRA。",
        "“沿用旧法：按 KL 停止”是工作室以前的配方：Prodigy，在 KL = 1.4 时停止（约 1.25 开始相似，约 1.9 模型变差），上限 750 步，每 50 步一个检查点，歌词时间对齐（需要人声分离器），停止方式可改为按轮次。",
        "在旧版本工作室中开始的训练，继续训练时沿用它开始时的配方。",
      ],
    },
    {
      title: '选择检查点与生成',
      list: [
        '凭耳朵选择，而不是看损失曲线：用不同检查点生成同一首歌对比。',
        '在想要的步数下点击“加入 LoRA”——它会以“训练 · 步数”的名字出现在 LoRA 页面。',
        '在“创建”页选择这个 LoRA 时，触发词会自动加入。',
        'LoRA 有两个强度：作曲（AR）和声音（NAR），默认都是 1。训练器作者听感上 NAR 约为 2 时声音最好。',
      ],
    },
    {
      title: "出现问题时",
      list: [
        "歌曲循环、没有结尾、人声崩坏——LoRA 过度训练：换更早的检查点或降低强度。",
        "LoRA 几乎没有变化——换更晚的检查点或精细配方，检查风格和歌词，增加歌曲。",
        "在“沿用旧法”中歌词与音乐错位——检查歌词：录音里没有的多余行和重复会打乱时间对齐。",
        "“沿用旧法”训练因分离器报错无法开始——安装人声分离器或关闭歌词时间对齐。",
      ],
    },
    {
      title: '开始训练前的检查清单',
      checklist: [
        '同一艺人或风格的歌曲 5–20 首，质量良好。',
        '没有重复或残缺片段。',
        '每首歌都有一句英文风格，含 BPM，不含艺人名和歌名。',
        '歌词已对照录音核对，并标注 [Verse] / [Chorus]。',
        '纯音乐已勾选，有人声的歌未勾选。',
        '没有黄色三角。',
        '已设置罕见的触发词。',
        '约 11 GB 显存空闲，已停止生成。',
      ],
    },
  ],
};

const ja: Guide = {
  title: 'LoRA 学習ガイド',
  intro: 'LoRA はモデルへの小さな追加で、あなたの曲の響き（歌い方、ボーカル、アレンジ、プロダクション）を学びます。タイトルバーをドラッグして移動、右下の角でサイズを変えられます。',
  expandAll: 'すべて開く',
  collapseAll: 'すべて閉じる',
  close: '閉じる',
  resize: 'ドラッグでサイズ変更',
  sections: [
    {
      title: "作業の流れ",
      steps: [
        "同じアーティストやスタイルの曲のフォルダーを「学習」ページにドロップするか、ボタンでフォルダーやファイルを選びます。スタジオがフォルダー名でデータセットを作ります。WAV、MP3、FLAC、OGG、M4A、AIFF に対応。.cue 付きの一枚ファイルのアルバムは曲ごとに分割され、横の .txt や .lrc の歌詞はそのまま使われます。",
        "ステップ 1「曲」：準備は自動で始まります。歌詞はまずデータベースから取り、見つからない曲だけボーカルを分離して認識し、各曲を聴いて、測定したテンポ付きでスタイルを書きます。曲ごとに状態が表示され、全体の進行は上に出ます。",
        "各曲はどこまで進んだかを覚えています。準備中にスタジオを閉じたり落ちたりしても、次に起動すると同じところから自動で続き、済んだ作業は繰り返しません。失敗した曲には「再試行」、途中の曲には「この曲を仕上げる」ボタンがあり、その曲だけを仕上げます。",
        "結果を確認：曲をクリックすると、プレーヤー、スタイル、歌詞が開きます。必要に応じて修正し、「もう一度説明」「探し直す」はその曲だけをやり直します。",
        "ステップ 2「学習」：LoRA の名前、トリガーワード（データセット名からスタジオが珍しい単語を作ります。変更も削除もできます）、準備状況の確認と「学習」ボタン。学習ファイル（約 8.6 GB、一度だけ）もここでダウンロードします。NVIDIA RTX 30 シリーズ以降と約 11 GB のビデオメモリが必要です。",
        "待つ必要はありません：準備中に「全曲の準備ができたら自動で学習を開始」にチェックを入れてください。",
        "ステップ 3「結果」：チェックポイントを聴き比べ、一番良いものの下の「LoRA へ」を押すと LoRA ページに表示されます。学習中は生成、アシスタント、カラオケ、ステム分離は使えません。",
      ],
    },
    {
      title: "曲の自動説明",
      text: [
        "任意のパック（約 10.5 GB）で、「曲」ステップの「曲の自動説明」カードから一度だけダウンロードします。なくても歌詞は認識されますが、スタイルは自分で書く必要があります。",
        "各モデルはデータセット全体で一度だけ読み込まれ、その段階が終わると解放されます：歌詞データベース、見つからない曲のボーカル分離と認識、聴き取り、アシスタントの順です。GPU に載るモデルは常に一つです。だから 50 曲のデータセットでも 1 曲の 50 倍はかかりません。",
      ],
      list: [
        "MOSS-Music-8B は曲を聴いて、ジャンル、ボーカル、楽器、雰囲気、プロダクションを説明するモデルです。HOT-Step トレーナーの作者も同じ方法を使っています。音で説明するほうがタイトルからよりずっと正確です。",
        "Beat This! は録音の拍を見つけるネットワークで、スタジオはそこからテンポを計算します。Monetochka の曲では Tunebat や SongBPM と 1 BPM 以内で一致しました。",
        "スタジオのアシスタントがこれを YuE2 形式のスタイル一行にまとめ、末尾に測定したテンポを入れます。MOSS 自身のテンポは誤りやすいので、常に測定値に置き換えます。キーは YuE2 のスタイルには書きません。モデルが楽譜を書くときに自分で決めます。",
        "MOSS は曲の最大 7 分を聴き、長く聴くほどビデオメモリを使います。1 分で約 11 GB、7 分で約 18 GB です。小さめのカードでは曲の中央を収まる分だけ聴き、12 GB 未満のカードでは小さい Q4_K_M 版を、8 GB 未満または CUDA 13 非対応のカードではプロセッサーで動き、1 曲に数分かかります。RTX 4090 なら 1 曲 6〜7 秒です。すべてあなたのコンピューターで動き、どこにも送信されません。",
        "結果は必ず確認してください。モデルがジャンルや楽器を間違えることがあります。手で直してください。",
      ],
    },
    {
      title: 'どの曲を使うか',
      list: [
        '一人のアーティストか、狭い一つのスタイル。寄せ集めでは何も具体的に学びません。',
        '通常 5〜20 曲。学習器の作者は 10 曲と 20 曲で確かな差を見ていません。歌詞の多いアルバムや作風がばらばらなアルバムは学びにくいです。',
        '録音品質をそろえる：スタジオ版で、ライブの雑音、ラジオのジングル、途切れた断片は避けます。',
        '曲はまるごと：学習器が自分で 10 秒ずつに切ります。',
        '同じ曲を二度入れない（オリジナルとリマスターなど）。その曲に偏ります。',
      ],
    },
    {
      title: 'スタイル — 何を書くか',
      text: [
        '英語で一文、約 35〜70 語。順番は 言語 → ジャンルと年代 → ボーカル → 楽器 → ムード → プロダクション → テンポ「N BPM」。',
        'アーティスト名、曲名、キー、拍子は書かず、歌詞も引用しません。インストは言語の代わりに「instrumental」と書きます。',
        'BPM は推測せず、解析ツールや曲データベースから取ります。間違ったテンポは間違いを学ばせます。',
        'トリガーワードはスタイルに書かないでください。スタジオが自動で加えます。',
      ],
      examples: [
        {
          label: '例',
          body: 'Russian-language 2010s indie pop with bright synth pop touches, young female lead vocal with a playful ironic delivery and light doubled harmonies, analog synths, drum machine and clean electric guitar, carefree yet slightly melancholic mood, crisp modern bedroom-pop production with a warm low end, 120 BPM',
        },
      ],
    },
    {
      title: "歌詞",
      list: [
        "実際に歌われている言葉だけを、コード、リンク、注釈なしで。サイトの歌詞は必ず録音と照合してください。",
        "パートを示します：[Verse 1]、[Chorus]、[Verse 2]、[Bridge]、[Outro]。各パートは改行して始めます。",
        "音声と同じ名前の .txt や .lrc は追加時に読み込まれ、.lrc のタイムスタンプは取り除かれます。",
        "歌詞がないときは、スタジオがアーティスト、タイトル、長さで公開歌詞データベース（LRCLIB、QQ Music、Kugou）を検索します（アーティストとタイトルはファイルのタグ、または名前とフォルダーから）。どのデータベースにもない曲だけ、ボーカルを分離して Whisper で認識します。精度はかなり下がります。歌詞の上に出どころが表示されます。結果は必ず確認してください。",
        "認識器が歌詞を聞き取れなかった曲はインストゥルメンタルになります。誤りなら「インストゥルメンタル」を外して「探し直す」を押してください。",
      ],
    },
    {
      title: "スタジオが自動でやること",
      list: [
        "すべてを 48 kHz の WAV に変換し、10 秒ずつに切り分けます。",
        "各曲の楽譜を作ります（SheetSage2）。",
        "トリガーワードを学習に入れ、生成でこの LoRA を選ぶとスタイルにも入れます。",
        "10 ステップごとと学習の終わりにチェックポイントを保存します。",
        "「従来どおり」レシピでは、歌詞のある曲のボーカルを分離し、ボーカルに合わせて単語を時間に結びつけ（どの単語がどこで歌われるかをモデルが学びます）、LoRA が十分に似たら（KL 指標で）自動で止めます。",
      ],
    },
    {
      title: "自分でやること",
      list: [
        "曲を選び、音質を確認する。",
        "スタジオが書いたスタイルと歌詞を確認する。モデルも認識も間違えます。自動説明パックがなければスタイルは手で書きます。",
        "自動説明パックがなければテンポ（BPM）を調べる。あればスタジオが測ります。",
        "チェックポイントを耳で選ぶ。",
      ],
    },
    {
      title: "学習の設定",
      text: [
        "既定は base-matched レシピで、2026 年 9 月 27 日から HOT-Step トレーナー作者のメインのレシピです。モデルの技術レポートにある基本モデル自体の学習方法で LoRA を学習します。理由がなければ設定は変えないでください。",
      ],
      list: [
        "レシピで量が決まります。高速は 4 曲 × 50 ステップ、バランスは 4 曲 × 100 ステップ（既定。トレーナー作者はアルバム全体で 100 ステップ前後で似かたが最良になると聴いています）、入念は 8 曲 × 200 ステップで約 4 倍の時間がかかります。",
        "レシピの中身: AdamW、学習率 1e-4、短いウォームアップのあと終わりまでに 10% まで緩やかに下げます。作曲側は重み 0.25 で、モデル自身が生成する部分だけで学習します。ステップの 10% はスタイルなし、10% は歌詞なし、10% は両方なし。サウンド側は曲全体を見ます。歌詞タイミングは不要なので、ボーカル分離と整列は省かれ、録音の音量もそのままです。",
        "LoKr 64 / 係数 4 / alpha 256 — 軽いアダプター（約 106 MB）。LoRA も選べます。",
        "「従来どおり: KL で停止」はスタジオの以前のレシピです。Prodigy、KL = 1.4 で停止（1.25 前後から似始め、1.9 前後でモデルが崩れます）、上限 750 ステップ、50 ステップごとのチェックポイント、歌詞タイミング（ボーカル分離が必要）。停止はエポック単位にも切り替えられます。",
        "以前のバージョンのスタジオで始めた学習は、始めたときのレシピで続けて学習します。",
      ],
    },
    {
      title: 'チェックポイントの選び方と生成',
      list: [
        '損失グラフではなく耳で選びます。同じ曲を別々のチェックポイントで生成して比べます。',
        '選んだステップの下で「LoRA に追加」を押すと、「実行 · ステップ」という名前で LoRA ページに出ます。',
        '「作成」ページでこの LoRA を選ぶと、トリガーは自動で加わります。',
        'LoRA の強さは作曲（AR）と音（NAR）の二つで、既定は 1 と 1。学習器の作者は NAR を 2 前後にしたとき一番良い音を聴いています。',
      ],
    },
    {
      title: "うまくいかないとき",
      list: [
        "曲がループする、終わりがない、ボーカルが崩れる — LoRA が学習しすぎです。前のチェックポイントを使うか強さを下げてください。",
        "LoRA がほとんど何も変えない — 後のチェックポイントか入念レシピを使い、スタイルと歌詞を確認し、曲を増やしてください。",
        "「従来どおり」で単語が音楽からずれる — 歌詞を確認してください。録音にない余分な行や繰り返しがタイミングを狂わせます。",
        "「従来どおり」の学習が分離器のエラーで始まらない — ボーカル分離器をインストールするか、歌詞タイミングをオフにしてください。",
      ],
    },
    {
      title: '開始前のチェックリスト',
      checklist: [
        '同じアーティストかスタイルの曲が 5〜20 曲、品質良好。',
        '重複や断片がない。',
        '各曲に英語一文のスタイル（BPM 付き、アーティスト名と曲名なし）。',
        '歌詞を録音と照合し、[Verse] / [Chorus] を付けた。',
        'インストにはチェック、ボーカル曲はチェックなし。',
        '黄色い三角がない。',
        '珍しいトリガーワードを設定した。',
        'VRAM が約 11 GB 空いていて、生成は止めてある。',
      ],
    },
  ],
};

const ko: Guide = {
  title: 'LoRA 학습 안내',
  intro: 'LoRA는 모델에 붙는 작은 추가 파일로, 당신의 곡처럼 들리는 법(창법, 보컬, 편곡, 프로덕션)을 배웁니다. 제목 표시줄을 끌어 옮기고, 오른쪽 아래 모서리로 크기를 바꿀 수 있습니다.',
  expandAll: '모두 펼치기',
  collapseAll: '모두 접기',
  close: '닫기',
  resize: '끌어서 크기 조절',
  sections: [
    {
      title: "작업 순서",
      steps: [
        "한 아티스트나 한 스타일의 노래 폴더를 \"학습\" 페이지에 끌어다 놓거나 버튼으로 폴더와 파일을 고르세요. 스튜디오가 폴더 이름으로 데이터셋을 만듭니다. WAV, MP3, FLAC, OGG, M4A, AIFF를 지원하고, .cue가 있는 한 파일짜리 앨범은 곡별로 나뉘며, 옆의 .txt나 .lrc 가사는 그대로 사용합니다.",
        "1단계 \"곡\": 준비가 저절로 시작됩니다. 가사는 먼저 데이터베이스에서 가져오고, 없는 곡만 보컬을 분리해 인식하며, 각 곡을 듣고, 측정한 템포로 스타일을 씁니다. 곡마다 상태가 보이고, 전체 진행은 위에 있습니다.",
        "곡마다 어디까지 했는지 기억합니다. 준비 중에 스튜디오를 닫거나 멈추면 다음 실행 때 같은 곳에서 저절로 이어지고, 끝난 작업은 다시 하지 않습니다. 실패한 곡에는 '다시 시도', 덜 된 곡에는 '이 곡 마무리' 버튼이 있어 그 곡만 마무리합니다.",
        "결과를 확인하세요: 곡을 누르면 플레이어, 스타일, 가사가 펼쳐집니다. 필요한 것을 고치세요. \"다시 설명\"과 \"다시 찾기\"은 그 곡만 다시 합니다.",
        "2단계 \"학습\": LoRA 이름, 트리거 단어(데이터셋 이름으로 스튜디오가 드문 단어를 만들어 줍니다. 바꾸거나 지울 수 있습니다), 준비 확인과 \"학습\" 버튼. 학습 파일(약 8.6GB, 한 번)도 여기서 내려받습니다. NVIDIA RTX 30 시리즈 이상과 약 11GB의 비디오 메모리가 필요합니다.",
        "기다릴 필요 없습니다: 준비 중에 \"모든 곡이 준비되면 자동으로 학습 시작\"을 체크하세요.",
        "3단계 \"결과\": 체크포인트를 들어 보고 가장 좋은 것 아래의 \"LoRA로\"를 누르면 LoRA 페이지에 나타납니다. 학습 중에는 생성, 어시스턴트, 가라오케, 스템 분리를 쓸 수 없습니다.",
      ],
    },
    {
      title: "곡 자동 설명",
      text: [
        "선택 패키지(약 10.5GB)로, \"곡\" 단계의 \"곡 자동 설명\" 카드에서 한 번 내려받습니다. 없어도 가사는 인식되지만 스타일은 직접 써야 합니다.",
        "각 모델은 데이터셋 전체에 한 번만 로드되고, 그 단계가 끝나면 해제됩니다: 가사 데이터베이스, 없는 곡의 보컬 분리와 인식, 듣기, 어시스턴트 순서입니다. 그래픽 카드에는 항상 모델 하나만 올라갑니다. 그래서 50곡짜리 데이터셋도 한 곡의 50배가 걸리지 않습니다.",
      ],
      list: [
        "MOSS-Music-8B는 곡을 듣고 장르, 보컬, 악기, 분위기, 프로덕션을 설명하는 모델입니다. HOT-Step 트레이너 작성자도 같은 방법을 씁니다. 소리로 쓴 설명이 제목으로 쓴 것보다 훨씬 정확합니다.",
        "Beat This!는 녹음에서 박을 찾는 네트워크이고, 스튜디오는 그것으로 템포를 계산합니다. Monetochka의 곡에서 Tunebat, SongBPM과 1 BPM 이내로 일치했습니다.",
        "스튜디오의 어시스턴트가 이것을 YuE2 형식의 스타일 한 줄로 정리하고 끝에 측정한 템포를 넣습니다. MOSS 자체의 템포는 자주 틀리므로 항상 측정값으로 바꿉니다. 키는 YuE2 스타일에 쓰지 않습니다. 모델이 악보를 쓸 때 스스로 정합니다.",
        "MOSS는 곡을 최대 7분까지 듣고, 오래 들을수록 비디오 메모리를 더 씁니다. 1분에 약 11GB, 7분에 약 18GB입니다. 작은 카드에서는 곡의 가운데를 들어갈 만큼만 듣고, 12GB 미만 카드에서는 더 작은 Q4_K_M 빌드를, 8GB 미만이거나 CUDA 13을 지원하지 않는 카드에서는 프로세서에서 돌아가며 한 곡에 몇 분이 걸립니다. RTX 4090에서는 한 곡에 6–7초가 걸립니다. 모든 것이 여러분의 컴퓨터에서 돌아가며 어디에도 보내지 않습니다.",
        "결과는 항상 확인하세요. 모델이 장르나 악기를 틀릴 수 있으니 직접 고치세요.",
      ],
    },
    {
      title: '어떤 곡을 쓸까',
      list: [
        '한 아티스트 또는 좁은 한 가지 스타일. 뒤섞인 모음은 아무것도 구체적으로 배우지 못합니다.',
        '보통 5~20곡. 학습기 제작자는 10곡과 20곡 사이에 뚜렷한 차이를 보지 못했습니다. 가사가 빽빽하거나 스타일이 섞인 앨범은 배우기 더 어렵습니다.',
        '녹음 품질을 고르게: 스튜디오 버전, 라이브 잡음·라디오 징글·잘린 조각은 제외합니다.',
        '곡은 통째로: 학습기가 알아서 10초씩 자릅니다.',
        '같은 곡을 두 번 넣지 마세요(원곡과 리마스터 등). 그 곡 쪽으로 치우칩니다.',
      ],
    },
    {
      title: '스타일 — 무엇을 쓸까',
      text: [
        '영어 한 문장, 약 35~70 단어, 순서: 언어 → 장르와 시대 → 보컬 → 악기 → 분위기 → 프로덕션 → 템포 「N BPM」.',
        '아티스트 이름, 곡 제목, 조성, 박자는 쓰지 말고 가사도 인용하지 않습니다. 연주곡은 언어 자리에 「instrumental」이라고 씁니다.',
        'BPM은 짐작하지 말고 분석 도구나 곡 데이터베이스에서 가져옵니다. 틀린 템포는 틀린 것을 가르칩니다.',
        '트리거 단어는 스타일에 쓰지 마세요. 스튜디오가 알아서 넣습니다.',
      ],
      examples: [
        {
          label: '예시',
          body: 'Russian-language 2010s indie pop with bright synth pop touches, young female lead vocal with a playful ironic delivery and light doubled harmonies, analog synths, drum machine and clean electric guitar, carefree yet slightly melancholic mood, crisp modern bedroom-pop production with a warm low end, 120 BPM',
        },
      ],
    },
    {
      title: "가사",
      list: [
        "실제로 부르는 가사만, 코드나 링크, 메모 없이. 사이트의 가사는 반드시 녹음과 대조하세요.",
        "파트를 표시하세요: [Verse 1], [Chorus], [Verse 2], [Bridge], [Outro]. 각 파트는 새 줄에서 시작합니다.",
        "오디오와 이름이 같은 .txt나 .lrc는 추가할 때 읽히고, .lrc의 타임스탬프는 지워집니다.",
        "가사가 없으면 스튜디오가 아티스트, 제목, 길이로 공개 가사 데이터베이스(LRCLIB, QQ Music, Kugou)에서 찾습니다(아티스트와 제목은 파일 태그나 이름과 폴더에서 가져옵니다). 어느 데이터베이스에도 없는 곡만 보컬을 분리해 Whisper로 인식하며, 정확도는 훨씬 낮습니다. 가사 위에 출처가 표시됩니다. 결과는 항상 확인하세요.",
        "인식기가 가사를 듣지 못한 곡은 연주곡으로 표시됩니다. 틀렸다면 \"연주곡\"을 끄고 \"다시 찾기\"을 누르세요.",
      ],
    },
    {
      title: "스튜디오가 알아서 하는 일",
      list: [
        "모든 것을 48 kHz WAV로 바꾸고 10초 조각으로 자릅니다.",
        "곡마다 악보를 만듭니다(SheetSage2).",
        "트리거 단어를 학습에 넣고, 생성할 때 이 LoRA를 고르면 스타일에도 넣습니다.",
        "10스텝마다, 그리고 학습이 끝날 때 체크포인트를 저장합니다.",
        "\"이전 방식\" 레시피에서는 가사가 있는 곡의 보컬을 분리하고, 보컬에 맞춰 단어를 시간에 연결하며(어떤 단어가 어디서 불리는지 모델이 배웁니다), LoRA가 충분히 비슷해지면(KL 지표로) 스스로 학습을 멈춥니다.",
      ],
    },
    {
      title: "직접 해야 하는 일",
      list: [
        "곡을 고르고 음질을 확인하기.",
        "스튜디오가 쓴 스타일과 가사를 확인하기. 모델도 인식도 틀립니다. 자동 설명 패키지가 없으면 스타일은 직접 씁니다.",
        "자동 설명 패키지가 없으면 템포(BPM)를 찾기. 있으면 스튜디오가 직접 잽니다.",
        "체크포인트를 귀로 고르기.",
      ],
    },
    {
      title: "학습 설정",
      text: [
        "기본값은 base-matched 레시피로, 2026년 9월 27일부터 HOT-Step 트레이너 제작자의 주 레시피입니다. 모델 기술 보고서에 나온 기본 모델 학습 방식 그대로 LoRA를 학습합니다. 이유가 없다면 설정은 건드리지 않는 편이 좋습니다.",
      ],
      list: [
        "레시피가 학습량을 정합니다. 빠름은 4곡 × 50스텝, 균형은 4곡 × 100스텝(기본값: 트레이너 제작자는 앨범 전체에서 100스텝 무렵 닮음이 최고라고 들었습니다), 꼼꼼은 8곡 × 200스텝으로 약 4배 오래 걸립니다.",
        "레시피 내용: AdamW, 학습률 1e-4, 짧은 워밍업 후 끝까지 10%로 부드럽게 낮춥니다. 작곡 부분은 가중치 0.25로, 모델이 스스로 생성하는 부분에서만 배웁니다. 스텝의 10%는 스타일 없이, 10%는 가사 없이, 10%는 둘 다 없이 진행하고, 사운드 부분은 곡 전체를 봅니다. 가사 타이밍이 필요 없어 보컬 분리와 정렬을 건너뛰며, 녹음의 음량은 그대로 둡니다.",
        "LoKr 64 / 팩터 4 / alpha 256 — 가벼운 어댑터(약 106 MB). LoRA로 바꿀 수도 있습니다.",
        "\"이전 방식: KL로 중지\"는 스튜디오의 예전 레시피입니다. Prodigy, KL = 1.4에서 중지(1.25 무렵부터 닮기 시작하고 1.9 무렵 모델이 망가집니다), 750스텝 상한, 50스텝마다 체크포인트, 가사 타이밍(보컬 분리기 필요). 중지 방식은 에포크 단위로 바꿀 수 있습니다.",
        "이전 버전 스튜디오에서 시작한 학습은 시작할 때의 레시피로 이어서 학습합니다.",
      ],
    },
    {
      title: '체크포인트 고르기와 생성',
      list: [
        '손실 그래프가 아니라 귀로 고릅니다. 같은 곡을 여러 체크포인트로 생성해 비교하세요.',
        '원하는 스텝 아래의 「LoRA에 추가」를 누르면 「실행 · 스텝」 이름으로 LoRA 페이지에 나타납니다.',
        '「만들기」 페이지에서 이 LoRA를 고르면 트리거가 알아서 들어갑니다.',
        'LoRA 강도는 작곡(AR)과 소리(NAR) 두 가지이고 기본은 1과 1입니다. 학습기 제작자는 NAR 약 2에서 가장 좋은 소리를 들었습니다.',
      ],
    },
    {
      title: "문제가 있을 때",
      list: [
        "곡이 반복되거나 끝이 없고 보컬이 무너진다 — LoRA가 과학습되었습니다. 더 이른 체크포인트를 쓰거나 강도를 낮추세요.",
        "LoRA가 거의 아무것도 바꾸지 않는다 — 더 늦은 체크포인트나 꼼꼼 레시피를 쓰고, 스타일과 가사를 확인하고, 곡을 더하세요.",
        "\"이전 방식\"에서 단어가 음악과 어긋난다 — 가사를 확인하세요. 녹음에 없는 줄과 반복이 타이밍을 흐트러뜨립니다.",
        "\"이전 방식\" 학습이 분리기 오류로 시작되지 않는다 — 보컬 분리기를 설치하거나 가사 타이밍을 끄세요.",
      ],
    },
    {
      title: '시작 전 체크리스트',
      checklist: [
        '같은 아티스트나 스타일의 곡 5~20곡, 좋은 품질.',
        '중복이나 잘린 조각이 없음.',
        '모든 곡에 BPM이 들어간 영어 한 문장 스타일, 아티스트 이름과 제목 없음.',
        '가사를 녹음과 대조하고 [Verse] / [Chorus]를 표시함.',
        '연주곡은 체크, 보컬 곡은 체크 해제.',
        '노란 삼각형 없음.',
        '드문 트리거 단어를 정함.',
        'VRAM 약 11 GB 여유, 생성은 멈춤.',
      ],
    },
  ],
};

export const trainingGuide: Record<Language, Guide> = { en, ru, zh, ja, ko, pt: ptTrainingGuide };
