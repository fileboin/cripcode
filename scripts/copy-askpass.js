/**
 * Copy the cargo-built ssh-askpass helper into `src-tauri/binaries/` with
 * the target-triple filename Tauri's `bundle.externalBin` expects. Runs as
 * `beforeBundleCommand` during `tauri build` (after the release cargo build
 * has produced `target/release/ssh-askpass(.exe)`), so password-mode SSH
 * works in the installed app: Tauri places the helper next to the main
 * executable, which is exactly where the backend resolves it.
 */
const { existsSync, mkdirSync, copyFileSync } = require('fs');
const path = require('path');
const { execSync } = require('child_process');

const repoRoot = path.resolve(__dirname, '..');

function detectTriple() {
  if (process.env.TAURI_ENV_TARGET_TRIPLE) return process.env.TAURI_ENV_TARGET_TRIPLE;
  const out = execSync('rustc -vV', { encoding: 'utf8' });
  const hostLine = out.split('\n').find((line) => line.startsWith('host:'));
  if (!hostLine) throw new Error('cannot determine the Rust target triple');
  return hostLine.slice('host:'.length).trim();
}

const ext = process.platform === 'win32' ? '.exe' : '';
const helperName = `ssh-askpass${ext}`;
const triple = detectTriple();

const src = path.join(repoRoot, 'src-tauri', 'target', 'release', helperName);
const outDir = path.join(repoRoot, 'src-tauri', 'binaries');
const dest = path.join(outDir, `ssh-askpass-${triple}${ext}`);

if (!existsSync(src)) {
  console.error(`[copy-askpass] helper not built: ${src}`);
  console.error('[copy-askpass] run the release cargo build before bundling');
  process.exit(1);
}

mkdirSync(outDir, { recursive: true });
copyFileSync(src, dest);
console.log(`[copy-askpass] ${src} -> ${dest}`);
