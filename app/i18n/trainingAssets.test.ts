import { describe, expect, it } from 'vitest';
import { trainingAssetLabel } from './trainingAssets';
import { LANGUAGE_NAMES, translations, type Language } from './translations';

describe('training and provider labels', () => {
  it('names a training component in the window language, without the build revision', () => {
    const installed = { id: 'music-train', label: 'Trainer (HOT-Step 75923e70)' };
    expect(trainingAssetLabel('ru', installed)).toBe('Тренер');
    expect(trainingAssetLabel('ja', installed)).toBe('トレーナー');
    expect(trainingAssetLabel('ru', { id: 'yue2-convrot-base', label: 'YuE2 base for training (INT8 ConvRot)' })).toBe('Базовая модель YuE2 для обучения');
    expect(trainingAssetLabel('pt', { id: 'extension', label: 'Custom trainer' })).toBe('Custom trainer');
  });

  it('provides visibility controls and an asset translation in all six languages', () => {
    for (const language of Object.keys(LANGUAGE_NAMES) as Language[]) {
      const { providerKeyShow, providerKeyHide, providerKeyReplace } = translations[language];
      expect(providerKeyShow).toBeTruthy();
      expect(providerKeyHide).toBeTruthy();
      expect(providerKeyReplace).toBeTruthy();
      expect(providerKeyHide).not.toBe(providerKeyShow);
      const label = trainingAssetLabel(language, { id: 'yue2-lyric-aligner', label: 'untranslated legacy label' });
      expect(label).toContain('MMS');
    }
  });
});
