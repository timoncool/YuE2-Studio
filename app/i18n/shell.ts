/** Strings of what the desktop shell asks through the window: quitting while a song is made, an update. Engine-neutral. */

const en = {
  shellQuitTitle: 'Quit the studio?',
  shellQuitMessage: 'A song is being made. Quitting stops it.',
  shellQuitConfirm: 'Quit',
  shellUpdateTitle: 'An update is out',
  shellUpdateMessage: 'Version {version} is ready. Install it now? The studio restarts into the new version.',
  shellUpdatePortable: 'Version {version} is ready. A portable copy is updated by hand: open the download page?',
  shellUpdateInstall: 'Install',
  shellUpdateOpen: 'Open the page',
  shellUpdateLater: 'Later',
  shellUpdateInstalling: 'Downloading and installing…',
  shellUpdateFailed: 'The update did not install: {error}',
  shellCloseToTray: 'Keep running in the system tray when the window is closed',
  shellCloseToTrayHint: 'Closing the window hides it; songs keep being made. Click the tray icon to bring it back, and quit from its menu.',
};

type Strings = typeof en;

const ru: Strings = {
  shellQuitTitle: 'Выйти из студии?',
  shellQuitMessage: 'Сейчас создаётся песня. Выход её остановит.',
  shellQuitConfirm: 'Выйти',
  shellUpdateTitle: 'Вышло обновление',
  shellUpdateMessage: 'Готова версия {version}. Установить сейчас? Студия перезапустится уже новой.',
  shellUpdatePortable: 'Готова версия {version}. Портативная копия обновляется вручную: открыть страницу загрузки?',
  shellUpdateInstall: 'Установить',
  shellUpdateOpen: 'Открыть страницу',
  shellUpdateLater: 'Позже',
  shellUpdateInstalling: 'Скачиваю и устанавливаю…',
  shellUpdateFailed: 'Обновление не установилось: {error}',
  shellCloseToTray: 'Продолжать работу в системном трее при закрытии окна',
  shellCloseToTrayHint: 'Закрытие окна только прячет его, песни продолжают создаваться. Нажмите на значок в трее, чтобы вернуть окно, а выйти можно из его меню.',
};

const zh: Strings = {
  shellQuitTitle: '退出工作室？',
  shellQuitMessage: '正在生成歌曲。退出会停止生成。',
  shellQuitConfirm: '退出',
  shellUpdateTitle: '有新版本',
  shellUpdateMessage: '版本 {version} 已就绪。现在安装吗？工作室将以新版本重新启动。',
  shellUpdatePortable: '版本 {version} 已就绪。便携版需要手动更新：打开下载页面吗？',
  shellUpdateInstall: '安装',
  shellUpdateOpen: '打开页面',
  shellUpdateLater: '稍后',
  shellUpdateInstalling: '正在下载并安装…',
  shellUpdateFailed: '更新未能安装：{error}',
  shellCloseToTray: '关闭窗口时继续在系统托盘中运行',
  shellCloseToTrayHint: '关闭窗口只会将其隐藏，歌曲会继续生成。点击托盘图标即可恢复窗口，并可从其菜单退出。',
};

const ja: Strings = {
  shellQuitTitle: 'スタジオを終了しますか？',
  shellQuitMessage: '曲を生成中です。終了すると生成は止まります。',
  shellQuitConfirm: '終了',
  shellUpdateTitle: 'アップデートがあります',
  shellUpdateMessage: 'バージョン {version} の準備ができました。今すぐインストールしますか？スタジオは新しいバージョンで再起動します。',
  shellUpdatePortable: 'バージョン {version} の準備ができました。ポータブル版は手動で更新します。ダウンロードページを開きますか？',
  shellUpdateInstall: 'インストール',
  shellUpdateOpen: 'ページを開く',
  shellUpdateLater: '後で',
  shellUpdateInstalling: 'ダウンロードしてインストールしています…',
  shellUpdateFailed: 'アップデートをインストールできませんでした：{error}',
  shellCloseToTray: 'ウィンドウを閉じてもシステムトレイで動作し続ける',
  shellCloseToTrayHint: 'ウィンドウを閉じると非表示になるだけで、曲の生成は続きます。トレイのアイコンをクリックすると戻り、終了はそのメニューから行います。',
};

const ko: Strings = {
  shellQuitTitle: '스튜디오를 종료할까요?',
  shellQuitMessage: '노래를 만드는 중입니다. 종료하면 생성이 멈춥니다.',
  shellQuitConfirm: '종료',
  shellUpdateTitle: '업데이트가 있습니다',
  shellUpdateMessage: '{version} 버전이 준비되었습니다. 지금 설치할까요? 스튜디오가 새 버전으로 다시 시작됩니다.',
  shellUpdatePortable: '{version} 버전이 준비되었습니다. 포터블 버전은 직접 업데이트합니다. 다운로드 페이지를 열까요?',
  shellUpdateInstall: '설치',
  shellUpdateOpen: '페이지 열기',
  shellUpdateLater: '나중에',
  shellUpdateInstalling: '다운로드하고 설치하는 중…',
  shellUpdateFailed: '업데이트를 설치하지 못했습니다: {error}',
  shellCloseToTray: '창을 닫아도 시스템 트레이에서 계속 실행',
  shellCloseToTrayHint: '창을 닫으면 숨겨질 뿐 노래 생성은 계속됩니다. 트레이 아이콘을 클릭하면 다시 열리고, 종료는 트레이 메뉴에서 합니다.',
};

export const shellStrings = { en, ru, zh, ja, ko };
