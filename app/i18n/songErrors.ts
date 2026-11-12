import type { Language } from './translations';

// Why a song failed, in the window's language. The service sends a code and fields beside
// its English text (crates/music-server/src/song_errors.rs); here is a template for each
// code. The service's test `every_code_has_a_translation_in_every_language` checks that each
// code has a line in all five languages. The codes are the same in every music studio; YuE2 adds
// its own engine's (adapter_missing … score_unsingable_adapters).

type ErrorTexts = Record<string, string>;

const EN: ErrorTexts = {
  training_on_card: 'A LoRA is training on the graphics card; make songs once it finishes or is stopped.',
  lost_vram: 'The graphics card ran out of memory on this song and the engine restarted, so the song was lost. Choose a smaller model set in Settings → Models or a shorter song, and close whatever else uses the card; a card below the smallest set cannot make songs.',
  lost_vram_retried: 'The graphics card ran out of memory on this song, and again when it was made with a {n} KV cache. Choose a smaller model set or a shorter song, and close whatever else uses the card.',
  lost_ram: 'The computer ran out of memory (RAM) on this song and the engine restarted, so the song was lost. Close other programs that take memory, or choose a smaller model set in Settings → Models or a shorter song.',
  lost_device: 'The graphics card failed during this song ({detail}) and the engine restarted, so the song was lost. The engine log is in Settings → Local engine.',
  lost_device_silent: 'The graphics card failed during this song (a CUDA or driver error) and the engine restarted, so the song was lost. The engine log is in Settings → Local engine.',
  lost_at_line: 'The engine stopped during this song and restarted, so the song was lost; its last line was {line}. The engine log is in Settings → Local engine.',
  lost: 'The engine stopped during this song and restarted, so the song was lost. The engine log is in Settings → Local engine.',
  lost_exited: 'The engine crashed during this song (exit code {exit}) and restarted, so the song was lost. The engine log is in Settings → Local engine.',
  lost_port_set: 'The engine could not listen on port {port}, which YUE_ENGINE_PORT sets, so the song was lost: another program holds it. Close that program or set another port.',
  lost_port_taken: 'The engine could not listen on port {port}, so the song was lost: another program took the port after the studio chose it. Restart the studio: it chooses a free port at start.',
  lost_illegal_instruction: 'The engine stopped during this song on an instruction this processor does not have (exit code {exit}) and restarted, so the song was lost. Please report it with the engine log from Settings → Local engine.',
  load_vram: 'The graphics card ran out of memory while the engine was loading the models. Choose a smaller quantisation in Settings → Models, or close whatever else is using the card — the writing assistant holds several gigabytes of its own.',
  load_ram: 'The computer ran out of memory (RAM) while the engine was loading the models. Close other programs that take memory, or choose a smaller quantisation in Settings → Models.',
  engine_fatal: 'The engine stopped this song: {line}',
  engine_failed: 'The engine stopped this song; its log in Settings → Local engine has the reason.',
  engine_unreachable: 'The engine stopped answering about this song: {detail}',
  import_failed: 'The engine finished the song, but the studio could not take it into the library: {detail}',
  adapter_missing: 'The LoRA {name} is not installed; add it again on the LoRA page.',
  vocals_only_no_separator: 'Vocals only needs the stem separator: install it in Studio tools.',
  separator_unavailable: 'The stem separator cannot run on this computer: {detail}',
  engine_refused: 'The engine did not take this song: {detail}',
  context_short: 'The style, lyrics and score take {tokens} of the model\'s {context} tokens, so this song can last {can_last} s, and {asked} s were asked for. Make the song or its lyrics shorter.',
  score_unsingable: 'The score the model wrote for song {song} cannot be sung ({problem}). Draw another seed.',
  score_unsingable_adapters: 'The score the model wrote for song {song} cannot be sung ({problem}). LoRAs past their limits on the score half do this: lower their strengths or draw another seed.',
};

