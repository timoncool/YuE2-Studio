import { readFile } from 'node:fs/promises';

const [port, action] = process.argv.slice(2);
const page = (await fetch(`http://127.0.0.1:${port}/json/list`).then(reply => reply.json()))
  .find(target => target.type === 'page' && target.url.startsWith('http://tauri.localhost/'));
if (!page) throw new Error('Studio WebView is missing');
const expressions = {
  install: `(() => {
    const tauri = window.__TAURI__;
    const invoke = tauri.core.invoke;
    window.songNotificationTrace = [];
    window.__TAURI__ = {...tauri, core: {...tauri.core, async invoke(command, args) {
      if (command !== 'notify_song_ready') return invoke(command, args);
      const result = { message: args.message, status: 'pending' };
      window.songNotificationTrace.push(result);
      try {
        const value = await invoke(command, args);
        result.status = value === true ? 'shell_accepted' : 'suppressed';
        return value;
      } catch (error) {
        result.status = 'shell_error';
        result.error = String(error);
        throw error;
      }
    }}};
    return window.__TAURI__ !== tauri;
  })()`,
  read: '(async () => ({ invocations: window.songNotificationTrace, preference: await window.__TAURI__.core.invoke("song_notification_preference") }))()',
};
const expression = expressions[action];
if (!expression) throw new Error('Unrecognized observation mode');
const socket = new WebSocket(page.webSocketDebuggerUrl);
try {
  await new Promise((resolve, reject) => {
    const cutoff = setTimeout(() => reject(new Error('CDP timeout')), 8000);
    socket.addEventListener('error', reject, { once: true });
    socket.addEventListener('open', () => {
      socket.send(JSON.stringify({ id: 1, method: 'Runtime.evaluate', params: { expression, returnByValue: true, awaitPromise: true } }));
    }, { once: true });
    socket.addEventListener('message', event => {
      const response = JSON.parse(event.data);
      if (response.id !== 1) return;
      clearTimeout(cutoff);
      if (response.error || response.result.exceptionDetails) reject(new Error(JSON.stringify(response)));
      else {
        console.log(JSON.stringify(response.result.result.value));
        resolve();
      }
    });
  });
} finally {
  socket.close();
}
