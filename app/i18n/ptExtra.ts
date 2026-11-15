import type { Guide } from './trainingGuide';

export const ptSongPartHints = {
  languageHint: 'A língua em que a letra é cantada. Automática: o modelo percebe-a pela letra.',
  tempoHint: 'Batidas por minuto, escritas no estilo. Automático: o modelo escolhe um tempo para o género; definir quando a música precisa de uma velocidade certa.',
  voiceHint: 'Quem canta, em palavras inglesas: "sweet female vocal", "raspy male voice". Automática: o modelo tira a voz do género.',
  soundHint: 'Mais uma parte do estilo: um género, um ambiente ou um instrumento, em palavras inglesas que o modelo conhece ("lo-fi", "warm piano").',
};

export const ptExitMeanings: Record<string, string> = {
  access_violation: 'uma falha: violação de acesso à memória, ',
  illegal_instruction: 'uma instrução que este processador não tem, ',
  aborted: 'uma falha: o motor abortou, ',
  stack_overflow: 'uma falha: transbordo da pilha, ',
  corrupted_memory: 'uma falha: memória corrompida, ',
  library: 'uma biblioteca do motor não foi carregada, ',
  out_of_memory: 'o computador ficou sem memória, ',
  none: '',
};

export const ptSongTokenStrings = { tokens: 'tokens', tokenLoading: 'A ler o tokenizador do checkpoint…' };

export const ptSongWritingStrings = {
  parts: 'Editar partes do estilo', sections: 'Editar secções', add: 'Adicionar', duplicate: 'Duplicar', remove: 'Eliminar',
  up: 'Subir', down: 'Descer', tag: 'Etiqueta de secção', words: 'Palavras', language: 'Língua da voz', auto: 'Automática',
  lines: 'Linhas cantadas', instructions: 'Instruções adicionais', descriptor: 'Descritor de estilo',
};

export const ptSongDetailStrings = {
  tempo: 'Tempo', voice: 'Vozes', preset: 'Tipo de secção', cycle: 'Etiqueta de secção seguinte',
  lineEditor: 'Editar linhas e maiúsculas', addLine: 'Adicionar linha', duplicateStanza: 'Duplicar estrofe',
  deleteStanza: 'Eliminar estrofe', flipCase: 'Trocar maiúsculas/minúsculas', sound: 'Som / género',
  caseHint: 'Clicar numa letra para mudar entre maiúscula e minúscula. O modelo recebe as letras editadas.',
};

export const ptEngineParityLabels = {
  realaudio: 'Descodificador realaudio',
  realaudioHint: 'O adaptador realaudio de Mothersuperior na fase do som, treinado com o tokenizador do mesmo autor em gravações reais. Auto deixa-o desligado — o som do checkpoint YuE2 tal como os autores, o audio.cpp e o ComfyUI o renderizam — e liga-o com uma LoRA treinada no estúdio, que foi treinada por cima dele.',
  realaudioAuto: 'Auto', realaudioOn: 'Ligado', realaudioOff: 'Desligado',
  lyricTiming: 'As palavras seguem a partitura',
  lyricTimingHint: 'As palavras de cada secção esperam até a partitura lá chegar, para a voz acompanhar a banda numa música longa ou numa versão.',
  transpose: 'Transpor a partitura',
  transposeHint: 'Meios-tons antes de cantar. Compor ou carregar primeiro uma partitura; uma interpretação já feita não pode ser transposta.',
  vocals: 'Só a voz',
  vocalsHint: 'Extrair a voz depois da geração com o separador de pistas instalado. A voz fica no formato abaixo, FLAC sem perdas ou MP3.',
};

export const ptPlanLabel = 'Planos de partitura';

export const ptMidiImportOptions = {
  grid: 'Grelha de tempo', auto: 'Automática', voice: 'Voz: oitavas adicionais', instrument: 'Instrumento: oitavas adicionais',
  hint: 'Cada linha da partitura fica com a nota de cima das notas sobrepostas. As oitavas aplicam-se depois do ajuste automático da extensão.',
  keep: 'Manter a partitura editada para outra letra',
  keepHint: 'Permitir que esta melodia seja cantada depois de o estilo ou a letra mudarem.',
};

