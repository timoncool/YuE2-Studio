import { describe, expect, it } from 'vitest';
import { completeCustomComponentIds, componentPrecision, componentsByKind, partialOnDisk, setupLabel, type ModelComponent } from './modelCatalog';

describe('partialOnDisk', () => {
  it('counts what stopped downloads left of the files still missing, against those files whole', () => {
    const files: ModelComponent[] = [
      { id: 'a', kind: 'backbone', filename: 'a.gguf', bytes: 100, sha256: 'a', partial_bytes: 40 },
      { id: 'b', kind: 'vae', filename: 'b.gguf', bytes: 50, sha256: 'b' },
      { id: 'c', kind: 'transcriber', filename: 'c.gguf', bytes: 30, sha256: 'c', partial_bytes: 30 },
    ];
    expect(partialOnDisk(files, ['a', 'b'])).toEqual({ done: 40, total: 100 });
    expect(partialOnDisk(files, ['b'])).toEqual({ done: 0, total: 0 });
  });
});

describe('setupLabel', () => {
  const t = ((key: string) => `<${key}>`) as Parameters<typeof setupLabel>[0];
  it('names the set, and the own model first when it sings in the set\'s place', () => {
    expect(setupLabel(t, 'ru', { selected_profile_id: 'quality-q8' })).toBe('<profile_quality-q8>');
    expect(setupLabel(t, 'ru', { selected_component_ids: ['a'] })).toBe('<customSet>');
    expect(setupLabel(t, 'ru', null)).toBe('<noProfile>');
    expect(setupLabel(t, 'ru', { selected_profile_id: 'quality-q8', own_model: 'mine.gguf' })).toBe('<ownModelRow>: mine.gguf · <profile_quality-q8>');
    expect(setupLabel(t, 'ru', { selected_profile_id: 'quality-q8', own_model: 'rs.gguf', own_model_name: { en: 'RS', ru: 'РС' } })).toBe('<ownModelRow>: РС · <profile_quality-q8>');
    expect(setupLabel(t, 'ja', { selected_profile_id: 'quality-q8', own_model: 'rs.gguf', own_model_name: { en: 'RS' } })).toBe('<ownModelRow>: RS · <profile_quality-q8>');
  });
});

const components: ModelComponent[] = [
  { id: 'backbone-q8', kind: 'backbone', filename: 'YuE2-3B-Q8_0.gguf', bytes: 1, sha256: 'a' },
  { id: 'backbone-q5', kind: 'backbone', filename: 'YuE2-3B-Q5_K_M.gguf', bytes: 1, sha256: 'b' },
  { id: 'vae-f32', kind: 'vae', filename: 'YuE2-Vae-F32.gguf', bytes: 1, sha256: 'c' },
  { id: 'transcriber-q8', kind: 'transcriber', filename: 'SheetSage2-Q8_0.gguf', bytes: 1, sha256: 'd' },
];

describe('YuE2 model catalog helpers', () => {
  it('needs a backbone and a VAE; the transcriber is optional', () => {
    expect(completeCustomComponentIds(components, { backbone: 'backbone-q8', vae: 'vae-f32', transcriber: 'transcriber-q8' }))
      .toEqual(['backbone-q8', 'vae-f32', 'transcriber-q8']);
    expect(completeCustomComponentIds(components, { backbone: 'backbone-q5', vae: 'vae-f32' })).toEqual(['backbone-q5', 'vae-f32']);
    expect(completeCustomComponentIds(components, { backbone: 'backbone-q8' })).toBeNull();
    expect(completeCustomComponentIds(components, { backbone: 'vae-f32', vae: 'vae-f32' })).toBeNull();
  });

  it('brings the MERT encoder of the transcriber precision along', () => {
    const catalog: ModelComponent[] = [
      ...components,
      { id: 'mert-f32', kind: 'transcriber-base', filename: 'MERT-v2-FullSong-F32.gguf', bytes: 1, sha256: 'f' },
      { id: 'mert-q8', kind: 'transcriber-base', filename: 'MERT-v2-FullSong-Q8_0.gguf', bytes: 1, sha256: 'g' },
    ];
    expect(completeCustomComponentIds(catalog, { backbone: 'backbone-q8', vae: 'vae-f32', transcriber: 'transcriber-q8' }))
      .toEqual(['backbone-q8', 'vae-f32', 'transcriber-q8', 'mert-q8']);
    expect(componentsByKind(catalog).map((group) => group.kind)).toEqual(['backbone', 'vae', 'transcriber']);
  });

  it('groups by role and marks the optional one', () => {
    const groups = componentsByKind(components);
    expect(groups.map((group) => group.kind)).toEqual(['backbone', 'vae', 'transcriber']);
    expect(groups.find((group) => group.kind === 'transcriber')?.optional).toBe(true);
  });

  it('derives the displayed precision from the pinned filename', () => {
    expect(componentPrecision(components[0])).toBe('Q8_0');
    expect(componentPrecision(components[1])).toBe('Q5_K_M');
    expect(componentPrecision(components[2])).toBe('F32');
  });

  it('puts the decoder companion in every set without offering it as a choice', () => {
    const companion: ModelComponent = { id: 'companion-v9', kind: 'companion', filename: 'nar_lora_joint_v9.safetensors', bytes: 1, sha256: 'e' };
    const catalog = [...components, companion];
    expect(completeCustomComponentIds(catalog, { backbone: 'backbone-q5', vae: 'vae-f32' })).toEqual(['backbone-q5', 'vae-f32', 'companion-v9']);
    expect(componentsByKind(catalog).map((group) => group.kind)).toEqual(['backbone', 'vae', 'transcriber']);
    expect(componentPrecision(companion)).toBe('v9');
  });
});
