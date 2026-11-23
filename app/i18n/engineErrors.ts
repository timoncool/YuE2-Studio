import type { Language } from './translations';

// Why the chosen model could not be prepared or its engine did not start, in the window's language: what happened
// and what to do. The service sends the code beside its English text (crates/music-server/src/music_pair.rs and
// `engine_error_code` in lib.rs); its test `every_code_has_a_translation_in_every_language` checks each code here.

type Texts = Record<string, string>;

const EN: Texts = {
  engine_no_answer: 'The engine started but did not answer in time. Restart it; if it happens again, the log is in Settings → Local engine.',
  cuda_backend_missing: 'A file of the engine\'s CUDA build is missing from the studio\'s folder. Reinstall the studio.',
  runtime_libraries_failed: 'The engine\'s libraries could not be downloaded. Check the connection and restart the engine.',
  device_missing: 'The engine found no graphics card it can run on. Update the graphics driver or choose the processor in Settings → Local engine.',
  cuda_unsupported: 'This graphics card or its driver runs neither CUDA build of the engine. Update the driver or choose Vulkan in Settings → Local engine.',
  engine_port_busy: 'Another program holds the engine\'s port. Close it or restart the computer, then retry.',
  engine_runtime_missing: 'The engine is missing from the studio\'s folder. Reinstall the studio.',
  model_set_not_selected: 'No model set is chosen. Choose the quality above and download it.',
  chosen_card_missing: 'The graphics card chosen in Settings is not in this computer. Choose another in Settings → Local engine.',
  engine_exited: 'The engine closed while starting. Restart it; if it happens again, report the problem with the log from Settings → Local engine.',
  engine_failed: 'The engine did not start. Restart it; if it happens again, report the problem.',
  yue2_model_set_missing: 'The chosen YuE2 quality is not downloaded. Download it above.',
  yue2_hip_missing: 'The YuE2 build for AMD cards is not installed. Install it in Settings → Local engine.',
  yue2_runtime_missing: 'The engine\'s libraries for this graphics card are not downloaded yet. Restart the engine in Settings → Local engine and the studio downloads them.',
  ace_files_missing: 'ACE-Step or the files of the chosen quality are not downloaded. Download them above.',
  minimax_files_missing: 'MiniMax Music3 or the files of the chosen quality are not downloaded. Download them above.',
  model_files_missing: 'This model is not downloaded. Download it above.',
  signed_package_gone: 'This model came from an old package the studio no longer uses. Choose another model.',
  openrouter_model_missing: 'No OpenRouter music model is chosen. Choose one above.',
  openrouter_key_missing: 'The OpenRouter key is not set. Add it in Settings → Providers.',
  unknown_model: 'The studio does not know this model any more. Choose another one.',
};

const RU: Texts = {
  engine_no_answer: 'Движок запустился, но не ответил вовремя. Перезапустите его; если повторится — журнал в Настройках → Локальный движок.',
  cuda_backend_missing: 'В папке студии нет файла сборки движка для CUDA. Переустановите студию.',
  runtime_libraries_failed: 'Не удалось скачать библиотеки движка. Проверьте интернет и перезапустите движок.',
  device_missing: 'Движок не нашёл видеокарту, на которой может работать. Обновите драйвер видеокарты или выберите процессор в Настройках → Локальный движок.',
  cuda_unsupported: 'Эта видеокарта или её драйвер не запускает ни одну сборку движка под CUDA. Обновите драйвер или выберите Vulkan в Настройках → Локальный движок.',
  engine_port_busy: 'Порт движка занят другой программой. Закройте её или перезагрузите компьютер и повторите.',
  engine_runtime_missing: 'В папке студии нет движка. Переустановите студию.',
  model_set_not_selected: 'Набор моделей не выбран. Выберите качество выше и скачайте его.',
  chosen_card_missing: 'Видеокарты, выбранной в настройках, нет в этом компьютере. Выберите другую в Настройках → Локальный движок.',
  engine_exited: 'Движок закрылся при запуске. Перезапустите его; если повторится — сообщите о проблеме с журналом из Настроек → Локальный движок.',
  engine_failed: 'Движок не запустился. Перезапустите его; если повторится — сообщите о проблеме.',
  yue2_model_set_missing: 'Выбранное качество YuE2 не скачано. Скачайте его выше.',
  yue2_hip_missing: 'Сборка YuE2 для видеокарт AMD не установлена. Установите её в Настройках → Локальный движок.',
  yue2_runtime_missing: 'Библиотеки движка для этой видеокарты ещё не скачаны. Перезапустите движок в Настройках → Локальный движок — студия скачает их сама.',
  ace_files_missing: 'ACE-Step или файлы выбранного качества не скачаны. Скачайте их выше.',
  minimax_files_missing: 'MiniMax Music3 или файлы выбранного качества не скачаны. Скачайте их выше.',
  model_files_missing: 'Эта модель не скачана. Скачайте её выше.',
  signed_package_gone: 'Эта модель из старого пакета, который студия больше не использует. Выберите другую модель.',
  openrouter_model_missing: 'Музыкальная модель OpenRouter не выбрана. Выберите её выше.',
  openrouter_key_missing: 'Ключ OpenRouter не задан. Добавьте его в Настройках → Провайдеры.',
  unknown_model: 'Студия больше не знает эту модель. Выберите другую.',
};

