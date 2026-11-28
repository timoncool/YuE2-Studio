import { describe, expect, it } from 'vitest';
import { engravedScore, fontScoreAccidentals } from './scoreEngraving';

const head = 'X:1\nM:2/4\nL:1/32\nK:Em\n';

describe('engravedScore', () => {
  it('draws a multi-measure rest as whole-bar rests', () => {
    expect(engravedScore(`${head}V: Vocal\nZ4|\nV: Ins\nZ|E4B4f2g4d2-|\n`)).toBe(`${head}V: Vocal\nz16|z16|z16|z16|\nV: Ins\nz16|E4B4f2g4d2-|\n`);
  });

  it('leaves headers, comments, chords and plain rests alone', () => {
    const score = `${head}% intro Z4|\nV: Vocal\n"Em"z16|e8b8|\n`;
    expect(engravedScore(score)).toBe(score);
  });

  it('reads common time and leaves a score without a bar length as it is', () => {
    expect(engravedScore('X:1\nM:C\nL:1/8\nK:C\nZ2|\n')).toBe('X:1\nM:C\nL:1/8\nK:C\nz8|z8|\n');
    expect(engravedScore('X:1\nK:C\nZ2|\n')).toBe('X:1\nK:C\nZ2|\n');
  });
});

describe('fontScoreAccidentals', () => {
  it('sets the embedded font only on accidental characters in SVG chords', () => {
    const host = document.createElement('div');
    host.innerHTML = '<svg><text data-name="chord"><tspan x="10">E♭maj7/F♯</tspan></text><text data-name="chord">D♮</text><text data-name="chord">Dm</text><text data-name="lyric">♭</text></svg>';
    expect(fontScoreAccidentals(host, 'ScoreAccidentals')).toBe(true);
    const spans = [...host.querySelectorAll('tspan[font-family="ScoreAccidentals"]')];
    expect(spans.map(span => span.textContent)).toEqual(['♭', '♯', '♮']);
    expect(spans.every(span => span.namespaceURI === 'http://www.w3.org/2000/svg')).toBe(true);
    expect(host.querySelector('tspan[x="10"]')?.textContent).toBe('E♭maj7/F♯');
    expect(host.querySelector('[data-name="lyric"]')?.innerHTML).toBe('♭');
  });

  it('leaves an ordinary score untouched and does not request the embedded font', () => {
    const host = document.createElement('div');
    host.innerHTML = '<svg><text data-name="chord"><tspan x="10">Dm</tspan></text></svg>';
    const original = host.innerHTML;
    expect(fontScoreAccidentals(host, 'ScoreAccidentals')).toBe(false);
    expect(host.innerHTML).toBe(original);
  });
});
