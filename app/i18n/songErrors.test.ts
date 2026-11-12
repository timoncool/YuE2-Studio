import { describe, expect, it } from 'vitest';
import { songErrorText } from './songErrors';

describe('songErrorText', () => {
  it('fills the fields into the template of the window language', () => {
    const failure = { message: 'English text', code: 'context_short', params: { tokens: 9700, context: 24576, can_last: 595, asked: 600 } };
    const ru = songErrorText(failure, 'ru');
    expect(ru).toContain('9700 из 24576');
    expect(ru).toContain('длиться 595 с');
    expect(ru).toContain('запрошено 600 с');
    expect(songErrorText(failure, 'ja')).toContain('595 秒');
  });

  it('shows the service text as it is without a code or a template', () => {
    expect(songErrorText({ message: 'raw words' }, 'ru')).toBe('raw words');
    expect(songErrorText({ message: 'raw words', code: 'no_such_code' }, 'ko')).toBe('raw words');
    expect(songErrorText(null, 'en')).toBe('');
  });

  it('names the Composition strength with one decimal in the window language', () => {
    const failure = (strength: number) => ({ message: 'x', code: 'score_unsingable_strength', params: { song: 1, problem: 'x', strength } });
    expect(songErrorText(failure(3), 'ru')).toBe('Партитура не написалась: сила LoRA на «Композиции» — 3,0. Попробуйте 1,0.');
    expect(songErrorText(failure(1.5), 'en')).toBe('The score could not be written: the LoRA strength on Composition is 1.5. Try 1.0.');
  });

  it('keeps a placeholder whose field did not come', () => {
    expect(songErrorText({ message: 'x', code: 'engine_fatal', params: {} }, 'en')).toBe('The engine stopped this song: {line}');
  });
});
