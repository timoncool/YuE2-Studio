import { writeFile } from 'node:fs/promises';

const [output] = process.argv.slice(2);
const pages = await fetch('http://127.0.0.1:9889/json/list').then(response => response.json());
const page = pages.find(item => item.type === 'page' && item.url.startsWith('http://tauri.localhost/'));
if (!page) throw new Error('Installed Studio WebView was not found');
const socket = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((resolve, reject) => {
  socket.addEventListener('open', resolve, { once: true });
  socket.addEventListener('error', reject, { once: true });
});
let counter = 0;
function call(method, params) {
  return new Promise((resolve, reject) => {
    const id = ++counter;
    const timer = setTimeout(() => reject(new Error(`CDP timed out: ${method}`)), 10000);
    const listener = event => {
      const message = JSON.parse(event.data);
      if (message.id !== id) return;
      clearTimeout(timer);
      socket.removeEventListener('message', listener);
      if (message.error || message.result?.exceptionDetails) reject(new Error(JSON.stringify(message)));
      else resolve(message.result);
    };
    socket.addEventListener('message', listener);
    socket.send(JSON.stringify({ id, method, params }));
  });
}
try {
  const { result } = await call('Runtime.evaluate', {
    returnByValue: true,
    expression: `(() => {
      const dialog = document.querySelector('dialog.studio-songbook:modal');
      if (!dialog) throw new Error('Songbook is not in the modal top layer');
      const { x, y, width, height } = dialog.getBoundingClientRect();
      const center = document.elementFromPoint(x + width / 2, y + height / 2);
      if (!dialog.contains(center)) throw new Error('Songbook is hidden');
      return { text: dialog.innerText, dimensions: [width, height], diagrams: dialog.querySelectorAll('.songbook-shape svg').length };
    })()`,
  });
  const { text, diagrams } = result.value;
  if (!['Harbor Lights', 'Cmaj7', 'Print A4'].every(term => text.includes(term)) || diagrams < 2) {
    throw new Error(JSON.stringify(result.value));
  }
  const shot = await call('Page.captureScreenshot', { format: 'png', fromSurface: true });
  await writeFile(output, Buffer.from(shot.data, 'base64'));
  await writeFile(output + '.json', JSON.stringify(result.value, null, 2));
} finally {
  socket.close();
}
