import { describe, expect, it } from 'vitest';
import { yue2 } from '../i18n/yue2';
import { keepLoadedNow } from './EngineSettings';

const t = (key: string) => (yue2.en as Record<string, string>)[key] ?? key;

describe('keepLoadedNow', () => {
  it('says whether the running engine keeps its models and why, in numbers when Auto weighed them', () => {
    expect(keepLoadedNow({ keep_loaded: true, keep_loaded_reason: 'fits', keep_loaded_need_gb: 10.2, keep_loaded_free_gb: 22 }, t))
      .toBe('Now: kept loaded - 10.2 GB needed, 22.0 GB free');
    expect(keepLoadedNow({ keep_loaded: false, keep_loaded_reason: 'does_not_fit', keep_loaded_need_gb: 10.2, keep_loaded_free_gb: 7 }, t))
      .toBe('Now: loaded for each song - 10.2 GB needed, only 7.0 GB free');
    expect(keepLoadedNow({ keep_loaded: false, keep_loaded_reason: 'not_on_card' }, t)).toContain('NVIDIA');
    expect(keepLoadedNow({ keep_loaded: false, keep_loaded_reason: 'unmeasured' }, t)).toContain('not measured');
    expect(keepLoadedNow({ keep_loaded: true, keep_loaded_reason: 'set' }, t)).toBe('Now: kept loaded - as chosen');
  });
});
