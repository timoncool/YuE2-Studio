import { createHash } from 'node:crypto';
import { writeFile } from 'node:fs/promises';
import path from 'node:path';

const [mode, output] = process.argv.slice(2);
const list = await fetch('http://127.0.0.1:9889/json/list').then(reply => reply.json());
const page = list.find(target => target.type === 'page' && target.url.startsWith('http://tauri.localhost/'));
if (!page) throw new Error('Installed Studio WebView is missing');
const socket = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((resolve, reject) => {
  socket.addEventListener('open', resolve, { once: true });
  socket.addEventListener('error', reject, { once: true });
});
const pending = new Map();
let sequence = 0;
socket.addEventListener('message', event => {
  const reply = JSON.parse(event.data);
  const request = pending.get(reply.id);
  if (!request) return;
  pending.delete(reply.id);
  if (reply.error) request.reject(reply.error);
  else request.resolve(reply.result);
});
function call(method, params = {}) {
  return new Promise((resolve, reject) => {
    const id = ++sequence;
    pending.set(id, { resolve, reject });
    socket.send(JSON.stringify({ id, method, params }));
  });
}

try {
  const hashes = new Set();
  const frames = [];
  for (let sample = 0; sample < 16 && hashes.size < 2; sample++) {
    const selector = mode === 'milkdrop' ? 'canvas[data-webgl]' : 'canvas:not([data-webgl])';
    const { result } = await call('Runtime.evaluate', { returnByValue: true, expression: `(() => {
      const boxes = [...document.querySelectorAll(${JSON.stringify(selector)})]
        .map(canvas => canvas.getBoundingClientRect())
        .filter(rect => rect.width >= 120 && rect.height >= 80 && rect.left >= 0 && rect.top >= 0
                      && rect.right <= innerWidth && rect.bottom <= innerHeight)
        .sort((a,b) => b.width*b.height - a.width*a.height);
      const box = boxes[0];
      return box && { x: box.x + 20, y: box.y + 40, width: box.width - 40, height: box.height - 60, scale: 1 };
    })()` });
    if (result.value) {
      const { data } = await call('Page.captureScreenshot', { format: 'png', clip: result.value });
      const pixels = Buffer.from(data, 'base64');
      const sha256 = createHash('sha256').update(pixels).digest('hex');
      if (pixels.length > 2000 && !hashes.has(sha256)) {
        hashes.add(sha256);
        const name = `visualizer-${mode}-${frames.length + 1}.png`;
        await writeFile(path.join(output, name), pixels);
        frames.push({ name, sha256, bytes: pixels.length, bounds: result.value });
      }
    }
    await new Promise(resolve => setTimeout(resolve, 220));
  }
  await writeFile(path.join(output, `${mode}-frames.json`), JSON.stringify(frames, null, 2));
  if (frames.length < 2) throw new Error(`${mode}: two rendered canvas frames were not observed`);
} finally {
  socket.close();
}
