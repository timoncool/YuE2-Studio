import { afterEach, describe, expect, it, vi } from 'vitest';
import { keepContextRunning } from './audioGraph';

/** Just enough of an AudioContext for the watch: WebKitGTK starts it "interrupted". */
class FakeContext extends EventTarget {
  state = 'interrupted';
  resumed = 0;

  resume(): Promise<void> {
    this.resumed += 1;
    this.state = 'running';
    this.dispatchEvent(new Event('statechange'));
    return Promise.resolve();
  }
}

const open = (state = 'interrupted') => {
  const context = new FakeContext();
  context.state = state;
  return context;
};

afterEach(() => {
  vi.useRealTimers();
});

describe('keeping an audio context running', () => {
  it('resumes a context WebKitGTK calls interrupted', () => {
    const context = open();
    const stop = keepContextRunning(context as unknown as AudioContext);
    expect(context.resumed).toBe(1);
    expect(context.state).toBe('running');
    stop();
  });

  it('resumes a context the system interrupted later, even though nothing announces it', () => {
    vi.useFakeTimers();
    const context = open('running');
    const stop = keepContextRunning(context as unknown as AudioContext);
    expect(context.resumed).toBe(0);
    // another audio session takes over: the watch asks again on its own
    context.state = 'interrupted';
    vi.advanceTimersByTime(1500);
    expect(context.resumed).toBe(1);
    expect(context.state).toBe('running');
    stop();
  });

  it('asks again when the context reports the interruption itself', () => {
    const context = open('running');
    const stop = keepContextRunning(context as unknown as AudioContext);
    context.state = 'suspended';
    context.dispatchEvent(new Event('statechange'));
    expect(context.resumed).toBe(1);
    stop();
  });

  it('stops watching once the context is let go', () => {
    vi.useFakeTimers();
    const context = open('running');
    const stop = keepContextRunning(context as unknown as AudioContext);
    stop();
    context.state = 'interrupted';
    vi.advanceTimersByTime(5000);
    expect(context.resumed).toBe(0);
  });
});