const RU: ErrorTexts = {
  training_on_card: 'На видеокарте идёт обучение LoRA: песни можно делать, когда оно закончится или будет остановлено.',
  lost_vram: 'На этой песне у видеокарты кончилась память, движок перезапустился, и песня потеряна. Выберите набор моделей поменьше в Настройках → Модели или песню короче и закройте всё, что ещё занимает видеокарту; на карте слабее самого маленького набора песни не делаются.',
  lost_vram_retried: 'Видеокарте не хватило памяти на этой песне и при повторе с KV-кэшем {n}. Выберите набор моделей меньше или песню короче и закройте то, что ещё занимает карту.',
  lost_ram: 'На этой песне у компьютера кончилась оперативная память (RAM), движок перезапустился, и песня потеряна. Закройте программы, которые занимают память, или выберите набор моделей поменьше в Настройках → Модели или песню короче.',
  lost_device: 'Во время этой песни видеокарта дала сбой ({detail}), движок перезапустился, и песня потеряна. Лог движка — в Настройках → Локальный движок.',
  lost_device_silent: 'Во время этой песни видеокарта дала сбой (ошибка CUDA или драйвера), движок перезапустился, и песня потеряна. Лог движка — в Настройках → Локальный движок.',
  lost_at_line: 'Движок остановился во время этой песни и перезапустился, песня потеряна; его последняя строка: {line}. Лог движка — в Настройках → Локальный движок.',
  lost: 'Движок остановился во время этой песни и перезапустился, песня потеряна. Лог движка — в Настройках → Локальный движок.',
  lost_exited: 'Движок упал во время этой песни (код выхода {exit}) и перезапустился, песня потеряна. Лог движка — в Настройках → Локальный движок.',
  lost_port_set: 'Движок не смог слушать порт {port}, заданный в YUE_ENGINE_PORT, песня потеряна: порт занят другой программой. Закройте её или задайте другой порт.',
  lost_port_taken: 'Движок не смог слушать порт {port}, песня потеряна: порт заняла другая программа после того, как студия его выбрала. Перезапустите студию — при старте она выберет свободный порт.',
  lost_illegal_instruction: 'Движок остановился во время этой песни на инструкции, которой нет у этого процессора (код выхода {exit}), и перезапустился, песня потеряна. Сообщите об этом и приложите лог движка из Настроек → Локальный движок.',
  load_vram: 'У видеокарты кончилась память, пока движок загружал модели. Выберите квантование поменьше в Настройках → Модели или закройте всё, что ещё занимает видеокарту, — помощник по текстам сам держит несколько гигабайт.',
  load_ram: 'У компьютера кончилась оперативная память (RAM), пока движок загружал модели. Закройте программы, которые занимают память, или выберите квантование поменьше в Настройках → Модели.',
  engine_fatal: 'Движок остановил эту песню: {line}',
  engine_failed: 'Движок остановил эту песню; причина — в его логе в Настройках → Локальный движок.',
  engine_unreachable: 'Движок перестал отвечать про эту песню: {detail}',
  import_failed: 'Движок доделал песню, но студия не смогла забрать её в медиатеку: {detail}',
  adapter_missing: 'LoRA {name} не установлена: добавьте её заново на странице LoRA.',
  vocals_only_no_separator: 'Для «Только вокал» нужна модель разделения: установите её в Инструментах.',
  separator_unavailable: 'Модель разделения не может работать на этом компьютере: {detail}',
  engine_refused: 'Движок не принял эту песню: {detail}',
  context_short: 'Стиль, текст и партитура занимают {tokens} из {context} токенов модели, поэтому песня может длиться {can_last} с, а запрошено {asked} с. Сократите песню или её текст.',
  score_unsingable: 'Партитуру, которую модель написала для песни {song}, нельзя спеть ({problem}). Попробуйте другое зерно.',
  score_unsingable_adapters: 'Партитуру, которую модель написала для песни {song}, нельзя спеть ({problem}). Так бывает, когда LoRA выкручены сверх своих пределов на партитуре: уменьшите их силу или попробуйте другое зерно.',
};

