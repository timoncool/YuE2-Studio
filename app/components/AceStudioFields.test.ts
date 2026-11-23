import { expect, it } from 'vitest';
import { aceGenderLine, aceStudioFields, ditKind, restoreAceStudio } from './AceStudioFields';

it('sends what ACE-Step Studio sent: the turbo defaults, the song metadata and only the knobs that were moved', () => {
  const draft = {
    vocal_language: 'ru', bpm: '96', keyscale: 'A minor', timesignature: '3', gender: 'female',
    lm_temperature: '0.7', lm_rep_penalty: '1.1', lm_rep_mode: 'dry', lm_dry_base: '2', retake_variance: '0',
    group_mlp: '0.5', group_self_attn: '1', fade_out: '4',
  };
  expect(aceStudioFields(draft, ditKind('acestep-v15-xl-turbo-Q8_0.gguf'), 'text2music')).toEqual({
    think: true, vocal_language: 'ru', bpm: 96, keyscale: 'A minor', timesignature: '3',
    lm_temperature: 0.7, lm_rep_penalty: 1.1, lm_rep_mode: 'dry', lm_dry_base: 2,
    shift: 3, solver: 'heun', scheduler: 'linear_quadratic', fade_out: 4,
    adapter_group_scales: { mlp: 0.5 },
  });
  expect(aceGenderLine(draft)).toBe('Female vocals');
  expect(aceStudioFields({ think: false }, ditKind('acestep-v15-xl-sft-Q8_0.gguf'), 'text2music'))
    .toEqual({ think: false, shift: 1, solver: 'euler', scheduler: 'linear' });
});

it('puts a saved request back into the form for Reuse', () => {
  const draft = restoreAceStudio({ bpm: 120, keyscale: 'C major', think: false, solver: 'dpm2m', adapter_group_scales: { proj_in: 0.8 } });
  expect(draft).toEqual({ bpm: '120', keyscale: 'C major', think: false, solver: 'dpm2m', group_proj_in: '0.8' });
  expect(aceStudioFields(draft, 'turbo', 'text2music')).toMatchObject({ bpm: 120, think: false, solver: 'dpm2m', adapter_group_scales: { proj_in: 0.8 } });
});