const ZH: Texts = {
  engine_no_answer: '引擎已启动，但没有及时响应。请重启引擎；如果再次出现，日志在“设置 → 本地引擎”。',
  cuda_backend_missing: '工作室文件夹中缺少引擎 CUDA 版本的文件。请重新安装工作室。',
  runtime_libraries_failed: '无法下载引擎所需的库。请检查网络连接并重启引擎。',
  device_missing: '引擎没有找到可用的显卡。请更新显卡驱动，或在“设置 → 本地引擎”中选择处理器。',
  cuda_unsupported: '这张显卡或其驱动无法运行任何 CUDA 版本的引擎。请更新驱动，或在“设置 → 本地引擎”中选择 Vulkan。',
  engine_port_busy: '引擎端口被其他程序占用。请关闭该程序或重启电脑后重试。',
  engine_runtime_missing: '工作室文件夹中缺少引擎。请重新安装工作室。',
  model_set_not_selected: '尚未选择模型组合。请在上方选择质量并下载。',
  chosen_card_missing: '设置中选择的显卡不在这台电脑上。请在“设置 → 本地引擎”中选择另一张。',
  engine_exited: '引擎在启动时关闭了。请重启引擎；如果再次出现，请附上“设置 → 本地引擎”中的日志报告问题。',
  engine_failed: '引擎没有启动。请重启引擎；如果再次出现，请报告问题。',
  yue2_model_set_missing: '所选的 YuE2 质量尚未下载。请在上方下载。',
  yue2_hip_missing: '尚未安装适用于 AMD 显卡的 YuE2 版本。请在“设置 → 本地引擎”中安装。',
  yue2_runtime_missing: '尚未下载这张显卡所需的引擎库。请在“设置 → 本地引擎”中重启引擎，工作室会自动下载。',
  ace_files_missing: 'ACE-Step 或所选质量的文件尚未下载。请在上方下载。',
  minimax_files_missing: 'MiniMax Music3 或所选质量的文件尚未下载。请在上方下载。',
  model_files_missing: '此模型尚未下载。请在上方下载。',
  signed_package_gone: '此模型来自工作室已不再使用的旧软件包。请选择其他模型。',
  openrouter_model_missing: '尚未选择 OpenRouter 音乐模型。请在上方选择。',
  openrouter_key_missing: '尚未设置 OpenRouter 密钥。请在“设置 → 提供方”中添加。',
  unknown_model: '工作室已不再识别此模型。请选择其他模型。',
};

