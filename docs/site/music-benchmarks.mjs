// The prior English-only YuE2 and RVC benchmarks keep their original figures and wording.
export const NATIVE_YUE2 = `    <h3>YuE2 · native yue2.cpp 892b9b0</h3>
    <p>Windows 11 (build 26100), Ryzen 9 9950X3D and RTX 4090 24 GB (driver 617.42). All runs used YuE2 3B Q8_0, the F32 VAE, NAR companion v9 and seed 1221. Replay used 100 fixed semantic tokens and two synthesis steps. Text to song started from style and lyrics and used 32 synthesis steps.</p>
    <div class="scroll"><table>
      <thead><tr><th>Workload</th><th>Backend</th><th>Start to first WAV (s)</th><th>Warm median (s)</th><th>Peak GPU (GiB)</th><th>Above GPU baseline (GiB)</th><th>Peak process-tree RSS (GiB)</th></tr></thead>
      <tbody>
        <tr><td>Semantic replay (3.999 s)</td><td>CUDA 13 · cuBLAS 13.10.0.20</td><td>6.516</td><td>0.172</td><td>6.88</td><td>4.51</td><td>3.40</td></tr>
        <tr><td>Semantic replay (3.999 s)</td><td>CUDA 12 · cuBLAS 12.9.2.10</td><td>4.969</td><td>0.125</td><td>6.88</td><td>4.50</td><td>3.45</td></tr>
        <tr><td>Semantic replay (3.999 s)</td><td>Vulkan</td><td>13.766</td><td>0.252</td><td>6.54</td><td>4.17</td><td>3.82</td></tr>
        <tr><td>Semantic replay (3.999 s)</td><td>CPU</td><td>16.234</td><td>3.503</td><td>—</td><td>—</td><td>6.41</td></tr>
        <tr><td>Text to song (29.999 s)</td><td>CUDA 13 · cuBLAS 13.10.0.20</td><td>8.813</td><td>4.759</td><td>8.13</td><td>5.75</td><td>3.37</td></tr>
        <tr><td>Text to song (29.999 s)</td><td>Vulkan</td><td>16.609</td><td>5.861</td><td>8.19</td><td>5.81</td><td>3.08</td></tr>
      </tbody>
    </table></div>
    <p class="note">Cold time includes server startup and the first request through receipt of the encoded WAV. Warm time is the median of three requests with the models kept loaded. Memory was sampled every 200 ms during startup and the first request. GPU peak is for the whole RTX 4090. The increase uses the reading before server launch as the baseline. CPU runs report process-tree RSS alone.</p>
`;

export const RVC = `    <h3>RVC voice conversion on GPU (5.0 development builds)</h3>
    <p>Windows on one RTX 4090 24 GB, using the same voice checkpoint and FAISS index with a retrieval blend of 0.5. Each run converts an existing vocal to a new voice.</p>
    <div class="scroll"><table>
      <thead><tr><th>RVC backend</th><th>Input</th><th>First run, empty cache</th><th>Restart, saved cache</th><th>Warm request</th><th>Peak GPU memory</th></tr></thead>
      <tbody>
        <tr><td>vocal-fast-rs · TensorRT-RTX</td><td>4 s</td><td>21.046 s</td><td>1.694 s</td><td>64.83 ms</td><td>3.91 GiB</td></tr>
        <tr><td>audio.cpp · CUDA</td><td>4 s</td><td>4.639 s</td><td>4.448 s</td><td>204.84 ms</td><td>4.09 GiB</td></tr>
        <tr><td>vocal-fast-rs · TensorRT-RTX</td><td>30 s</td><td>24.258 s</td><td>2.009 s</td><td>279.29 ms</td><td>5.51 GiB</td></tr>
        <tr><td>audio.cpp · CUDA</td><td>30 s</td><td>5.334 s</td><td>5.477 s</td><td>942.17 ms</td><td>6.83 GiB</td></tr>
      </tbody>
    </table></div>
    <p class="note">Startup runs include process launch through the first saved WAV. Columns show medians of two empty-cache launches, three cached launches and three warm requests. Both warm paths use the same local HTTP client, from POST through writing the WAV. Peak memory is the median whole-card peak across three cached processes during warm conversion, including desktop usage. The engines search different numbers of FAISS lists; timing alone does not measure voice quality. <a href="rvc-gpu-benchmarks.json">Individual runs and build identifiers</a></p>
`;

