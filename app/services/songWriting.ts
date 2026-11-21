import { STYLE_SUGGESTIONS } from './styleSuggestions';
import { styleAdvice } from '../i18n/styleAdvice';

const catalogueDescriptors = new Set(STYLE_SUGGESTIONS.map(s => s.toLowerCase()));
const instruments = /\b(guitars?|piano|bass|drums?|violins?|cello|viola|saxophone|trumpet|trombone|flute|clarinet|harp|banjo|mandolin|synth(?:esizer)?s?|accordion|timpani|marimba|organ)\b/gi;
const genreTag = /^(rock|pop|jazz|metal|classical|country|reggae|disco|folk|punk|blues|techno|house|rap)$/i;
export type StyleRule = 'language' | 'tempo' | 'length' | 'numbers' | 'negative' | 'instruction' | 'artist'
  | 'genres' | 'quality' | 'instruments' | 'voice' | 'lyrics';

/** Optional advice for YuE2's English style descriptors. A catalogue fragment is valid on its own. */
export function styleLint(text: string, hints: Record<StyleRule, string> = styleAdvice.en) {
  const tags = text.trim().split(/[,;\n]+/).map(s => s.trim().toLowerCase()).filter(Boolean);
  if (!tags.length || tags.every(tag => catalogueDescriptors.has(tag))) return [];
  const rules: StyleRule[] = [];
  const words = text.match(/[\p{L}\p{N}]+(?:[-'][\p{L}\p{N}]+)*/gu) ?? [];
  const uniqueInstruments = new Set(Array.from(text.matchAll(instruments), m => m[1].toLowerCase().replace(/s$/, '')));
  if (!/^(?:instrumental|english|russian|chinese|mandarin|cantonese|japanese|korean|spanish|french|german|italian|portuguese|swedish|norwegian|danish|finnish|polish|ukrainian|arabic|hindi|turkish|dutch|greek)\b/i.test(tags[0])) rules.push('language');
  if (!/\b\d{2,3}\s+bpm\s*$/i.test(text)) rules.push('tempo');
  if (words.length < 12 || words.length > 50) rules.push('length');
  // Decade genres, instrument names and meters are meaningful musical descriptors.
  const nonTempo = text.replace(/\b(?:\d+(?:\.\d+)?\s*bpm|bpm\s*\d+|(?:19|20)?\d0['’]?s|(?:tr[- ]?)?808|tr[- ]?909|[234679]\/[248])\b/gi, '');
  if (/\b\d+\b/.test(nonTempo)) rules.push('numbers');
  if (/\b(?:no|without|utan)\s+\w+/i.test(text)) rules.push('negative');
  if (/\b(?:make (?:it|the song)|the song (?:should|must)|please (?:write|generate|create))\b/i.test(text)) rules.push('instruction');
  if (/\bin the style of\s+\S/i.test(text) || /\b[A-Z][a-z]+\s+[A-Z][a-z]+(?:'s|’s)?\s+style\b/.test(text)) rules.push('artist');
  if (new Set(tags.filter(t => genreTag.test(t))).size >= 3) rules.push('genres');
  if (!uniqueInstruments.size && /\b(?:phat|huge|massive|high[- ]quality|masterpiece)\b/i.test(text)) rules.push('quality');
  if (uniqueInstruments.size > 6) rules.push('instruments');
  if (VOICE.test(text)) {
    const register = /\b(?:male|female|alto|soprano|tenor|baritone|contralto|mezzo|androgynous|mixed)\b/i.test(text);
    const delivery = /\b(?:airy|warm|soft|clear|deep|breathy|raspy|belt(?:ing)?|whisper(?:ed)?|spoken|smooth|operatic|falsetto|powerful)\b/i.test(text);
    if (!register || !delivery) rules.push('voice');
  }
  if (/^\s*\[(?:verse|chorus|bridge|outro|intro|hook|куплет|припев)\b[^\]]*\]/im.test(text)) rules.push('lyrics');
  return rules.map(rule => ({ rule, level: 'advice' as const, hint: hints[rule] }));
}

export type LyricSection = { tag: string; body: string };
export const SECTION_PRESETS = ['Intro', 'Verse', 'Pre-Chorus', 'Chorus', 'Hook', 'Bridge', 'Refrain', 'Outro', 'Instrumental', 'Interlude'];
const SECTION_CYCLE = ['Verse', 'Pre-Chorus', 'Chorus', 'Bridge', 'Outro', 'Intro'];
export const STYLE_LANGUAGES = ['English', 'Russian', 'Chinese', 'Mandarin', 'Japanese', 'Korean', 'Spanish', 'French', 'German', 'Italian', 'Portuguese'];
const TEMPO = /^(?:(\d{2,3})\s*bpm|bpm\s*(\d{2,3}))$/i;
const VOICE = /\b(voices?|vocals?|vocalists?|singers?|choir|duet|rappers?)\b/i;
const LANGUAGE_WORD = new RegExp(`\\b(${STYLE_LANGUAGES.join('|')})\\b`, 'i');
function languageIn(text: string): string {
  const bare = STYLE_LANGUAGES.find(language => language.toLowerCase() === text.trim().toLowerCase());
  const embedded = VOICE.test(text) ? LANGUAGE_WORD.exec(text)?.[1] : undefined;
  return bare || STYLE_LANGUAGES.find(language => language.toLowerCase() === embedded?.toLowerCase()) || '';
}
export type StyleRole = 'language' | 'voice' | 'tempo';
function isStyleRole(text: string, role: StyleRole) {
  return role === 'language' ? Boolean(languageIn(text)) : role === 'voice' ? VOICE.test(text) : TEMPO.test(text.trim());
}
export function styleRole(text: string, role: StyleRole): string {
  const part = text.split(/[,\n]/).find(part => isStyleRole(part, role))?.trim() ?? '';
  if (role === 'language') return languageIn(part);
  if (role === 'voice') return part;
  const match = TEMPO.exec(part);
  return match ? String(Number(match[1] || match[2])) : '';
}
export function setStyleRole(text: string, role: StyleRole, value: string): string {
  const parts = text.split(/[,\n]/);
  const index = parts.findIndex(part => isStyleRole(part, role));
  let nextValue = value.trim();
  if (role === 'language') {
    let found = false;
    const changed = parts.map(part => {
      if (!languageIn(part)) return part;
      found = true;
      if (VOICE.test(part)) return part.replace(new RegExp(LANGUAGE_WORD.source, 'gi'), nextValue).replace(/ {2,}/g, ' ');
      return nextValue ? `${/^\s/.test(part) ? ' ' : ''}${nextValue}` : '';
    });
    if (!found && nextValue) changed.unshift(nextValue);
    return changed.filter(part => part.trim()).join(',');
  }
  if (role === 'tempo' && nextValue) {
    const number = Number(nextValue);
    if (!Number.isFinite(number)) return text;
    nextValue = `${Math.max(40, Math.min(200, Math.round(number)))} BPM`;
  }
  if (index >= 0) {
    if (nextValue) parts[index] = `${/^\s/.test(parts[index]) ? ' ' : ''}${nextValue}`;
    else parts.splice(index, 1);
  } else if (nextValue) {
    if (role === 'tempo') parts.push(` ${nextValue}`);
    else {
      const genre = parts.findIndex(part => part.trim() && !isStyleRole(part, 'language') && !isStyleRole(part, 'tempo'));
      parts.splice(genre < 0 ? 0 : genre + 1, 0, ` ${nextValue}`);
    }
  }
  return parts.filter(part => part.trim()).join(',');
}
export function cycleSection(tag: string): string {
  const match = /^(.*?)\s*(\d+)?\s*$/.exec(tag);
  const next = SECTION_CYCLE[(SECTION_CYCLE.indexOf(match?.[1] ?? tag) + 1) % SECTION_CYCLE.length];
  return match?.[2] ? `${next} ${match[2]}` : next;
}
export function toggleLetterCase(line: string, index: number): string {
  const letters = Array.from(line);
  const letter = letters[index];
  if (!letter) return line;
  const flipped = letter === letter.toUpperCase() ? letter.toLowerCase() : letter.toUpperCase();
  if (Array.from(flipped).length === 1) letters[index] = flipped;
  return letters.join('');
}
export type LineAction = 'up' | 'down' | 'duplicate' | 'delete' | 'duplicateStanza' | 'deleteStanza';
export function editLyricLine(sections: LyricSection[], section: number, index: number, action: LineAction): LyricSection[] {
  const next = sections.map(part => ({ ...part }));
  if (!next[section]) return next;
  const lines = next[section].body.split('\n');
  if (index < 0 || index >= lines.length) return next;
  if (action === 'duplicate') lines.splice(index + 1, 0, lines[index]);
  else if (action === 'delete') lines.splice(index, 1);
  else if (action === 'up' || action === 'down') {
    const step = action === 'up' ? -1 : 1;
    const target = index + step;
    if (target >= 0 && target < lines.length) [lines[index], lines[target]] = [lines[target], lines[index]];
    else if (next[section + step]) {
      const other = next[section + step].body.split('\n');
      const [line] = lines.splice(index, 1);
      if (step < 0) other.push(line); else other.unshift(line);
      next[section + step].body = other.join('\n');
    }
  } else if (lines[index]) {
    let first = index;
    let last = index;
    while (first > 0 && lines[first - 1]) first--;
    while (last + 1 < lines.length && lines[last + 1]) last++;
    if (action === 'duplicateStanza') lines.splice(last + 1, 0, '', ...lines.slice(first, last + 1));
    else {
      if (lines[last + 1] === '') last++;
      else if (lines[first - 1] === '') first--;
      lines.splice(first, last - first + 1);
    }
  }
  next[section].body = lines.join('\n');
  return next;
}

/** Keep arbitrary section names and untagged opening lines when switching views. */
export function lyricSections(text: string): LyricSection[] {
  const sections: LyricSection[] = [];
  let current: { tag: string; lines: string[] } | undefined;
  const flush = (separator: boolean) => {
    if (!current) return;
    const lines = [...current.lines];
    if (separator && lines.at(-1) === '') lines.pop();
    sections.push({ tag: current.tag, body: lines.join('\n') });
  };
  for (const line of text.replace(/\r\n/g, '\n').split('\n')) {
    const tag = /^\[([^\]\n]+)\]\s*$/.exec(line);
    if (tag) {
      flush(true);
      current = { tag: tag[1], lines: [] };
    } else {
      if (!current) current = { tag: '', lines: [] };
      current.lines.push(line);
    }
  }
  flush(false);
  return sections;
}

export const sectionText = (sections: LyricSection[]) => sections.map(({ tag, body }) => `${tag.trim() ? `[${tag}]\n` : ''}${body}`).join('\n\n');

export function writerInstruction(idea: string, language: string, lines: string, extra: string): string {
  return [idea.trim(), language.trim() && `Write the sung lyrics in ${language.trim()} and name that vocal language first in the style.`,
    lines && `Write exactly ${lines} sung lines in total, excluding section tags. Repeat the chorus where the song repeats it.`, extra.trim()].filter(Boolean).join('\n\n');
}
