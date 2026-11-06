// What the project page says about this studio beyond its text: the release it offers, the features
// its tiles show (places in each language's features list, with the screenshot each shows), and the answers it points to.
export const STUDIO = {
  id: 'yue2',
  name: 'YuE2 Studio',
  repo: 'https://github.com/timoncool/YuE2-Studio',
  site: 'https://timoncool.github.io/YuE2-Studio/',
  installerMB: 301,
  updated: '2026-11-05',
  bento: [
    { feature: 1, shot: '02-score' },
    { feature: 3, shot: '04-cover' },
    { feature: 14, shot: '09-midi' },
    { feature: 10, shot: '08-training' },
    { feature: 19, shot: '15-covers' },
    { feature: 13, shot: '11-agent' },
  ],
  faq: { lora: 10, mcp: 13 },
  arch: "React UI ─┐\n          ├─ YuE2-Studio.exe   (Tauri window + Rust/Axum service)\nRust axum ┘        │\n                   └─ yue2.cpp `yue-server`   (C++/CUDA, GGUF)",
  // Community video reviews whose description links this repository. The first `reviewsFeatured`
  // are shown; the rest open under "more". Order is the ranking; view counts are not stored (they go stale).
  reviewsFeatured: 6,
  reviews: [
    { id: '6OnV_80f6ko', by: 'AI Controversy', lang: 'EN' },
    { id: 'JrNTvpoh318', by: 'ArtSonica', lang: 'ID' },
    { id: 'XxkGZU89FII', by: 'AI VINCI', lang: 'EN' },
    { id: 'kpaV_1jyO3E', by: 'Mamedov Info', lang: 'RU' },
    { id: 'JPUU9zB7yio', by: 'ЮЛАВИ Studio', lang: 'RU' },
    { id: '-TjHGgxKwBU', by: 'Wizzdome-Music', lang: 'EN' },
    { id: 'GmrstqfzC9k', by: 'Dream Travel', lang: 'RU' },
    { id: 'c5kwPNpkAO8', by: 'Luki AI Lab', lang: 'EN' },
    { id: '8AIaqTRrdPc', by: 'Skillo Labs', lang: 'PT' },
    { id: '08yKZTgMK_k', by: '아빠너구리 TV', lang: 'KO' },
    { id: 'dBetNSxbo-A', by: 'Frihandhika', lang: 'ID' },
    { id: 'BtlPRhbLNoM', by: 'Run It Local', lang: 'EN' },
    { id: 'Br6ABTJflPg', by: 'Franck D Digital AI', lang: 'FR' },
    { id: 'EFv_zOXyn5E', by: 'EZTV', lang: 'ZH' },
  ],
};
