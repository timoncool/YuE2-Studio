// What the project page says about this studio beyond its text: the release it offers, the features
// its tiles show (places in each language's features list, with the screenshot each shows), and the answers it points to.
export const STUDIO = {
  id: 'yue2',
  name: 'YuE2 Studio',
  repo: 'https://github.com/timoncool/YuE2-Studio',
  site: 'https://timoncool.github.io/YuE2-Studio/',
  version: '3.5.2',
  installerMB: 318.24,
  updated: '2026-10-10',
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
};