const JA: Texts = {
  engine_no_answer: 'エンジンは起動しましたが、時間内に応答しませんでした。再起動してください。再発する場合、ログは「設定 → ローカルエンジン」にあります。',
  cuda_backend_missing: 'スタジオのフォルダーにエンジンの CUDA 版のファイルがありません。スタジオを再インストールしてください。',
  runtime_libraries_failed: 'エンジンのライブラリをダウンロードできませんでした。接続を確認してエンジンを再起動してください。',
  device_missing: 'エンジンが使えるグラフィックカードが見つかりません。ドライバーを更新するか、「設定 → ローカルエンジン」でプロセッサーを選んでください。',
  cuda_unsupported: 'このグラフィックカードまたはドライバーでは、どの CUDA 版のエンジンも動きません。ドライバーを更新するか、「設定 → ローカルエンジン」で Vulkan を選んでください。',
  engine_port_busy: 'エンジンのポートを別のプログラムが使っています。そのプログラムを閉じるかコンピューターを再起動して、もう一度試してください。',
  engine_runtime_missing: 'スタジオのフォルダーにエンジンがありません。スタジオを再インストールしてください。',
  model_set_not_selected: 'モデルセットが選ばれていません。上で品質を選んでダウンロードしてください。',
  chosen_card_missing: '設定で選んだグラフィックカードがこのコンピューターにありません。「設定 → ローカルエンジン」で別のカードを選んでください。',
  engine_exited: '起動中にエンジンが終了しました。再起動してください。再発する場合は「設定 → ローカルエンジン」のログを添えて問題を報告してください。',
  engine_failed: 'エンジンが起動しませんでした。再起動してください。再発する場合は問題を報告してください。',
  yue2_model_set_missing: '選んだ YuE2 の品質はダウンロードされていません。上でダウンロードしてください。',
  yue2_hip_missing: 'AMD カード用の YuE2 がインストールされていません。「設定 → ローカルエンジン」でインストールしてください。',
  yue2_runtime_missing: 'このグラフィックカード用のエンジンのライブラリはまだダウンロードされていません。「設定 → ローカルエンジン」でエンジンを再起動すると、スタジオが自動でダウンロードします。',
  ace_files_missing: 'ACE-Step または選んだ品質のファイルがダウンロードされていません。上でダウンロードしてください。',
  minimax_files_missing: 'MiniMax Music3 または選んだ品質のファイルがダウンロードされていません。上でダウンロードしてください。',
  model_files_missing: 'このモデルはダウンロードされていません。上でダウンロードしてください。',
  signed_package_gone: 'このモデルはスタジオがもう使わない古いパッケージのものです。別のモデルを選んでください。',
  openrouter_model_missing: 'OpenRouter の音楽モデルが選ばれていません。上で選んでください。',
  openrouter_key_missing: 'OpenRouter のキーが設定されていません。「設定 → プロバイダー」で追加してください。',
  unknown_model: 'スタジオはこのモデルをもう認識していません。別のモデルを選んでください。',
};

const KO: Texts = {
  engine_no_answer: '엔진이 시작됐지만 제때 응답하지 않았습니다. 다시 시작하세요. 또 그러면 로그는 "설정 → 로컬 엔진"에 있습니다.',
  cuda_backend_missing: '스튜디오 폴더에 엔진 CUDA 빌드의 파일이 없습니다. 스튜디오를 다시 설치하세요.',
  runtime_libraries_failed: '엔진 라이브러리를 내려받지 못했습니다. 연결을 확인하고 엔진을 다시 시작하세요.',
  device_missing: '엔진이 쓸 수 있는 그래픽카드를 찾지 못했습니다. 그래픽 드라이버를 업데이트하거나 "설정 → 로컬 엔진"에서 프로세서를 고르세요.',
  cuda_unsupported: '이 그래픽카드나 드라이버로는 어떤 CUDA 빌드의 엔진도 실행되지 않습니다. 드라이버를 업데이트하거나 "설정 → 로컬 엔진"에서 Vulkan을 고르세요.',
  engine_port_busy: '다른 프로그램이 엔진 포트를 쓰고 있습니다. 그 프로그램을 닫거나 컴퓨터를 다시 시작한 뒤 다시 시도하세요.',
  engine_runtime_missing: '스튜디오 폴더에 엔진이 없습니다. 스튜디오를 다시 설치하세요.',
  model_set_not_selected: '모델 세트가 선택되지 않았습니다. 위에서 품질을 고르고 내려받으세요.',
  chosen_card_missing: '설정에서 고른 그래픽카드가 이 컴퓨터에 없습니다. "설정 → 로컬 엔진"에서 다른 카드를 고르세요.',
  engine_exited: '엔진이 시작하는 중에 종료됐습니다. 다시 시작하세요. 또 그러면 "설정 → 로컬 엔진"의 로그와 함께 문제를 보고하세요.',
  engine_failed: '엔진이 시작되지 않았습니다. 다시 시작하세요. 또 그러면 문제를 보고하세요.',
  yue2_model_set_missing: '선택한 YuE2 품질이 내려받아지지 않았습니다. 위에서 내려받으세요.',
  yue2_hip_missing: 'AMD 카드용 YuE2 빌드가 설치되지 않았습니다. "설정 → 로컬 엔진"에서 설치하세요.',
  yue2_runtime_missing: '이 그래픽카드용 엔진 라이브러리를 아직 내려받지 않았습니다. "설정 → 로컬 엔진"에서 엔진을 다시 시작하면 스튜디오가 직접 내려받습니다.',
  ace_files_missing: 'ACE-Step이나 선택한 품질의 파일이 내려받아지지 않았습니다. 위에서 내려받으세요.',
  minimax_files_missing: 'MiniMax Music3이나 선택한 품질의 파일이 내려받아지지 않았습니다. 위에서 내려받으세요.',
  model_files_missing: '이 모델이 내려받아지지 않았습니다. 위에서 내려받으세요.',
  signed_package_gone: '이 모델은 스튜디오가 더 이상 쓰지 않는 예전 패키지에서 왔습니다. 다른 모델을 고르세요.',
  openrouter_model_missing: 'OpenRouter 음악 모델이 선택되지 않았습니다. 위에서 고르세요.',
  openrouter_key_missing: 'OpenRouter 키가 설정되지 않았습니다. "설정 → 제공자"에서 추가하세요.',
  unknown_model: '스튜디오가 더 이상 이 모델을 알지 못합니다. 다른 모델을 고르세요.',
};

