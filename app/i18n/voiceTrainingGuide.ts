import type { Language } from './translations';
import { trainingGuide, type Guide } from './trainingGuide';

/**
 * The guide to training a voice. The numbers follow the RVC project's training notes and the AI Hub and Applio
 * dataset guides; the steps follow the studio's own trainer (it slices the recordings, trains on an NVIDIA card and
 * keeps the best version it heard).
 */
const sections: Record<Language, Pick<Guide, 'title' | 'intro' | 'sections'>> = {
  ru: {
    title: 'Справка по обучению голоса',
    intro: 'Голос (модель RVC) учится тембру одного человека по его чистому пению. Потом этим голосом можно перепеть любую песню в разделе «Голоса»: мелодия и слова остаются, меняется тембр.',
    sections: [
      { title: 'Что нужно', list: [
        'Минимум 10 минут чистого пения одного человека; лучше 15–40 минут — голос станет гибче и естественнее.',
        'Только голос: без музыки, без реверба и эха, без бэк-вокала и двойных дорожек.',
        'Разные ноты, громкость и манера — модель споёт только то, что слышала.',
        'Лучше WAV или FLAC; MP3 подойдёт, если хорошего качества.',
      ] },
      { title: 'Как подготовить записи', steps: [
        'Если голос есть только внутри песни — откройте «Инструменты → Разделить на стемы», выберите модель «Вокал и минус» и возьмите дорожку «Вокал».',
        'Уберите длинные паузы, кашель, шумы и места, где поёт кто-то другой.',
        'Не добавляйте компрессию и эффекты: модель выучит их вместе с голосом.',
        'Перетащите файлы или папку в зону на вкладке «Обучение».',
      ] },
      { title: 'Как идёт обучение', text: [
        'Студия режет записи на короткие куски и учит голос на видеокарте NVIDIA. Ход видно на шкале, лучшая версия сохраняется сама по ходу обучения.',
        'Обычно хватает 200–300 эпох на 10–15 минут записей и 100–200 эпох на 30–40 минут: на большом наборе каждая эпоха длиннее.',
      ] },
      { title: 'Как понять, что голос переучен', list: [
        'На нотах, которых не было в записях, голос звучит металлически, роботом или с треском.',
        'Сравните сохранённые версии на незнакомой песне и оставьте ту, что звучит естественнее, — не обязательно последнюю.',
      ] },
      { title: 'Перед стартом', checklist: [
        'Поёт один человек',
        'Нет музыки, эха и бэк-вокала',
        'Записей не меньше 10 минут',
        'Длинные паузы и шумы убраны',
      ] },
    ],
  },
  en: {
    title: 'Voice training guide',
    intro: 'A voice (an RVC model) learns one person’s timbre from their clean singing. Then the Voices section sings any song in that voice: the melody and words stay, the timbre changes.',
    sections: [
      { title: 'What you need', list: [
        'At least 10 minutes of one person singing cleanly; 15–40 minutes is better — the voice gets more flexible and natural.',
        'The voice only: no music, no reverb or echo, no backing vocals or doubled tracks.',
        'Different notes, loudness and phrasing — the model only sings what it has heard.',
        'WAV or FLAC is best; a good MP3 works.',
      ] },
      { title: 'Preparing the recordings', steps: [
        'If the voice exists only inside a song, open Tools → Split into stems, choose “Vocals and backing” and take the Vocals track.',
        'Cut long pauses, coughs, noise and places where someone else sings.',
        'Add no compression or effects: the model learns them along with the voice.',
        'Drop the files or the folder onto the Training tab.',
      ] },
      { title: 'How training goes', text: [
        'The studio slices the recordings into short pieces and trains the voice on an NVIDIA card. The bar shows the progress, and the best version is kept as training goes.',
        'Usually 200–300 epochs are enough for 10–15 minutes of recordings and 100–200 for 30–40 minutes: each epoch is longer on a larger set.',
      ] },
      { title: 'Telling an overtrained voice', list: [
        'On notes the recordings did not have, the voice sounds metallic, robotic or crackly.',
        'Compare the saved versions on an unfamiliar song and keep the most natural one — not necessarily the last.',
      ] },
      { title: 'Before you start', checklist: [
        'One person sings',
        'No music, echo or backing vocals',
        'At least 10 minutes of recordings',
        'Long pauses and noise removed',
      ] },
    ],
  },
  zh: {
    title: '声音训练说明',
    intro: '声音（RVC 模型）从一个人干净的演唱中学习他的音色。之后可以在“声音”中用这个声音重唱任何歌曲：旋律和歌词不变，音色改变。',
    sections: [
      { title: '需要什么', list: [
        '至少 10 分钟同一个人的干净演唱；15–40 分钟更好——声音更灵活自然。',
        '只要人声：没有音乐、混响和回声，没有和声或叠录。',
        '不同的音高、音量和唱法——模型只会唱它听过的。',
        '最好是 WAV 或 FLAC；音质好的 MP3 也可以。',
      ] },
      { title: '如何准备录音', steps: [
        '如果人声只在歌曲里，打开“工具 → 分离音轨”，选择“人声与伴奏”，取“人声”轨。',
        '剪掉长时间停顿、咳嗽、噪音和别人演唱的部分。',
        '不要加压缩和效果：模型会把它们和声音一起学进去。',
        '把文件或文件夹拖到“训练”选项卡的区域。',
      ] },
      { title: '训练过程', text: [
        '工作室把录音切成短片段，在 NVIDIA 显卡上训练声音。进度条显示进度，训练中会自动保存最好的版本。',
        '通常 10–15 分钟录音需要 200–300 个 epoch，30–40 分钟需要 100–200 个：数据越多，每个 epoch 越长。',
      ] },
      { title: '如何判断过度训练', list: [
        '在录音中没有的音高上，声音发金属声、机器声或噼啪声。',
        '在陌生的歌曲上比较保存的版本，保留最自然的——不一定是最后一个。',
      ] },
      { title: '开始之前', checklist: ['只有一个人演唱', '没有音乐、回声和和声', '录音至少 10 分钟', '已去掉长停顿和噪音'] },
    ],
  },
  ja: {
    title: '声の学習ガイド',
    intro: '声（RVC モデル）は、ひとりの人のクリーンな歌声から音色を学びます。その後「声」でどの曲もその声で歌い直せます。メロディーと歌詞はそのまま、音色が変わります。',
    sections: [
      { title: '必要なもの', list: [
        'ひとりのクリーンな歌声を最低 10 分。15〜40 分あると、より柔軟で自然な声になります。',
        '声だけ：音楽、リバーブやエコー、コーラスや重ね録りなし。',
        'さまざまな音の高さ、音量、歌い方——モデルは聴いたものしか歌えません。',
        'WAV か FLAC が最適。音質の良い MP3 でも構いません。',
      ] },
      { title: '録音の準備', steps: [
        '声が曲の中にしかない場合は「ツール → ステムに分離」を開き、「ボーカルと伴奏」を選んで「ボーカル」トラックを使います。',
        '長い無音、咳、ノイズ、別の人が歌う部分を取り除きます。',
        'コンプレッションやエフェクトをかけない：モデルが声と一緒に覚えてしまいます。',
        'ファイルかフォルダーを「学習」タブの領域にドロップします。',
      ] },
      { title: '学習の流れ', text: [
        'スタジオは録音を短い断片に切り、NVIDIA の GPU で声を学習します。進行はバーに表示され、学習中に最良のバージョンが自動で保存されます。',
        '録音 10〜15 分なら通常 200〜300 エポック、30〜40 分なら 100〜200 エポック：データが多いほど 1 エポックが長くなります。',
      ] },
      { title: '学習しすぎの見分け方', list: [
        '録音になかった音の高さで、声が金属的、ロボット的、またはノイズ混じりになります。',
        '保存されたバージョンを知らない曲で比べ、最も自然なものを残します——最後のものとは限りません。',
      ] },
      { title: '始める前に', checklist: ['歌っているのはひとり', '音楽、エコー、コーラスなし', '録音は 10 分以上', '長い無音とノイズを除去済み'] },
    ],
  },
  ko: {
    title: '목소리 학습 안내',
    intro: '목소리(RVC 모델)는 한 사람의 깨끗한 노래에서 음색을 배웁니다. 그다음 「목소리」에서 어떤 곡이든 그 목소리로 다시 부를 수 있습니다. 멜로디와 가사는 그대로, 음색만 바뀝니다.',
    sections: [
      { title: '필요한 것', list: [
        '한 사람의 깨끗한 노래 최소 10분, 15–40분이면 더 좋습니다 — 목소리가 더 유연하고 자연스러워집니다.',
        '목소리만: 음악, 리버브나 에코, 코러스나 더블링 없이.',
        '다양한 음높이, 크기, 창법 — 모델은 들은 것만 부릅니다.',
        'WAV나 FLAC가 가장 좋고, 음질 좋은 MP3도 됩니다.',
      ] },
      { title: '녹음 준비', steps: [
        '목소리가 곡 안에만 있다면 「도구 → 스템 분리」를 열고 「보컬과 반주」를 골라 「보컬」 트랙을 쓰세요.',
        '긴 정적, 기침, 잡음, 다른 사람이 부르는 부분을 잘라 내세요.',
        '컴프레션이나 효과를 넣지 마세요: 모델이 목소리와 함께 배웁니다.',
        '파일이나 폴더를 「학습」 탭의 영역에 끌어다 놓으세요.',
      ] },
      { title: '학습 진행', text: [
        '스튜디오가 녹음을 짧은 조각으로 자르고 NVIDIA 그래픽카드에서 목소리를 학습합니다. 진행은 막대로 보이고, 학습 중 가장 좋은 버전이 자동으로 저장됩니다.',
        '보통 녹음 10–15분에는 200–300 에포크, 30–40분에는 100–200 에포크면 됩니다: 데이터가 많을수록 에포크가 길어집니다.',
      ] },
      { title: '과학습 알아보기', list: [
        '녹음에 없던 음높이에서 목소리가 금속성, 로봇 같거나 지직거립니다.',
        '저장된 버전을 낯선 곡으로 비교해 가장 자연스러운 것을 남기세요 — 마지막 것이 아닐 수 있습니다.',
      ] },
      { title: '시작하기 전에', checklist: ['한 사람이 부름', '음악, 에코, 코러스 없음', '녹음 10분 이상', '긴 정적과 잡음 제거'] },
    ],
  },
  pt: {
    title: 'Guia de treino de voz',
    intro: 'Uma voz (um modelo RVC) aprende o timbre de uma pessoa a partir do seu canto limpo. Depois, em Vozes, qualquer música pode ser cantada com essa voz: a melodia e a letra ficam, o timbre muda.',
    sections: [
      { title: 'O que é preciso', list: [
        'Pelo menos 10 minutos de uma pessoa cantando limpo; 15–40 minutos é melhor — a voz fica mais flexível e natural.',
        'Só a voz: sem música, sem reverb ou eco, sem backing vocals ou dobras.',
        'Notas, volumes e jeitos diferentes — o modelo só canta o que ouviu.',
        'WAV ou FLAC é o melhor; um MP3 de boa qualidade serve.',
      ] },
      { title: 'Preparar as gravações', steps: [
        'Se a voz só existe dentro de uma música, abra Ferramentas → Separar em stems, escolha “Voz e acompanhamento” e use a faixa Voz.',
        'Corte pausas longas, tosses, ruídos e trechos em que outra pessoa canta.',
        'Não aplique compressão nem efeitos: o modelo aprende-os junto com a voz.',
        'Arraste os arquivos ou a pasta para a área da aba Treino.',
      ] },
      { title: 'Como corre o treino', text: [
        'O estúdio corta as gravações em pedaços curtos e treina a voz numa placa NVIDIA. A barra mostra o progresso, e a melhor versão é guardada durante o treino.',
        'Normalmente bastam 200–300 épocas para 10–15 minutos de gravações e 100–200 para 30–40 minutos: num conjunto maior cada época é mais longa.',
      ] },
      { title: 'Reconhecer uma voz sobretreinada', list: [
        'Em notas que as gravações não tinham, a voz soa metálica, robótica ou com estalos.',
        'Compare as versões guardadas numa música desconhecida e fique com a mais natural — não necessariamente a última.',
      ] },
      { title: 'Antes de começar', checklist: ['Canta uma pessoa', 'Sem música, eco nem backing vocals', 'Pelo menos 10 minutos de gravações', 'Pausas longas e ruídos removidos'] },
    ],
  },
};

export const voiceTrainingGuide: Record<Language, Guide> = Object.fromEntries(
  (Object.keys(sections) as Language[]).map(language => {
    const frame = trainingGuide[language] ?? trainingGuide.en;
    return [language, { ...frame, ...sections[language] }];
  }),
) as Record<Language, Guide>;