const ZH: ErrorTexts = {
  training_on_card: '显卡正在训练 LoRA，训练结束或停止后再生成歌曲。',
  lost_vram: '生成这首歌时显卡显存耗尽，引擎已重启，这首歌丢失了。请在“设置 → 模型”中选择更小的模型组合或更短的歌曲，并关闭其他占用显卡的程序；显卡低于最小组合时无法生成歌曲。',
  lost_vram_retried: '生成这首歌时显卡显存耗尽，用 {n} 的 KV 缓存重做时又一次耗尽。请选择更小的模型组合或更短的歌曲，并关闭其他占用显卡的程序。',
  lost_ram: '生成这首歌时电脑内存（RAM）耗尽，引擎已重启，这首歌丢失了。请关闭占用内存的程序，或在“设置 → 模型”中选择更小的模型组合或更短的歌曲。',
  lost_device: '生成这首歌时显卡出错（{detail}），引擎已重启，这首歌丢失了。引擎日志在“设置 → 本地引擎”。',
  lost_device_silent: '生成这首歌时显卡出错（CUDA 或驱动错误），引擎已重启，这首歌丢失了。引擎日志在“设置 → 本地引擎”。',
  lost_at_line: '生成这首歌时引擎停止并已重启，这首歌丢失了；它的最后一行是 {line}。引擎日志在“设置 → 本地引擎”。',
  lost: '生成这首歌时引擎停止并已重启，这首歌丢失了。引擎日志在“设置 → 本地引擎”。',
  lost_exited: '生成这首歌时引擎崩溃（退出码 {exit}）并已重启，这首歌丢失了。引擎日志在“设置 → 本地引擎”。',
  lost_port_set: '引擎无法监听 YUE_ENGINE_PORT 指定的端口 {port}，这首歌丢失了：该端口被其他程序占用。请关闭该程序或设置其他端口。',
  lost_port_taken: '引擎无法监听端口 {port}，这首歌丢失了：工作室选定该端口后被其他程序占用。请重启工作室，它会在启动时选择空闲端口。',
  lost_illegal_instruction: '生成这首歌时引擎遇到此处理器不支持的指令而停止（退出码 {exit}）并已重启，这首歌丢失了。请附上“设置 → 本地引擎”中的引擎日志反馈此问题。',
  load_vram: '引擎加载模型时显卡显存耗尽。请在“设置 → 模型”中选择更小的量化，或关闭其他占用显卡的程序——写作助手本身就占用数 GB。',
  load_ram: '引擎加载模型时电脑内存（RAM）耗尽。请关闭占用内存的程序，或在“设置 → 模型”中选择更小的量化。',
  engine_fatal: '引擎停止了这首歌：{line}',
  engine_failed: '引擎停止了这首歌；原因见“设置 → 本地引擎”中的日志。',
  engine_unreachable: '引擎不再回应这首歌的状态：{detail}',
  import_failed: '引擎已完成这首歌，但工作室未能将其放入媒体库：{detail}',
  adapter_missing: 'LoRA {name} 未安装：请在 LoRA 页面重新添加。',
  vocals_only_no_separator: '“仅人声”需要分轨模型：请在“工作室工具”中安装。',
  separator_unavailable: '分轨模型无法在这台电脑上运行：{detail}',
  engine_refused: '引擎没有接受这首歌：{detail}',
  context_short: '风格、歌词和乐谱占用了模型 {context} 个 token 中的 {tokens} 个，因此这首歌最长 {can_last} 秒，而请求的是 {asked} 秒。请缩短歌曲或歌词。',
  score_unsingable: '模型为第 {song} 首歌写的乐谱无法演唱（{problem}）。请换一个种子。',
  score_unsingable_adapters: '模型为第 {song} 首歌写的乐谱无法演唱（{problem}）。LoRA 在乐谱部分超出其限度时会这样：请降低它们的强度或换一个种子。',
};

