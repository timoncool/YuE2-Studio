import { expect, it } from 'vitest';
import { splitDit } from './AceDitSwitcher';

it('reads the variant and the quantisation of every catalogue DiT name, as ACE-Step Studio groups them', () => {
  expect(splitDit('acestep-v15-xl-turbo-Q8_0.gguf')).toEqual({ variant: 'xl-turbo', quant: 'q8' });
  expect(splitDit('acestep-v15-base-Q4_K_M.gguf')).toEqual({ variant: 'base', quant: 'q4' });
  expect(splitDit('acestep-v15-turbo-shift3-Q6_K.gguf')).toEqual({ variant: 'turbo-shift3', quant: 'q6' });
  expect(splitDit('acestep-v15-merge-sft-turbo-xl-ta-0.3-MXFP4.gguf')).toEqual({ variant: 'merge-sft-turbo-xl-ta-0.3', quant: 'mxfp4' });
  expect(splitDit('acestep_1.5_xl_turbo_regrind_v1-Q5_K_M.gguf')).toEqual({ variant: 'xl-turbo-regrind', quant: 'q5' });
});
