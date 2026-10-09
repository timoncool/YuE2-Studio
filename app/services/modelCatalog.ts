import { translations, type TranslationKey } from '../i18n/translations';

export type ModelComponent = {
  id: string;
  kind: string;
  filename: string;
  bytes: number;
  sha256: string;
};

/** Every runnable set has one of each. */
export const REQUIRED_KINDS = ['backbone', 'vae'] as const;
/** SheetSage2: without it the studio generates but cannot transcribe covers. */
export const OPTIONAL_KINDS = ['transcriber'] as const;
export const COMPONENT_KINDS = [...REQUIRED_KINDS, ...OPTIONAL_KINDS] as const;
/** In every set and never a choice: the decoder companion every render merges. */
export const ALWAYS_KINDS = ['companion'] as const;

const labels: Record<(typeof COMPONENT_KINDS)[number] | (typeof ALWAYS_KINDS)[number] | 'transcriber-base', string> = {
  backbone: 'Backbone',
  vae: 'VAE',
  transcriber: 'SheetSage2',
  'transcriber-base': 'MERT-v2',
  companion: 'Decoder companion',
};

export const componentKindLabel = (kind: string) => labels[kind as keyof typeof labels] || kind;

export const isOptionalKind = (kind: string) => (OPTIONAL_KINDS as readonly string[]).includes(kind);

export const componentPrecision = (component: ModelComponent) => {
  // the GGUF files end in -<ggml type>.gguf, the companion in _v<version>.safetensors
  const quant = component.filename.match(/-([A-Z0-9_]+)\.gguf$/i)?.[1];
  if (quant) return quant.toUpperCase();
  return component.filename.match(/_(v\d+)\.safetensors$/i)?.[1] || component.id;
};

export const componentsByKind = (components: ModelComponent[]) =>
  COMPONENT_KINDS.map((kind) => ({ kind, optional: isOptionalKind(kind), components: components.filter((component) => component.kind === kind) }));

/** The ids of a runnable set, or null while a required role is still unchosen. */
export const completeCustomComponentIds = (components: ModelComponent[], selectedByKind: Record<string, string>) => {
  const ids: string[] = [];
  for (const kind of COMPONENT_KINDS) {
    const id = selectedByKind[kind];
    if (!id) {
      if (isOptionalKind(kind)) continue;
      return null;
    }
    const component = components.find((entry) => entry.id === id);
    if (!component || component.kind !== kind) return null;
    ids.push(id);
  }
  const head = components.find((component) => component.id === selectedByKind.transcriber);
  // the transcriber head reads MERT from the file of its own precision beside it
  const base = head && components.find((component) => component.kind === 'transcriber-base' && componentPrecision(component) === componentPrecision(head));
  if (base) ids.push(base.id);
  for (const component of components) {
    if ((ALWAYS_KINDS as readonly string[]).includes(component.kind)) ids.push(component.id);
  }
  return ids;
};

export const selectedComponentBytes = (components: ModelComponent[], ids: string[] | null) =>
  ids?.reduce((total, id) => total + (components.find((component) => component.id === id)?.bytes || 0), 0) || 0;

/** The set's name in the UI language; a set the studio has no name for keeps the server's. */
export function profileLabel(t: (key: TranslationKey) => string, profile: { id: string; label: string }): string {
  const key = `profile_${profile.id}`;
  return key in translations.en ? t(key as TranslationKey) : profile.label;
}
