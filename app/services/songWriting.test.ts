import { describe, expect, it } from 'vitest';
import { cycleSection, editLyricLine, lyricSections, sectionText, setStyleRole, styleLint, styleRole, toggleLetterCase, writerInstruction } from './songWriting';
import { STYLE_GLOSSES, STYLE_SUGGESTIONS } from './styleSuggestions';
import { styleAdvice } from '../i18n/styleAdvice';

describe('YuE2 style advice', () => {
  it('accepts every offered catalogue phrase including all translated suggestion keys', () => {
    const phrases = new Set([...STYLE_SUGGESTIONS, ...Object.values(STYLE_GLOSSES).flatMap(gloss => [...gloss.keys()])]);
    for (const phrase of phrases) expect(styleLint(phrase), phrase).toEqual([]);
  });

  it('keeps decades, meters, 808, musical texture and a complete descriptor prompt intact', () => {
    const style = "English, 90s synth-pop, airy female vocals, 808 bass with warm piano and guitar in 3/4, 120 BPM";
    expect(styleLint(style)).toEqual([]);
  });

  it('reports twelve localised suggestions without changing the text', () => {
    const text = 'rock, pop, jazz, no drums, make it massive, in the style of X, 90 Hz, vocals\n[Chorus]';
    const keys = new Set([
      ...styleLint(text, styleAdvice.ru),
      ...styleLint('massive'),
      ...styleLint('piano, guitar, violin, cello, viola, banjo, harp'),
    ].map(hint => hint.rule));
    expect(keys.size).toBe(12);
    expect(styleLint(text, styleAdvice.ru)).toContainEqual({rule:'negative', level:'advice', hint:styleAdvice.ru.negative});
    expect(styleLint('  ')).toEqual([]);
  });
});

describe('structured lyric editing', () => {
  it('preserves repeated choruses, custom tags and untagged opening words', () => {
    const text = 'Opening words\n\n[Verse 1]\nПервая строка\nВторая строка\n\n[Chorus]\nSing again\n\n[Custom Part]\nOther words\n\n[Chorus]\nSing again';
    expect(sectionText(lyricSections(text))).toBe(text);
  });
  it('preserves a newline typed at the end of a section, including before another section', () => {
    const sections = [{ tag: 'Verse', body: '\nOne\n\nTwo\n' }, { tag: 'Chorus', body: 'Three\n' }];
    expect(lyricSections(sectionText(sections))).toEqual(sections);
  });
  it('recognises only whole-line tags and preserves inline brackets as sung words', () => {
    expect(lyricSections('[Verse]\r\nKeep [these words] here')).toEqual([{ tag: 'Verse', body: 'Keep [these words] here' }]);
  });
  it('keeps spaces being typed in a section name', () => {
    expect(lyricSections(sectionText([{ tag: 'Verse ', body: 'One' }]))[0].tag).toBe('Verse ');
  });
});
describe('style and lyric operations', () => {
  it('updates a named style role while preserving custom descriptors and voices', () => {
    const original = 'Russian, folk rock, deep male voice and soft female voice, banjo, 88 BPM';
    expect(setStyleRole(original, 'language', 'Japanese')).toBe('Japanese, folk rock, deep male voice and soft female voice, banjo, 88 BPM');
    expect(setStyleRole(original, 'tempo', '250')).toBe('Russian, folk rock, deep male voice and soft female voice, banjo, 200 BPM');
    expect(setStyleRole(original, 'tempo', '')).toBe('Russian, folk rock, deep male voice and soft female voice, banjo');
    expect(styleRole(original, 'voice')).toBe('deep male voice and soft female voice');
  });
  it('replaces the old language inside vocal descriptors without losing mixed voices or unknown sound parts', () => {
    const original = 'Russian vocals, indie pop, deep male voice and soft female voice, banjo, moonlit texture';
    const english = setStyleRole(original, 'language', 'English');
    expect(styleRole(original, 'language')).toBe('Russian');
    expect(english).toBe('English vocals, indie pop, deep male voice and soft female voice, banjo, moonlit texture');
    expect(setStyleRole(english, 'language', 'Russian')).toBe(original);
    expect(setStyleRole('Jazz ballad, intimate Mandarin male vocal, piano', 'language', 'Japanese')).toBe('Jazz ballad, intimate Japanese male vocal, piano');
    expect(setStyleRole('Russian, warm Russian male voice, banjo', 'language', 'English')).toBe('English, warm English male voice, banjo');
  });
  it('cycles known tags and preserves the verse number', () => {
    expect(cycleSection('Verse 2')).toBe('Pre-Chorus 2');
    expect(cycleSection('Intro')).toBe('Verse');
    expect(cycleSection('Custom')).toBe('Verse');
  });
  it('moves boundary lines between sections and preserves blank lines in stanzas', () => {
    const sections = [{ tag: 'Verse', body: 'One\nTwo' }, { tag: 'Chorus', body: 'Three' }];
    expect(editLyricLine(sections, 0, 1, 'down')).toEqual([{ tag: 'Verse', body: 'One' }, { tag: 'Chorus', body: 'Two\nThree' }]);
    expect(editLyricLine([{ tag: 'Verse', body: 'One\nTwo\n\nThree' }], 0, 1, 'duplicateStanza')[0].body).toBe('One\nTwo\n\nOne\nTwo\n\nThree');
    expect(editLyricLine([{ tag: 'Verse', body: 'One\nTwo\n\nThree' }], 0, 1, 'deleteStanza')[0].body).toBe('Three');
    expect(sections[0].body).toBe('One\nTwo');
  });
  it('changes one Unicode letter without expanding sharp-s or damaging emoji', () => {
    expect(toggleLetterCase('а😀б', 2)).toBe('а😀Б');
    expect(toggleLetterCase('а😀Б', 2)).toBe('а😀б');
    expect(toggleLetterCase('straße', 4)).toBe('straße');
  });
});

describe('writer constraints', () => {
  it('keeps the idea and explicit language, total line budget and extra instructions', () => {
    const instruction = writerInstruction(' winter ', ' Russian ', '16', ' Male voice. ');
    expect(instruction).toContain('winter');
    expect(instruction).toContain('in Russian');
    expect(instruction).toContain('exactly 16 sung lines in total, excluding section tags');
    expect(instruction).toContain('Male voice.');
  });
  it('leaves automatic language and length to the existing assistant contract', () => {
    expect(writerInstruction('My idea', '', '', '')).toBe('My idea');
  });
});
