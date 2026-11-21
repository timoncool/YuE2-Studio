import { mkdir, readFile, unlink, writeFile } from 'node:fs/promises';
import path from 'node:path';

const [directory, downloads] = process.argv.slice(2);
const discovery = 'http://127.0.0.1:9889';
const trace = [];
const shouldObserve = new Set();
const pending = new Map();
let sequence = 0;

function connect(url) {
  return new Promise((resolve, reject) => {
    const socket = new WebSocket(url);
    socket.addEventListener('open', () => resolve(socket), { once: true });
    socket.addEventListener('error', reject, { once: true });
    socket.addEventListener('message', event => {
      const message = JSON.parse(event.data);
      if (message.id) {
        const result = pending.get(message.id);
        pending.delete(message.id);
        if (message.error) result?.reject(message.error);
        else result?.resolve(message.result);
        return;
      }
      const fields = message.params;
      if (message.method === 'Fetch.requestPaused') {
        void continueSave(socket, fields).catch(error => { console.error(error); process.exit(1); });
        return;
      }
      if (message.method === 'Network.requestWillBeSent' && fields.request.url.includes('/v1/files/save')) {
        shouldObserve.add(fields.requestId);
        trace.push({ method: message.method, id: fields.requestId, request: fields.request });
      } else if (shouldObserve.has(fields?.requestId) || message.method?.includes('download')) {
        trace.push({ method: message.method, fields });
        if (message.method === 'Network.loadingFinished') {
          void send(socket, 'Network.getResponseBody', { requestId: fields.requestId })
            .then(async result => {
              trace.push({ id: fields.requestId, responseBody: result.body });
              const error = JSON.parse(result.body)?.error;
              if (typeof error !== 'string' || !error.length) return;
              const expression = 'document.body.innerText.includes(' + JSON.stringify(error) + ')';
              for (let attempt = 0; attempt < 30; attempt++) {
                const value = await send(socket, 'Runtime.evaluate', { expression, returnByValue: true });
                if (value.result.value) {
                  trace.push({ id: fields.requestId, visibleSaveError: error });
                  break;
                }
                await new Promise(resolve => setTimeout(resolve, 100));
              }
            })
            .catch(error => trace.push({ id: fields.requestId, responseBodyError: error }));
        }
      }
    });
  });
}

// The runner introduces a real destination conflict after the native picker has closed.
async function continueSave(socket, fields) {
  const marker = path.join(directory, 'save-conflict.json');
  let scenario;
  try { scenario = JSON.parse(await readFile(marker, 'utf8')); }
  catch (error) { if (error.code !== 'ENOENT') throw error; }
  if (scenario && fields.request.method === 'POST') {
    if (!/^conflict-[a-z0-9-]+\.[a-z0-9]+$/.test(scenario.file)) throw new Error('Unexpected fixture filename');
    await mkdir(path.join(downloads, scenario.file));
    await unlink(marker);
  }
  await send(socket, 'Fetch.continueRequest', { requestId: fields.requestId });
}

function send(socket, method, params = {}) {
  return new Promise((resolve, reject) => {
    const id = ++sequence;
    pending.set(id, { resolve, reject });
    socket.send(JSON.stringify({ id, method, params }));
  });
}

await mkdir(downloads, { recursive: true });
const targets = await fetch(discovery + '/json/list').then(reply => reply.json());
const target = targets.find(item => item.type === 'page' && item.webSocketDebuggerUrl);
if (!target) throw new Error('The isolated Studio WebView target is missing');
const page = await connect(target.webSocketDebuggerUrl);
await send(page, 'Network.enable');
await send(page, 'Fetch.enable', { patterns: [{ urlPattern: '*/v1/files/save/*', requestStage: 'Request' }] });
await send(page, 'Page.enable');
const { result } = await send(page, 'Runtime.evaluate', {
  expression: '({ address: location.href, desktop: Boolean(window.__TAURI__), focused: document.hasFocus() })',
  returnByValue: true,
});
trace.push({ view: result.value });
try {
  await send(page, 'Page.setDownloadBehavior', { behavior: 'allow', downloadPath: downloads });
} catch (error) {
  trace.push({ downloadControlError: error });
}

async function record() {
  await writeFile(path.join(directory, 'lrc-webview-network.json'), JSON.stringify(trace, null, 2));
}

await record();
await writeFile(path.join(directory, 'lrc-observer-ready'), '');
const timer = setInterval(() => void record(), 300);
process.stdin.resume();
process.stdin.once('end', () => {
  clearInterval(timer);
  void record().finally(() => page.close());
});
