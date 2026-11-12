import { describe, expect, it } from 'vitest';
import { issueReportUrl } from './issueReport';

const models = (url: string) => new URL(url).searchParams.get('models');

describe('issueReportUrl', () => {
  it('names the model set, and the own model when one sings in its place', () => {
    const base = 'https://github.com/timoncool/YuE2-Studio';
    expect(models(issueReportUrl(base, '3.9.0', { selected_profile_id: 'quality-q8' }))).toBe('quality-q8');
    expect(models(issueReportUrl(base, '3.9.0', { selected_profile_id: 'quality-q8', own_model: 'YuE2-3B-RS-TW-2.0x0.75-Q8_0.gguf' })))
      .toBe('quality-q8, own model YuE2-3B-RS-TW-2.0x0.75-Q8_0.gguf');
    expect(models(issueReportUrl(base, '3.9.0', {}))).toBeNull();
  });
});
