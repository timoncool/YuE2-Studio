import { describe, expect, it, vi } from 'vitest';
import { createHardwareEncoder } from './videoEncoder';

describe('createHardwareEncoder', () => {
  it('fails instead of waiting forever when the frame queue stops draining', async () => {
    vi.useFakeTimers();
    class StalledEncoder {
      static async isConfigSupported() {
        return { supported: true };
      }

      encodeQueueSize = 13;

      configure() {}
      close() {}
    }

    vi.stubGlobal('VideoEncoder', StalledEncoder);
    vi.stubGlobal('VideoFrame', class {});

    try {
      const encoder = await createHardwareEncoder({ width: 640, height: 480, fps: 30 });
      expect(encoder).not.toBeNull();

      const pendingEncode = encoder!.encode(document.createElement('canvas'), 0);
      const assertion = expect(pendingEncode).rejects.toThrow('stopped processing frames');
      await vi.advanceTimersByTimeAsync(15_000);
      await assertion;
    } finally {
      vi.useRealTimers();
      vi.unstubAllGlobals();
    }
  });
});