export const ptTrainingGuide: Guide = {
  title: 'Guia de treino de LoRA',
  intro: 'Uma LoRA é um pequeno complemento ao modelo que aprende a soar como as suas músicas: a maneira de cantar, a voz, os arranjos, a produção. Arrastar esta janela pelo título e redimensioná-la pelo canto inferior direito.',
  expandAll: 'Expandir tudo',
  collapseAll: 'Recolher tudo',
  close: 'Fechar',
  resize: 'Arrastar para redimensionar',
  sections: [
    {
      title: 'Como funciona',
      steps: [
        'Largar na página Treino uma pasta de músicas de um artista ou de um estilo, ou escolher uma pasta ou ficheiros com os botões. O estúdio cria um conjunto de dados com o nome da pasta. Funcionam WAV, MP3, FLAC, OGG e M4A; um álbum num só ficheiro com .cue é cortado nas suas músicas, e a letra num .txt ou .lrc ao lado de uma música é usada tal como está.',
        'Passo 1 · Músicas: a preparação começa sozinha. O estúdio vai buscar as letras às bases de dados, separa a voz e reconhece só as músicas que elas não conhecem, ouve cada música e escreve o estilo com o tempo medido. Cada música mostra o seu estado, e o progresso geral está em cima.',
        'Cada música guarda onde parou. Se o estúdio for fechado ou falhar durante a preparação, o trabalho continua sozinho do mesmo sítio no próximo arranque, e nada do que já foi feito é repetido. Uma música que falhou tem o botão Tentar de novo, uma por acabar tem Terminar esta música: acabam só essa música.',
        'Verificar o resultado: clicar numa música abre-a com um leitor, o estilo e a letra. Corrigir o que for preciso; Descrever de novo e Procurar de novo refazem só essa música.',
        'Passo 2 · Treino: o nome da LoRA, uma palavra-chave (o estúdio cria uma palavra rara a partir do nome do conjunto de dados; pode ser mudada ou apagada), a verificação de que está tudo pronto e o botão Treinar. Os ficheiros de treino (cerca de 8,6 GB, uma vez) descarregam-se ali mesmo. Precisa de uma placa NVIDIA RTX série 30 ou mais recente e de cerca de 11 GB de memória de vídeo.',
        'Não é preciso esperar: durante a preparação, marcar «Iniciar o treino sozinho quando todas as músicas estiverem prontas».',
        'Passo 3 · Resultado: ouvir os checkpoints e carregar em Para LoRA no melhor; aparece na página LoRA. Durante o treino, a geração, o assistente, o karaoke e a separação de pistas não estão disponíveis.',
      ],
    },
    {
      title: 'Descrição automática das músicas',
      text: [
        'Um pacote opcional (cerca de 10,5 GB), descarregado uma vez a partir do cartão «Descrever músicas automaticamente», no passo Músicas. Sem ele, o estúdio continua a reconhecer a letra, e o estilo é escrito à mão.',
        'Cada modelo é carregado uma vez para todo o conjunto de dados e libertado quando a sua fase termina: as bases de dados de letras, depois a voz e o reconhecimento para o que elas não têm, depois a audição, depois o assistente. A placa tem um modelo de cada vez. Por isso, um conjunto de cinquenta músicas não demora cinquenta vezes mais do que uma.',
      ],
      list: [
        'O MOSS-Music-8B é um modelo que ouve uma música e descreve o que tem: género, voz, instrumentos, ambiente, produção. O autor do treinador HOT-Step faz o mesmo: uma descrição de ouvido é muito mais precisa do que uma feita pelo título.',
        'O Beat This! é uma rede que encontra as batidas numa gravação; o estúdio calcula o tempo a partir delas.',
        'O assistente do estúdio transforma tudo isto numa linha de estilo YuE2 e põe o tempo medido no fim. O MOSS erra o tempo sozinho, por isso o seu número é sempre substituído pelo medido. A tonalidade não é escrita num estilo YuE2: o modelo escolhe-a ao escrever a partitura.',
        'O MOSS ouve até 7 minutos de uma música, e quanto mais ouve, mais memória de vídeo ocupa: de 11 GB para um minuto a 18 GB para sete. Numa placa mais pequena ouve o que couber do meio da música; numa placa com menos de 12 GB usa a versão mais pequena Q4_K_M; numa placa com menos de 8 GB ou sem CUDA 13 corre no processador, uns minutos por música. Numa RTX 4090 uma música é descrita em 6–7 segundos. Tudo corre no computador, nada é enviado para lado nenhum.',
        'Verificar sempre o resultado: o modelo pode errar o género ou os instrumentos; corrigir a linha à mão.',
      ],
    },
    {
      title: 'Que músicas usar',
      list: [
        'Um artista ou um estilo bem definido. Uma mistura não ensina nada em particular.',
        'Normalmente 5 a 20 músicas. O autor do treinador não viu diferença fiável entre 10 e 20 faixas; álbuns com muita letra ou estilos misturados são mais difíceis de aprender.',
        'Qualidade de gravação uniforme: versões de estúdio, sem ruído de concerto, jingles de rádio ou fragmentos cortados.',
        'Músicas inteiras: o treinador corta-as em pedaços de 10 segundos.',
        'Não adicionar a mesma música duas vezes (original e remasterização) — inclina a LoRA para ela.',
        'Para pronúncia de Portugal: vozes reais portuguesas, bem gravadas, com a letra correta ao lado de cada música.',
      ],
    },
    {
      title: 'Estilo — o que escrever',
      text: [
        'Uma frase em inglês de cerca de 35–70 palavras, por esta ordem: língua → género e época → voz → instrumentos → ambiente → produção → tempo como "N BPM".',
        'Não nomear o artista nem a música, não indicar a tonalidade nem o compasso, não citar a letra. Num instrumental, escrever "instrumental" no lugar da língua.',
        'Para português de Portugal, começar por "European Portuguese-language" e não só "Portuguese", que puxa para o Brasil.',
        'Tirar o BPM de um analisador ou de uma base de dados de faixas em vez de adivinhar: um tempo errado ensina a coisa errada.',
        'Não escrever a palavra-chave no estilo — o estúdio acrescenta-a sozinho.',
      ],
      examples: [
        {
          label: 'Exemplo',
          body: 'European Portuguese-language electronic fado with deep house grooves, male lead vocal with traditional Lisbon fado phrasing and long melismatic lines, Portuguese guitar, classical guitar, warm sub bass and four-on-the-floor drums, longing and nostalgic mood with a hopeful lift, polished modern club production with a wide stereo image, 122 BPM',
        },
      ],
    },
    {
      title: 'Letra',
      list: [
        'Exatamente as palavras cantadas, sem acordes, ligações ou notas. Verificar sempre as letras de sites com a gravação.',
        'Marcar as partes: [Verse 1], [Chorus], [Verse 2], [Bridge], [Outro], cada parte numa linha nova.',
        'Um .txt ou .lrc com o mesmo nome do áudio é usado quando a música é adicionada; os tempos de um .lrc são removidos.',
        'Quando não há letra, o estúdio procura-a em bases de dados abertas — LRCLIB, QQ Music, Kugou — pelo artista, título e duração (o artista e o título vêm das etiquetas do ficheiro, ou do nome e das pastas). Só quando nenhuma base de dados conhece a música é que a voz é separada e as palavras reconhecidas pelo Whisper, muito menos preciso. Por cima da letra diz de onde veio. Verificar sempre o resultado.',
        'Se o reconhecedor não ouvir palavras, a música é marcada como instrumental. Se estiver errado, desmarcar Instrumental e carregar em Procurar de novo.',
      ],
    },
    {
      title: 'O que o estúdio faz sozinho',
      list: [
        'Converte tudo para WAV a 48 kHz e corta em pedaços de 10 segundos.',
        'Cria uma partitura para cada música (SheetSage2).',
        'Põe a palavra-chave no treino, e no estilo quando esta LoRA é escolhida para uma música.',
        'Guarda um checkpoint a cada 10 passos e no fim do treino.',
        'Na receita «Como antes», também separa a voz das músicas com letra, liga as palavras ao tempo na voz (o modelo aprende onde cada palavra é cantada) e para o treino sozinho quando a LoRA está suficientemente próxima (pela métrica KL).',
      ],
    },
    {
      title: 'O que tem de ser feito à mão',
      list: [
        'Escolher as músicas e verificar a sua qualidade.',
        'Verificar os estilos e as letras que o estúdio escreveu: o modelo e o reconhecimento enganam-se. Sem o pacote de descrição automática, o estilo é escrito à mão.',
        'Sem o pacote de descrição automática, descobrir o tempo (BPM); com ele, o estúdio mede-o sozinho.',
        'Escolher um checkpoint de ouvido.',
      ],
    },
    {
      title: 'Definições do treino',
      text: [
        'O predefinido é a receita base-matched, a principal do autor do treinador HOT-Step: a LoRA aprende como o próprio modelo base foi treinado, segundo o relatório técnico do modelo. Não mexer nas definições sem motivo.',
      ],
      list: [
        'A receita define o tamanho: Rápida, Equilibrada (a predefinida) e Minuciosa, cada uma mais longa do que a anterior.',
        'Dentro da receita: AdamW a 1e-4 com um aquecimento curto, a descer até 10% da taxa no fim; a composição aprende com peso 0,25 e só no que o modelo gera sozinho; 10% dos passos deixam de fora o estilo, 10% a letra, 10% os dois; a metade do som vê a música inteira. Não são precisos tempos da letra, por isso a separação de voz e o alinhamento são saltados, e as gravações mantêm o seu volume.',
        'LoKr 64 / fator 4 / alpha 256 — um adaptador leve (cerca de 106 MB); pode escolher-se LoRA em vez disso.',
        '«Como antes: parar pelo KL» é a receita anterior do estúdio: Prodigy, paragem em KL = 1,4 (a semelhança começa por volta de 1,25, o modelo degrada-se por volta de 1,9), um teto de 750 passos, um checkpoint a cada 50 passos, tempos da letra (precisa do separador de voz); a paragem pode passar a ser por épocas.',
        'Um treino iniciado numa versão anterior do estúdio continua com a receita com que começou.',
      ],
    },
    {
      title: 'Escolher um checkpoint e gerar',
      list: [
        'Escolher de ouvido, não pelo gráfico da perda: gerar a mesma música com checkpoints diferentes.',
        'Carregar em «Para LoRA» debaixo do passo escolhido — a LoRA aparece na página LoRA como «treino · passo».',
        'Ao escolher a LoRA na página Criar, a palavra-chave é acrescentada sozinha.',
        'A LoRA tem duas forças: composição (AR) e som (NAR), 1 e 1 por defeito. O autor do treinador ouviu o melhor som com NAR por volta de 2.',
      ],
    },
    {
      title: 'Quando algo corre mal',
      list: [
        'As músicas repetem-se em ciclo, não têm fim, a voz desfaz-se — a LoRA está sobretreinada: usar um checkpoint anterior ou baixar a força.',
        'A LoRA quase não muda nada — usar um checkpoint posterior ou a receita Minuciosa, verificar os estilos e as letras, acrescentar músicas.',
        'Em «Como antes» as palavras fogem da música — verificar a letra: linhas a mais e repetições que não estão na gravação estragam os tempos.',
        'O treino «Como antes» não arranca, com um erro sobre o separador — instalar o separador de voz ou desligar os tempos da letra.',
      ],
    },
    {
      title: 'Lista antes de treinar',
      checklist: [
        'Músicas de um artista ou estilo, 5–20, de boa qualidade.',
        'Sem duplicados nem fragmentos.',
        'Cada música tem um estilo de uma frase em inglês com BPM, sem o artista nem o título.',
        'Letra verificada com a gravação e marcada com [Verse] / [Chorus].',
        'Instrumentais marcados, músicas com voz desmarcadas.',
        'Sem triângulos amarelos.',
        'Uma palavra-chave rara definida.',
        'Cerca de 11 GB de VRAM livres, geração parada.',
      ],
    },
  ],
};
