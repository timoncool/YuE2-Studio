import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';

const [directory, songId] = process.argv.slice(2);
const discovery = 'http://127.0.0.1:9890';
const trace = [];
const sleep = delay => new Promise(resolve => setTimeout(resolve, delay));

async function retry(operation, message) {
  for (let attempt = 0; attempt < 90; attempt++) {
    const result = await operation().catch(() => null);
    if (result) return result;
    await sleep(250);
  }
  throw new Error(message);
}

function connect(url) {
  return new Promise((resolve, reject) => {
    const socket = new WebSocket(url);
    const pending = new Map();
    let request = 0;
    socket.addEventListener('open', () => resolve((method, params = {}) => new Promise((done, failed) => {
      const id = ++request;
      pending.set(id, { done, failed });
      socket.send(JSON.stringify({ id, method, params }));
    })), { once: true });
    socket.addEventListener('error', reject, { once: true });
    socket.addEventListener('message', event => {
      const message = JSON.parse(event.data);
      if (message.method?.includes('download')) trace.push(message);
      if (!message.id) return;
      const reply = pending.get(message.id);
      pending.delete(message.id);
      if (message.error) reply?.failed(message.error);
      else reply?.done(message.result);
    });
  });
}

await mkdir(directory, { recursive: true });
const browser = await retry(async () => fetch(discovery + '/json/version').then(reply => reply.json()), 'Chrome debugging endpoint did not start');
const controlBrowser = await connect(browser.webSocketDebuggerUrl);
try {
  await controlBrowser('Browser.setDownloadBehavior', { behavior: 'allow', downloadPath: directory, eventsEnabled: true });
  const target = await retry(async () => {
    const targets = await fetch(discovery + '/json/list').then(reply => reply.json());
    return targets.find(item => item.type === 'page' && item.url.startsWith('http://127.0.0.1:8791/'));
  }, 'The Studio tab did not open');
  const controlPage = await connect(target.webSocketDebuggerUrl);
  const evaluate = async expression => {
    const value = await controlPage('Runtime.evaluate', { expression, returnByValue: true });
    if (value.exceptionDetails) throw new Error(value.exceptionDetails.text);
    return value.result.value;
  };
  const click = (caption, scope, allowPrefix = false) => evaluate(`(() => {
    const scope = ${scope};
    if (!scope) return false;
    for (const button of scope.querySelectorAll('button')) {
      if (button.disabled || !button.getBoundingClientRect().height) continue;
      const labels = [button.getAttribute('aria-label'), button.title, button.innerText].filter(Boolean).map(label => label.trim().toLowerCase());
      const caption = ${JSON.stringify(caption.toLowerCase())};
      if (labels.some(label => ${allowPrefix} ? label.startsWith(caption) : label === caption)) {
        button.click();
        return true;
      }
    }
    return false;
  })()`);
  await retry(async () => {
    if (await click('Library', 'document')) return true;
    await click('Use OpenRouter', 'document');
    return false;
  }, 'Library was unavailable');
  trace.push({ clicked: 'Library' });
  const context = JSON.stringify(`[data-mcp-context^="song ${songId}:"]`);
  await retry(() => click('Song Details', `document.querySelector(${context})`), 'Details were unavailable');
  trace.push({ clicked: 'Song Details' });
  await retry(() => click('Download LRC (Lyrics)', 'document'), 'The LRC button was unavailable');
  trace.push({ clicked: 'Download LRC (Lyrics)' });
  await retry(async () => {
    const file = await readFile(path.join(directory, 'Hosted LRC receipt.lrc'));
    return file.length > 0;
  }, 'A file was not downloaded');
} finally {
  await writeFile(path.join(directory, 'lrc-browser-trace.json'), JSON.stringify(trace, null, 2));
  void controlBrowser('Browser.close').catch(() => undefined);
  setTimeout(() => process.exit(process.exitCode ?? 0), 500);
}
