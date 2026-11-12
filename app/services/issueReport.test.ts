import fs from 'fs';
import path from 'path';
import { describe, expect, it } from 'vitest';
import { bugTemplate, issueReportUrl, issueTemplate } from './issueReport';

const FORMS = path.resolve(__dirname, '../../.github/ISSUE_TEMPLATE');
const LANGUAGES = [['en', '1'], ['ru', '2'], ['zh', '3'], ['ja', '4'], ['ko', '5']] as const;
const read = (file: string) => fs.readFileSync(path.join(FORMS, file), 'utf-8');
const ids = (form: string) => [...form.matchAll(/^\s+id: (\S+)$/gm)].map(match => match[1]);
const options = (form: string) => [...form.matchAll(/^\s+options:\n((?:\s{8}- .+\n)+)/gm)].map(match => match[1]);
const models = (url: string) => new URL(url).searchParams.get('models');
const base = 'https://github.com/timoncool/YuE2-Studio';

describe('the report link', () => {
  it('opens the bug form of the window language', () => {
    for (const [language, order] of LANGUAGES) {
      const file = `${order}-${language}-1-bug.yml`;
      expect(bugTemplate(language)).toBe(file);
      expect(fs.existsSync(path.join(FORMS, file)), file).toBe(true);
      const url = new URL(issueReportUrl(base, '4.0.0', language, { selected_profile_id: 'quality-q8' }));
      expect(url.pathname).toBe('/timoncool/YuE2-Studio/issues/new');
      expect(url.searchParams.get('template')).toBe(file);
      expect(url.searchParams.get('version')).toBe('4.0.0');
      expect(url.searchParams.get('models')).toBe('quality-q8');
    }
    expect(bugTemplate('de')).toBe('1-en-1-bug.yml');
    for (const [language, order] of LANGUAGES) {
      for (const [kind, file] of [['idea', '2-idea'], ['question', '3-question']] as const) {
        expect(issueTemplate(language, kind)).toBe(`${order}-${language}-${file}.yml`);
        expect(fs.existsSync(path.join(FORMS, issueTemplate(language, kind)))).toBe(true);
      }
    }
  });

  it('finds the same fields, choices and labels in all five languages of a form, English first in the list', () => {
    for (const kind of ['1-bug', '2-idea', '3-question']) {
      const english = read(`1-en-${kind}.yml`);
      for (const [language, order] of LANGUAGES.slice(1)) {
        const form = read(`${order}-${language}-${kind}.yml`);
        expect(ids(form), `${language}-${kind}`).toEqual(ids(english));
        expect(options(form), `${language}-${kind}`).toEqual(options(english));
        expect(form.match(/^labels: .*$/m)?.[0]).toBe(english.match(/^labels: .*$/m)?.[0]);
      }
    }
    const files = fs.readdirSync(FORMS).filter(file => file !== 'config.yml').sort();
    expect(files.length).toBe(15);
    expect(files.slice(0, 3).every(file => file.startsWith('1-en-'))).toBe(true);
    const names = files.map(file => read(file).match(/^name: (.+)$/m)?.[1]);
    expect(new Set(names).size, 'every form has a name of its own').toBe(names.length);
  });

  it('names the model set, and the own model when one sings in its place', () => {
    expect(models(issueReportUrl(base, '4.0.0', 'en', { selected_profile_id: 'quality-q8', own_model: 'YuE2-3B-RS-TW-2.0x0.75-Q8_0.gguf' })))
      .toBe('quality-q8, own model YuE2-3B-RS-TW-2.0x0.75-Q8_0.gguf');
    expect(models(issueReportUrl(base, '4.0.0', 'ru', { selected_profile_id: 'quality-q8', own_model: 'YuE2-3B-RS-TW-2.0x0.75-Q8_0.gguf', own_model_name: { en: 'Ruach Studio TW 2.0×0.75' } })))
      .toBe('quality-q8, own model Ruach Studio TW 2.0×0.75 (YuE2-3B-RS-TW-2.0x0.75-Q8_0.gguf)');
    expect(models(issueReportUrl(base, '4.0.0', 'en', {}))).toBeNull();
  });
});