const JA: ErrorTexts = {
  training_on_card: 'グラフィックカードで LoRA の学習中です。終わるか止めてから曲を作ってください。',
  lost_vram: 'この曲でグラフィックカードのメモリが足りなくなり、エンジンが再起動したため曲は失われました。「設定 → モデル」で小さいモデルセットか短い曲を選び、カードを使っている他のものを閉じてください。最小のセットに満たないカードでは曲を作れません。',
  lost_vram_retried: 'この曲でグラフィックカードのメモリが足りなくなり、KV キャッシュ {n} で作り直したときも足りませんでした。小さいモデルセットか短い曲を選び、カードを使っている他のものを閉じてください。',
  lost_ram: 'この曲でパソコンのメモリ（RAM）が足りなくなり、エンジンが再起動したため曲は失われました。メモリを使う他のプログラムを閉じるか、「設定 → モデル」で小さいモデルセットか短い曲を選んでください。',
  lost_device: 'この曲の途中でグラフィックカードが失敗し（{detail}）、エンジンが再起動したため曲は失われました。エンジンのログは「設定 → ローカルエンジン」にあります。',
  lost_device_silent: 'この曲の途中でグラフィックカードが失敗し（CUDA またはドライバーのエラー）、エンジンが再起動したため曲は失われました。エンジンのログは「設定 → ローカルエンジン」にあります。',
  lost_at_line: 'この曲の途中でエンジンが止まって再起動し、曲は失われました。最後の行：{line}。エンジンのログは「設定 → ローカルエンジン」にあります。',
  lost: 'この曲の途中でエンジンが止まって再起動し、曲は失われました。エンジンのログは「設定 → ローカルエンジン」にあります。',
  lost_exited: 'この曲の途中でエンジンがクラッシュし（終了コード {exit}）、再起動したため曲は失われました。エンジンのログは「設定 → ローカルエンジン」にあります。',
  lost_port_set: 'エンジンが YUE_ENGINE_PORT で指定したポート {port} で待ち受けできず、曲は失われました。別のプログラムがそのポートを使っています。そのプログラムを閉じるか、別のポートを指定してください。',
  lost_port_taken: 'エンジンがポート {port} で待ち受けできず、曲は失われました。スタジオが選んだ後に別のプログラムがそのポートを使い始めました。スタジオを再起動してください。起動時に空いているポートを選びます。',
  lost_illegal_instruction: 'この曲の途中でエンジンがこのプロセッサーにない命令で止まり（終了コード {exit}）、再起動したため曲は失われました。「設定 → ローカルエンジン」のエンジンログを添えて報告してください。',
  load_vram: 'エンジンがモデルを読み込む間にグラフィックカードのメモリが足りなくなりました。「設定 → モデル」で小さい量子化を選ぶか、カードを使っている他のものを閉じてください。作詞アシスタントだけでも数 GB を使います。',
  load_ram: 'エンジンがモデルを読み込む間にパソコンのメモリ（RAM）が足りなくなりました。メモリを使う他のプログラムを閉じるか、「設定 → モデル」で小さい量子化を選んでください。',
  engine_fatal: 'エンジンがこの曲を止めました：{line}',
  engine_failed: 'エンジンがこの曲を止めました。理由は「設定 → ローカルエンジン」のログにあります。',
  engine_unreachable: 'エンジンがこの曲について応答しなくなりました：{detail}',
  import_failed: 'エンジンは曲を仕上げましたが、スタジオがライブラリに取り込めませんでした：{detail}',
  adapter_missing: 'LoRA {name} はインストールされていません。LoRA ページでもう一度追加してください。',
  vocals_only_no_separator: '「ボーカルのみ」には分離モデルが必要です。「スタジオツール」でインストールしてください。',
  separator_unavailable: '分離モデルはこのパソコンでは動きません：{detail}',
  engine_refused: 'エンジンがこの曲を受け付けませんでした：{detail}',
  context_short: 'スタイル、歌詞、楽譜でモデルの {context} トークンのうち {tokens} を使うため、この曲は {can_last} 秒までですが、{asked} 秒が指定されました。曲か歌詞を短くしてください。',
  score_unsingable: 'モデルが曲 {song} のために書いた楽譜は歌えません（{problem}）。別のシードを試してください。',
  score_unsingable_adapters: 'モデルが曲 {song} のために書いた楽譜は歌えません（{problem}）。楽譜側で LoRA を限度を超えて強くするとこうなります。強さを下げるか、別のシードを試してください。',
};