export const MATRIX_TEXT = {
  "en": {
    "title": "Music engine GPU matrix · 5.0 development and published builds",
    "note": "Windows 11, RTX 4090, driver 617.42. Cold includes server launch and the first WAV response. Warm is the median of three requests to the same server. Memory peaks are cold-run whole-card VRAM and process-tree RSS. Compare rows within each workload and check the weight profiles, silence and exact build before drawing a speed conclusion. Warm pre-request VRAM is the median reading for the whole card immediately before those three requests. Resident WDDM VRAM and RSS are medians of three idle process-tree samples after one warm-up on the published CUDA 13 builds.",
    "columns": [
      "Model and engine",
      "Backend",
      "WAV (s)",
      "Cold (s)",
      "Warm (s)",
      "Warm audio s/s",
      "VRAM peak / Δ (GiB)",
      "Warm pre-request VRAM (GiB)",
      "Peak RAM (GiB)",
      "Resident WDDM VRAM / RSS (GiB)"
    ],
    "detail": "Runs, SHA-256 and weight profiles"
  },
  "ru": {
    "title": "Музыкальные движки на GPU · тесты для 5.0 и выпущенные сборки",
    "note": "Windows 11, RTX 4090, драйвер 617.42. Холодный замер включает запуск сервера и получение первого WAV. Прогретый — медиана трёх запросов к тому же серверу. Пики холодного прогона относятся ко всей карте и RSS дерева процессов. Сравнивайте строки одной задачи с учётом весов, тишины и ревизий. VRAM до прогретого запроса — медиана трёх показаний всей карты непосредственно перед этими запросами. Резидентная VRAM WDDM и RSS — медианы трёх замеров дерева процессов в простое после одного прогрева опубликованных сборок CUDA 13.",
    "columns": [
      "Модель и движок",
      "Бэкенд",
      "WAV (с)",
      "Холодный (с)",
      "Прогретый (с)",
      "Аудиосекунд/с",
      "Пик / Δ VRAM (ГиБ)",
      "VRAM до прогретого запроса (ГиБ)",
      "Пик RAM (ГиБ)",
      "Резидентная VRAM WDDM / RSS (ГиБ)"
    ],
    "detail": "Замеры, SHA-256 и наборы весов"
  },
  "pt": {
    "title": "Motores de música na GPU · testes para a versão 5.0 e versões publicadas",
    "note": "Windows 11, RTX 4090, controlador 617.42. O arranque inclui o início do servidor até à primeira resposta WAV. A mediana aquecida usa três pedidos no mesmo servidor. Os picos no arranque incluem a VRAM da placa e o RSS da árvore de processos. Compare os resultados da mesma tarefa tendo em conta os pesos, o silêncio e as revisões. A VRAM antes do pedido aquecido é a mediana das três leituras da placa completa imediatamente antes dos pedidos. A VRAM WDDM e o RSS residentes são as medianas de três leituras da árvore de processos em repouso após um aquecimento nas versões publicadas com CUDA 13.",
    "columns": [
      "Modelo e motor",
      "Backend",
      "WAV (s)",
      "Arranque (s)",
      "Aquecido (s)",
      "Áudio s/s",
      "Pico / Δ VRAM (GiB)",
      "VRAM antes do pedido aquecido (GiB)",
      "Pico de RAM (GiB)",
      "VRAM WDDM / RSS residentes (GiB)"
    ],
    "detail": "Execuções, SHA-256 e conjuntos de pesos"
  },
  "zh": {
    "title": "音乐引擎 GPU 测试矩阵 · 5.0 开发版与已发布版本",
    "note": "Windows 11、RTX 4090、驱动 617.42。冷启动计时包含服务器启动至收到首个 WAV，预热后为同一服务器三次请求的中位数。内存数据为冷启动时整卡显存和进程树 RSS 峰值。比较同一任务时也需核对权重、静音段和构建版本。 热运行请求前显存为三次请求之前整张显卡显存读数的中位数。 驻留 WDDM 显存和 RSS 为已发布 CUDA 13 版本预热一次后，进程树空闲时三次测量的中位数。",
    "columns": [
      "模型和引擎",
      "后端",
      "WAV（秒）",
      "冷启动（秒）",
      "预热后（秒）",
      "音频秒/秒",
      "显存峰值 / 增量（GiB）",
      "热运行请求前显存（GiB）",
      "内存峰值（GiB）",
      "驻留 WDDM 显存 / RSS（GiB）"
    ],
    "detail": "各次运行、SHA-256 和权重配置"
  },
  "ja": {
    "title": "音楽エンジンの GPU 比較 · 5.0 開発版と公開版",
    "note": "Windows 11、RTX 4090、ドライバー 617.42。初回はサーバー起動から最初の WAV 受信までです。再実行は同じサーバーに送った 3 回の中央値です。メモリーは初回のカード全体とプロセスツリー RSS のピークです。同じ処理を比較し、重み、無音区間、リビジョンもご確認ください。 ウォームリクエスト前の VRAM は、各リクエストの直前に測定した GPU 全体の使用量の中央値です。 常駐 WDDM VRAM と RSS は、公開版の CUDA 13 で 1 回実行した後、アイドル状態のプロセスツリーを 3 回測定した中央値です。",
    "columns": [
      "モデルとエンジン",
      "バックエンド",
      "WAV（秒）",
      "初回（秒）",
      "再実行（秒）",
      "音声秒/秒",
      "VRAM ピーク / 増分（GiB）",
      "ウォームリクエスト前の VRAM（GiB）",
      "RAM ピーク（GiB）",
      "常駐 WDDM VRAM / RSS（GiB）"
    ],
    "detail": "個別の計測結果、SHA-256、重みの構成"
  },
  "ko": {
    "title": "음악 엔진 GPU 비교 · 5.0 개발 빌드와 출시 버전",
    "note": "Windows 11, RTX 4090, 드라이버 617.42. 첫 실행은 서버 시작부터 첫 WAV 응답까지입니다. 재실행은 같은 서버에 보낸 3개 요청의 중앙값입니다. 메모리는 첫 실행의 전체 GPU 및 프로세스 트리 RSS 최대값입니다. 같은 작업에서 가중치, 무음 구간 및 빌드 버전을 확인하세요. 웜 요청 전 VRAM은 세 요청 직전에 측정한 전체 GPU 사용량의 중앙값입니다. 상주 WDDM VRAM과 RSS는 출시된 CUDA 13 빌드를 한 번 예열한 후 유휴 상태의 프로세스 트리에서 측정한 세 값의 중앙값입니다.",
    "columns": [
      "모델 및 엔진",
      "백엔드",
      "WAV (초)",
      "첫 실행 (초)",
      "재실행 (초)",
      "오디오 초/초",
      "VRAM 최대 / 증가 (GiB)",
      "웜 요청 전 VRAM (GiB)",
      "RAM 최대 (GiB)",
      "상주 WDDM VRAM / RSS (GiB)"
    ],
    "detail": "개별 실행, SHA-256 및 가중치 구성"
  }
};
