import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import { SongbookDialog } from './SongbookDialog';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ language: 'en' }) }));
globalThis.IS_REACT_ACT_ENVIRONMENT = true;

it('shows transposed jianpu key changes independently of capo and ukulele shapes', () => {
  const host = document.createElement('div');
  document.body.append(host);
  const root = createRoot(host);
  try {
    const abc = 'X:1\nM:4/4\nL:1/4\nK:C\n"C"C D E G | [K:D] "D"D E F A |';
    act(() => root.render(<SongbookDialog abc={abc} title="Key change" onClose={() => {}} />));
    const sheet = document.querySelector<HTMLDialogElement>('dialog[aria-label="Songbook"]')!;
    const step = (label: string, times: number) => {
      for (let count = 0; count < times; count += 1) act(() => sheet.querySelector<HTMLButtonElement>(`button[aria-label="${label}: +1"]`)!.click());
    };
    const keyLabels = () => [...sheet.querySelectorAll('.songbook-cell > span')]
      .map(label => label.textContent).filter(value => value?.startsWith('Key'));
    expect(keyLabels()).toEqual([]);
    act(() => sheet.querySelector<HTMLButtonElement>('[role="switch"]')!.click());
    expect(keyLabels()).toEqual(['Key D']);
    step('Transpose (semitones)', 2);
    expect(keyLabels()).toEqual(['Key E']);
    step('Capo fret', 2);
    act(() => sheet.querySelector<HTMLButtonElement>('[role="radiogroup"][aria-label="Chord shapes"] [data-value="ukulele"]')!.click());
    expect(keyLabels()).toEqual(['Key E']);
    expect(sheet.querySelector('h3')?.textContent).toBe('Ukulele');
    expect(sheet.querySelector('figure svg[aria-label^="C:"]')?.getAttribute('aria-label')?.split(' ').slice(1)).toHaveLength(4);
  } finally {
    act(() => root.unmount());
    host.remove();
  }
});

it('shows the printable songbook and updates chord shapes when the capo changes', () => {
  const host = document.createElement('div');
  document.body.append(host);
  const root = createRoot(host);
  const print = vi.fn();
  vi.stubGlobal('print', print);
  const close = vi.fn();
  try {
    act(() => root.render(<SongbookDialog abc={'X:1\nM:4/4\nL:1/4\nK:D\n"D"D E F A|\nw:Hel-lo beau-ty'}
      title="My song" onClose={close} />));
    const sheet = document.querySelector<HTMLDialogElement>('dialog[aria-label="Songbook"]')!;
    expect(sheet.open).toBe(true);
    expect(sheet.querySelector('h2')?.textContent).toBe('Songbook');
    expect(sheet.querySelector('article h2')?.textContent).toBe('My song');
    expect(sheet.querySelector('strong')?.textContent).toBe('D');
    expect(sheet.textContent).toContain('Hel-');
    for (let count = 0; count < 2; count += 1) act(() => sheet.querySelector<HTMLButtonElement>('button[aria-label="Capo fret: +1"]')!.click());
    expect(sheet.querySelector('strong')?.textContent).toBe('C');
    expect(sheet.querySelector('figcaption')?.textContent).toBe('C');
    const printButton = Array.from(sheet.querySelectorAll('button')).find(button => button.textContent === 'Print A4')!;
    act(() => printButton.click());
    expect(print).toHaveBeenCalledOnce();
    act(() => sheet.querySelector<HTMLButtonElement>('button[aria-label="Close songbook"]')!.click());
    expect(close).toHaveBeenCalledOnce();
  } finally {
    act(() => root.unmount());
    host.remove();
    vi.unstubAllGlobals();
  }
});
