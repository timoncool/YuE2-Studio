import React, { act, useState } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { SongTextParts } from './SongTextParts';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ language: 'en' }) }));
globalThis.IS_REACT_ACT_ENVIRONMENT = true;
let root: Root;
let host: HTMLDivElement;
let output: string;
function mount(kind: 'style' | 'lyrics', initial: string) {
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
  function Harness() {
    const [text, setText] = useState(initial);
    output = text;
    return <SongTextParts kind={kind} value={text} onChange={setText} />;
  }
  act(() => root.render(<Harness />));
  act(() => host.querySelector('button')!.click());
}
function click(title: string, index = 0) {
  act(() => (host.querySelectorAll(`[data-section-controls] button[title="${title}"]`)[index] as HTMLButtonElement).click());
}
function edit(element: HTMLInputElement | HTMLTextAreaElement, value: string) {
  const prototype = element instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  Object.getOwnPropertyDescriptor(prototype, 'value')!.set!.call(element, value);
  act(() => element.dispatchEvent(new Event('input', { bubbles: true })));
}
afterEach(() => { act(() => root?.unmount()); host?.remove(); });
describe('song text controls', () => {
  it('duplicates and moves a chorus with its own words, then removes just the selected section', () => {
    mount('lyrics', '[Verse]\nFirst\n\n[Chorus]\nAgain');
    click('Duplicate', 1);
    expect(output).toBe('[Verse]\nFirst\n\n[Chorus]\nAgain\n\n[Chorus]\nAgain');
    click('Move up', 1);
    expect(output).toBe('[Chorus]\nAgain\n\n[Verse]\nFirst\n\n[Chorus]\nAgain');
    click('Delete', 1);
    expect(output).toBe('[Chorus]\nAgain\n\n[Chorus]\nAgain');
  });
  it('lets a person type spaces in tag names and new lyric lines', () => {
    mount('lyrics', '[Verse]\nFirst\n\n[Chorus]\nAgain');
    edit(host.querySelector('input')!, 'Verse ');
    expect(host.querySelector('input')!.value).toBe('Verse ');
    edit(host.querySelector('textarea')!, 'First\n');
    expect(host.querySelector('textarea')!.value).toBe('First\n');
  });
  it('edits one style descriptor and keeps all other descriptors', () => {
    mount('style', 'Russian,folk rock,male voice,120 BPM');
    edit(host.querySelector<HTMLInputElement>('[aria-label="Style descriptor 2"]')!, 'warm piano ');
    expect(output).toBe('Russian,warm piano ,male voice,120 BPM');
    expect(host.querySelector<HTMLInputElement>('[aria-label="Style descriptor 2"]')!.value).toBe('warm piano ');
  });
  it('keeps a section and its words while its tag is cleared and replaced', () => {
    mount('lyrics', '[Verse]\nFirst\n\n[Chorus]\nAgain');
    edit(host.querySelector<HTMLInputElement>('[aria-label="Section tag 2"]')!, '');
    expect(host.querySelectorAll('input[aria-label^="Section tag"]')).toHaveLength(2);
    expect(host.querySelectorAll('textarea')[1].value).toBe('Again');
    edit(host.querySelector<HTMLInputElement>('[aria-label="Section tag 2"]')!, 'Bridge');
    expect(output).toBe('[Verse]\nFirst\n\n[Bridge]\nAgain');
  });
  it('changes style language and tempo through the named controls without dropping instruments', () => {
    mount('style', 'Russian,folk rock,male voice,banjo,88 BPM');
    act(() => host.querySelector<HTMLInputElement>('input[role="combobox"]')!.click());
    act(() => [...document.querySelectorAll<HTMLElement>('[role="option"]')].find(option => option.textContent === 'Japanese')!.click());
    expect(output).toBe('Japanese,folk rock,male voice,banjo,88 BPM');
    const tempo = host.querySelector<HTMLInputElement>('input[type="number"]')!;
    edit(tempo, '140');
    act(() => tempo.dispatchEvent(new FocusEvent('focusout', { bubbles: true })));
    expect(output).toBe('Japanese,folk rock,male voice,banjo,140 BPM');
    expect(host.querySelectorAll('datalist option').length).toBeGreaterThan(28);
  });
  it('cycles a section and edits a single letter through the line controls', () => {
    mount('lyrics', '[Verse 2]\nhello\nworld');
    act(() => host.querySelector<HTMLButtonElement>('[title="Next section tag"]')!.click());
    expect(output).toBe('[Pre-Chorus 2]\nhello\nworld');
    const details = host.querySelector('details')!;
    act(() => { details.open = true; });
    act(() => details.querySelector<HTMLButtonElement>('[title="Flip letter case"]')!.click());
    expect(output).toBe('[Pre-Chorus 2]\nHello\nworld');
    act(() => details.querySelector<HTMLButtonElement>('[title="Duplicate"]')!.click());
    expect(output).toBe('[Pre-Chorus 2]\nHello\nHello\nworld');
    act(() => (Array.from(details.querySelectorAll('button')).find(button => button.textContent === 'Add line') as HTMLButtonElement).click());
    expect(output).toBe('[Pre-Chorus 2]\nHello\nHello\nworld\n');
  });
});
