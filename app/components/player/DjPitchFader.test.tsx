import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { DjDeck } from '../../services/djMixer';
import { DjPitchFader } from './DjPitchFader';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
let host: HTMLDivElement | null = null;
let root: Root | null = null;
afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  host = null;
  root = null;
});

describe('DJ pitch fader parity with the library player', () => {
  it('exposes the full range and one-click 2× (+12 semitones) on both decks', () => {
    for (const side of ['A', 'B'] as const) {
      const rate = vi.fn();
      const deck = { state: { rate: 1, bpm: 100 }, rate } as unknown as DjDeck;
      host = document.createElement('div');
      document.body.append(host);
      root = createRoot(host);
      act(() => root?.render(<DjPitchFader deck={deck} side={side} color="skyblue" />));

      const fader = host.querySelector<HTMLInputElement>(`input[aria-label="Vertical pitch fader deck ${side}"]`);
      expect(fader?.min).toBe('0.25');
      expect(fader?.max).toBe('2');
      expect(host.textContent).toContain('1.00× · 100%');
      expect(host.querySelectorAll('button[aria-pressed]')).toHaveLength(8);

      const maxPreset = Array.from(host.querySelectorAll('button')).find(button => button.textContent === '2×');
      expect(maxPreset).toBeDefined();
      act(() => maxPreset?.click());
      expect(rate).toHaveBeenCalledWith(2);

      act(() => root?.render(<DjPitchFader deck={{ ...deck, state: { ...deck.state, rate: 2 } } as DjDeck} side={side} color="skyblue" />));
      expect(host.textContent).toContain('2.00× · 200%');
      expect(host.textContent).toContain('+12.0 st');

      act(() => root?.unmount());
      host.remove();
      root = null;
      host = null;
    }
  });
});