const PT: Texts = {
  engine_no_answer: 'O motor arrancou mas não respondeu a tempo. Reinicie-o; se voltar a acontecer, o registo está em Definições → Motor local.',
  cuda_backend_missing: 'Falta um ficheiro da versão CUDA do motor na pasta do estúdio. Reinstale o estúdio.',
  runtime_libraries_failed: 'Não foi possível transferir as bibliotecas do motor. Verifique a ligação e reinicie o motor.',
  device_missing: 'O motor não encontrou uma placa gráfica em que possa correr. Atualize o controlador ou escolha o processador em Definições → Motor local.',
  cuda_unsupported: 'Esta placa gráfica ou o seu controlador não corre nenhuma versão CUDA do motor. Atualize o controlador ou escolha Vulkan em Definições → Motor local.',
  engine_port_busy: 'Outro programa está a usar a porta do motor. Feche-o ou reinicie o computador e tente novamente.',
  engine_runtime_missing: 'O motor não está na pasta do estúdio. Reinstale o estúdio.',
  model_set_not_selected: 'Nenhum conjunto de modelos escolhido. Escolha a qualidade acima e transfira-a.',
  chosen_card_missing: 'A placa gráfica escolhida nas definições não está neste computador. Escolha outra em Definições → Motor local.',
  engine_exited: 'O motor fechou ao arrancar. Reinicie-o; se voltar a acontecer, relate o problema com o registo de Definições → Motor local.',
  engine_failed: 'O motor não arrancou. Reinicie-o; se voltar a acontecer, relate o problema.',
  yue2_model_set_missing: 'A qualidade escolhida do YuE2 não foi transferida. Transfira-a acima.',
  yue2_hip_missing: 'A versão do YuE2 para placas AMD não está instalada. Instale-a em Definições → Motor local.',
  yue2_runtime_missing: 'As bibliotecas do motor para esta placa gráfica ainda não foram transferidas. Reinicie o motor em Definições → Motor local e o estúdio transfere-as.',
  ace_files_missing: 'O ACE-Step ou os ficheiros da qualidade escolhida não foram transferidos. Transfira-os acima.',
  minimax_files_missing: 'O MiniMax Music3 ou os ficheiros da qualidade escolhida não foram transferidos. Transfira-os acima.',
  model_files_missing: 'Este modelo não foi transferido. Transfira-o acima.',
  signed_package_gone: 'Este modelo vem de um pacote antigo que o estúdio já não usa. Escolha outro modelo.',
  openrouter_model_missing: 'Nenhum modelo de música do OpenRouter escolhido. Escolha um acima.',
  openrouter_key_missing: 'A chave do OpenRouter não está definida. Adicione-a em Definições → Fornecedores.',
  unknown_model: 'O estúdio já não reconhece este modelo. Escolha outro.',
};

const BY_LANGUAGE: Record<Language, Texts> = { en: EN, ru: RU, zh: ZH, ja: JA, ko: KO, pt: PT };

/** The words for an engine or model code in `language`; null for a code this window does not know. */
export function engineErrorText(code: string | null | undefined, language: Language): string | null {
  if (!code) return null;
  return BY_LANGUAGE[language]?.[code] ?? EN[code] ?? null;
}
