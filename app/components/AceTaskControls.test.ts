import { expect, it } from 'vitest';
import { aceTaskOptions, restoreAceTask } from './AceTaskControls';

it('sends the base-model tasks with the tracks ACE-Step Studio sent, and holds them back without one', () => {
  expect(aceTaskOptions({ task: 'lego', source: 's1', reference: '', tracks: ['guitar'], repainting_start: '4', repainting_end: '12' }))
    .toEqual({ task_type: 'lego', source_song_id: 's1', track: 'guitar', repainting_start: 4, repainting_end: 12 });
  expect(aceTaskOptions({ task: 'complete', source: 's1', reference: '', tracks: ['drums', 'bass'] }))
    .toEqual({ task_type: 'complete', source_song_id: 's1', track: 'DRUMS | BASS' });
  expect(aceTaskOptions({ task: 'extract', source: 's1', reference: '' })).toBeUndefined();
  expect(aceTaskOptions({ task: 'repaint', source: 's1', reference: '', repainting_start: '10', repainting_end: '5' })).toBeUndefined();
  expect(aceTaskOptions({ task: 'text2music', source: '', reference: 'r1' })).toEqual({ reference_song_id: 'r1' });
});

it('puts a saved task back into the card for Reuse', () => {
  expect(restoreAceTask({ task_type: 'complete', source_song_id: 's1', track: 'DRUMS | BASS' }))
    .toEqual({ task: 'complete', source: 's1', reference: '', tracks: ['drums', 'bass'] });
  expect(restoreAceTask({ task_type: 'cover', source_song_id: 's2', audio_cover_strength: 0.6 }))
    .toEqual({ task: 'cover', source: 's2', reference: '', audio_cover_strength: '0.6' });
});