const KO: ErrorTexts = {
  training_on_card: '그래픽카드에서 LoRA 학습 중입니다. 학습이 끝나거나 멈춘 뒤 곡을 만드세요.',
  lost_vram: '이 곡에서 그래픽카드 메모리가 부족해 엔진이 다시 시작되었고 곡을 잃었습니다. "설정 → 모델"에서 더 작은 모델 세트나 더 짧은 곡을 고르고, 카드를 쓰는 다른 것을 닫으세요. 가장 작은 세트보다 약한 카드로는 곡을 만들 수 없습니다.',
  lost_vram_retried: '이 곡에서 그래픽카드 메모리가 부족했고, KV 캐시 {n}으로 다시 만들 때도 부족했습니다. 더 작은 모델 세트나 더 짧은 곡을 고르고, 카드를 쓰는 다른 것을 닫으세요.',
  lost_ram: '이 곡에서 컴퓨터 메모리(RAM)가 부족해 엔진이 다시 시작되었고 곡을 잃었습니다. 메모리를 쓰는 다른 프로그램을 닫거나 "설정 → 모델"에서 더 작은 모델 세트나 더 짧은 곡을 고르세요.',
  lost_device: '이 곡을 만드는 중 그래픽카드에 오류가 나서({detail}) 엔진이 다시 시작되었고 곡을 잃었습니다. 엔진 로그는 "설정 → 로컬 엔진"에 있습니다.',
  lost_device_silent: '이 곡을 만드는 중 그래픽카드에 오류가 나서(CUDA 또는 드라이버 오류) 엔진이 다시 시작되었고 곡을 잃었습니다. 엔진 로그는 "설정 → 로컬 엔진"에 있습니다.',
  lost_at_line: '이 곡을 만드는 중 엔진이 멈췄다가 다시 시작되어 곡을 잃었습니다. 마지막 줄: {line}. 엔진 로그는 "설정 → 로컬 엔진"에 있습니다.',
  lost: '이 곡을 만드는 중 엔진이 멈췄다가 다시 시작되어 곡을 잃었습니다. 엔진 로그는 "설정 → 로컬 엔진"에 있습니다.',
  lost_exited: '이 곡을 만드는 중 엔진이 비정상 종료되었고(종료 코드 {exit}) 다시 시작되어 곡을 잃었습니다. 엔진 로그는 "설정 → 로컬 엔진"에 있습니다.',
  lost_port_set: '엔진이 YUE_ENGINE_PORT에 지정된 포트 {port}에서 대기할 수 없어 곡을 잃었습니다. 다른 프로그램이 그 포트를 쓰고 있습니다. 그 프로그램을 닫거나 다른 포트를 지정하세요.',
  lost_port_taken: '엔진이 포트 {port}에서 대기할 수 없어 곡을 잃었습니다. 스튜디오가 포트를 고른 뒤 다른 프로그램이 그 포트를 차지했습니다. 스튜디오를 다시 시작하세요. 시작할 때 빈 포트를 고릅니다.',
  lost_illegal_instruction: '이 곡을 만드는 중 엔진이 이 프로세서에 없는 명령에서 멈췄고(종료 코드 {exit}) 다시 시작되어 곡을 잃었습니다. "설정 → 로컬 엔진"의 엔진 로그와 함께 알려 주세요.',
  load_vram: '엔진이 모델을 불러오는 동안 그래픽카드 메모리가 부족했습니다. "설정 → 모델"에서 더 작은 양자화를 고르거나 카드를 쓰는 다른 것을 닫으세요. 작사 도우미만으로도 몇 GB를 씁니다.',
  load_ram: '엔진이 모델을 불러오는 동안 컴퓨터 메모리(RAM)가 부족했습니다. 메모리를 쓰는 다른 프로그램을 닫거나 "설정 → 모델"에서 더 작은 양자화를 고르세요.',
  engine_fatal: '엔진이 이 곡을 멈췄습니다: {line}',
  engine_failed: '엔진이 이 곡을 멈췄습니다. 이유는 "설정 → 로컬 엔진"의 로그에 있습니다.',
  engine_unreachable: '엔진이 이 곡에 대해 더 이상 응답하지 않습니다: {detail}',
  import_failed: '엔진은 곡을 완성했지만 스튜디오가 라이브러리로 가져오지 못했습니다: {detail}',
  adapter_missing: 'LoRA {name}이(가) 설치되어 있지 않습니다. LoRA 페이지에서 다시 추가하세요.',
  vocals_only_no_separator: '"보컬만"에는 분리 모델이 필요합니다. "스튜디오 도구"에서 설치하세요.',
  separator_unavailable: '분리 모델을 이 컴퓨터에서 실행할 수 없습니다: {detail}',
  engine_refused: '엔진이 이 곡을 받지 않았습니다: {detail}',
  context_short: '스타일, 가사, 악보가 모델의 {context} 토큰 중 {tokens}을(를) 차지해 이 곡은 최대 {can_last}초인데 {asked}초가 요청되었습니다. 곡이나 가사를 줄이세요.',
  score_unsingable: '모델이 곡 {song}을(를) 위해 쓴 악보는 부를 수 없습니다({problem}). 다른 시드를 써 보세요.',
  score_unsingable_adapters: '모델이 곡 {song}을(를) 위해 쓴 악보는 부를 수 없습니다({problem}). 악보 쪽에서 LoRA를 한도 넘게 세게 걸면 이렇게 됩니다. 강도를 낮추거나 다른 시드를 써 보세요.',
};

const BY_LANGUAGE: Record<Language, ErrorTexts> = { en: EN, ru: RU, zh: ZH, ja: JA, ko: KO };

/** A failed song as the service sent it: its English text and, when the reason has a kind, a code with fields. */
export interface SongFailure {
  message?: string | null;
  code?: string | null;
  params?: Record<string, unknown> | null;
}

/** Why a song failed, in the window's language. Without a code, the service's text as it is: it shows there is no translation. */
export function songErrorText(failure: SongFailure | null | undefined, language: Language): string {
  const raw = failure?.message ?? '';
  const template = failure?.code ? BY_LANGUAGE[language][failure.code] : undefined;
  if (!template) return raw;
  const params = failure?.params ?? {};
  return template.replace(/\{(\w+)\}/g, (match, key: string) => {
    const value = params[key];
    return value === undefined || value === null ? match : String(value);
  });
}
