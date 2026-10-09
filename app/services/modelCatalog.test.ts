import { describe, expect, it } from 'vitest';
import { completeCustomComponentIds, componentPrecision, componentsByKind, type ModelComponent } from './modelCatalog';

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
