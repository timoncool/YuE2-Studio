import { execFileSync } from 'node:child_process';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

// Resolve Cargo's target directory from the desktop workspace, including its
// configuration and any shared cache selected by the build environment.
const workspace = fileURLToPath(new URL('../desktop/src-tauri/', import.meta.url));
const metadata = JSON.parse(execFileSync('cargo', ['metadata', '--no-deps', '--format-version', '1'], {
  cwd: workspace,
  encoding: 'utf8',
}));
console.log(join(metadata.target_directory, process.env.CARGO_BUILD_TARGET || '', 'release'));
