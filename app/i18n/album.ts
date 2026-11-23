import type { Language } from './translations';

/** What an album page says where a playlist page says playlist. */
export const albumPageStrings: Record<Language, { emptyTitle: string; emptyHint: string; addTitle: string }> = {
  en: { emptyTitle: 'This album is empty', emptyHint: 'Find songs below and add them, or choose “Add to album” on any song.', addTitle: 'Add songs to the album' },
  ru: { emptyTitle: 'В альбоме пока пусто', emptyHint: 'Найдите песни ниже и добавьте их или выберите «Добавить в альбом» у любой песни.', addTitle: 'Добавить песни в альбом' },
  zh: { emptyTitle: '这个专辑还是空的', emptyHint: '在下方找到歌曲并添加，或在任意歌曲上选择“添加到专辑”。', addTitle: '向专辑添加歌曲' },
  ja: { emptyTitle: 'このアルバムはまだ空です', emptyHint: '下で曲を探して追加するか、曲の「アルバムに追加」を選んでください。', addTitle: 'アルバムに曲を追加' },
  ko: { emptyTitle: '이 앨범은 아직 비어 있습니다', emptyHint: '아래에서 곡을 찾아 추가하거나 곡의 「앨범에 추가」를 선택하세요.', addTitle: '앨범에 곡 추가' },
  pt: { emptyTitle: 'Este álbum está vazio', emptyHint: 'Encontre músicas abaixo e adicione-as, ou escolha “Adicionar ao álbum” em qualquer música.', addTitle: 'Adicionar músicas ao álbum' },
};
