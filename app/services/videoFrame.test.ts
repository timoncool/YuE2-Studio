import { describe, expect, it, vi } from 'vitest';
import { seekVideoToTime } from './videoFrame';

describe('seekVideoToTime', () => {
  it('does not wait for a seeked event when the requested frame is already ready', async () => {
    const video = document.createElement('video');
    Object.defineProperties(video, {
      currentTime: { configurable: true, value: 0 },
      readyState: { configurable: true, value: 2 },
    });
    const addEventListener = vi.spyOn(video, 'addEventListener');

    await expect(seekVideoToTime(video, 0)).resolves.toBeUndefined();

    expect(addEventListener).not.toHaveBeenCalledWith('seeked', expect.any(Function));
  });

  it('waits for the requested frame after changing currentTime', async () => {
    const video = document.createElement('video');
    let currentTime = 0;
    let seeking = false;
    Object.defineProperties(video, {
      currentTime: {
        configurable: true,
        get: () => currentTime,
        set: (value: number) => {
          currentTime = value;
          seeking = true;
          setTimeout(() => {
            seeking = false;
            video.dispatchEvent(new Event('seeked'));
          }, 0);
        },
      },
      seeking: { configurable: true, get: () => seeking },
      readyState: { configurable: true, value: 2 },
    });

    await expect(seekVideoToTime(video, 1)).resolves.toBeUndefined();
    expect(video.currentTime).toBe(1);
  });
});
