import { writeFile } from 'node:fs/promises';

const [out, original, edited] = process.argv.slice(2);
const pages = await fetch('http://127.0.0.1:9889/json/list').then(response => response.json());
const page = pages.find(item => item.type === 'page' && item.url.startsWith('http://tauri.localhost/'));
if (!page) throw Error('Installed WebView is missing');
const expression = `(async () => {
  const section = [...document.querySelectorAll('button[role=tab]')]
    .find(button => button.textContent.trim() === 'B')?.closest('section');
  if (!section) throw Error('A/B section is missing');
  const [a, b] = section.querySelectorAll('audio');
  const ids = ${JSON.stringify([original, edited])};
  const tabs = [...section.querySelectorAll('[role=tab]')];
  const playback = () => [...section.querySelectorAll('audio')].map(audio => ({
    url: audio.currentSrc, duration: audio.duration, position: audio.currentTime,
    muted: audio.muted, paused: audio.paused, ready: audio.readyState,
  }));
  const matches = condition => new Promise((resolve, reject) => {
    const started = performance.now();
    const poll = () => {
      if (condition()) resolve();
      else if (performance.now() - started > 7000) reject(Error(JSON.stringify(playback())));
      else setTimeout(poll, 30);
    };
    poll();
  });
  const result = {};
  await matches(() => [a,b].every(audio => audio.readyState >= 2));
  result.initial = playback();
  if (![a,b].every((audio,i) => new URL(audio.currentSrc).pathname.endsWith('/'+ids[i]))) {
    throw Error('A/B loaded unexpected saved media: '+JSON.stringify(result));
  }
  const waveform = section.querySelector('canvas').parentElement;
  const box = waveform.getBoundingClientRect();
  waveform.dispatchEvent(new MouseEvent('mousedown',{bubbles:true,clientX:box.left+box.width*.30}));
  await matches(() => [a,b].every(audio => !audio.seeking && Math.abs(audio.currentTime - 3.6) < .10));
  result.afterSeek = playback();
  section.querySelector('button[aria-label=play]').click();
  await matches(() => !a.paused && !b.paused && b.currentTime > 3.75);
  result.playingB = playback();
  tabs[0].click();
  await matches(() => !a.muted && b.muted && !a.paused && !a.seeking);
  result.playingA = playback();
  await matches(() => a.currentTime > result.playingA[0].position + .2);
  result.beforeReturn = playback();
  tabs[1].click();
  await matches(() => a.muted && !b.muted && !b.paused && !b.seeking);
  result.returnedB = playback();
  section.querySelector('button[aria-label=pause]').click();
  await matches(() => [a,b].every(audio => audio.paused));
  result.paused = playback();
  return result;
})()`;
const socket = new WebSocket(page.webSocketDebuggerUrl);
try {
  const result = await new Promise((resolve, reject) => {
    const cutoff = setTimeout(() => reject(Error('CDP timed out')), 40000);
    socket.addEventListener('error', reject, { once: true });
    socket.addEventListener('open', () => socket.send(JSON.stringify({ id: 1, method: 'Runtime.evaluate',
      params: { expression, returnByValue: true, awaitPromise: true } })), { once: true });
    socket.addEventListener('message', event => {
      const response = JSON.parse(event.data);
      if (response.id !== 1) return;
      clearTimeout(cutoff);
      if (response.error || response.result.exceptionDetails) reject(Error(JSON.stringify(response)));
      else resolve(response.result.result.value);
    });
  });
  await writeFile(out, JSON.stringify(result, null, 2));
  for (const [before, after] of [['playingB','playingA'], ['beforeReturn','returnedB']]) {
    const lead = result[before].find(audio => !audio.muted);
    const heard = result[after].find(audio => !audio.muted);
    if (!lead || !heard || lead.url === heard.url || heard.paused || Math.abs(lead.position-heard.position) > .12) {
      throw Error(`A/B timing or media mismatch: ${JSON.stringify([lead,heard])}`);
    }
  }
} finally {
  socket.close();
}
