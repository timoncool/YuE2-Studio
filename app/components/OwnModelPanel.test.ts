import { describe, expect, it, vi } from 'vitest';
import type { OwnModelStatus } from './OwnModelPanel';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ t: (key: string) => key, language: 'en' }) }));

const { choiceOf } = await import('./OwnModelPanel');

const status = (selected: string | null): OwnModelStatus => ({
  folder: '/models/yue2-cpp/own',
  files: [{ name: 'mine.gguf', bytes: 1 }, { name: 'YuE2-3B-RS-TW-2.0x0.75-Q8_0.gguf', bytes: 2 }],
  selected,
  missing: false,
  community: [{
    id: 'rs-tw-2.0x0.75',
    name: { en: 'Ruach Studio TW 2.0×0.75' },
    author: 'Ruach Studio',
    page: 'https://huggingface.co/goldhub/YuE2-RS-Finetunes',
    license: 'CC BY-NC-SA 4.0',
    files: [
      { quant: 'Q8_0', file: 'YuE2-3B-RS-TW-2.0x0.75-Q8_0.gguf', bytes: 2, installed: true },
      { quant: 'Q6_K', file: 'YuE2-3B-RS-TW-2.0x0.75-Q6_K.gguf', bytes: 1, installed: false },
    ],
  }],
});

describe('choiceOf', () => {
  it('shows a downloaded community file as its catalog entry, an own file by name, nothing as Do not use', () => {
    expect(choiceOf(status('YuE2-3B-RS-TW-2.0x0.75-Q8_0.gguf'))).toBe('community:rs-tw-2.0x0.75:Q8_0');
    expect(choiceOf(status('mine.gguf'))).toBe('mine.gguf');
    expect(choiceOf(status(null))).toBe('');
    expect(choiceOf(null)).toBe('');
  });
});
