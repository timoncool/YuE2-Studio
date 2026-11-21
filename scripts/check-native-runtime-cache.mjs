import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { readFile, stat } from 'node:fs/promises';
import { join } from 'node:path';

const cache = process.env.STUDIO_NATIVE_RUNTIME_CACHE;
if (cache) {
  const os = { win32: 'windows', darwin: 'macos', linux: 'linux' }[process.platform];
  const arch = { x64: 'x86_64', arm64: 'aarch64' }[process.arch];
  const platform = `${os}-${arch}`;
  const pins = JSON.parse(await readFile(new URL(
    '../crates/music-server/src/native_music_runtime_assets.json', import.meta.url,
  ), 'utf8'));
  for (const pin of pins.filter(entry => entry.platform === platform)) {
    const path = join(cache, pin.archive);
    const info = await stat(path);
    if (!info.isFile() || info.size !== pin.bytes) throw new Error(`Invalid cache size for ${pin.archive}`);
    const hash = createHash('sha256');
    for await (const chunk of createReadStream(path)) hash.update(chunk);
    if (hash.digest('hex') !== pin.sha256) throw new Error(`Invalid cache SHA-256 for ${pin.archive}`);
    console.log(`Verified runtime cache for ${pin.engine} ${pin.revision} (${platform})`);
  }
}
