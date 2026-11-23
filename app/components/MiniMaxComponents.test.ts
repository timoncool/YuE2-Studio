import { expect, it } from 'vitest';
import { completeComponents } from './MiniMaxComponents';

it('sends the components only when all five roles are chosen, as the engine takes them', () => {
  const all = { lm: 'a.gguf', depth: 'b.gguf', cond: 'c.gguf', dit: 'd.gguf', vae: 'e.gguf' };
  expect(completeComponents(all)).toEqual(all);
  expect(completeComponents({ lm: 'a.gguf', dit: 'd.gguf' })).toBeUndefined();
  expect(completeComponents({})).toBeUndefined();
});
