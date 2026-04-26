#!/usr/bin/env node
// FluxMirror cross-shell hook entry point (node variant).
//
// Usage:
//   node shim.mjs <kind>                  # kind = claude | gemini
//   FLUXMIRROR_KIND=<kind> node shim.mjs  # env override (lower priority than argv)
//
// Auto-downloads the per-arch fluxmirror binary from the latest GitHub
// release on first invocation, caches it under FLUXMIRROR_CACHE
// (default ~/.fluxmirror/cache on POSIX, %LOCALAPPDATA%\fluxmirror\cache
// on Windows), then spawns `fluxmirror hook --kind <kind>` with stdio
// inherited so the JSON tool-call payload on stdin reaches the binary.
//
// IMPORTANT: any failure in detection, download, or exec must NOT propagate
// to the calling agent. We always exit 0.

import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, chmodSync, renameSync, unlinkSync, createWriteStream } from 'node:fs';
import { get as httpsGet } from 'node:https';
import { homedir } from 'node:os';
import { join } from 'node:path';

const KIND = process.argv[2] || process.env.FLUXMIRROR_KIND || 'claude';

function cacheDir() {
  if (process.env.FLUXMIRROR_CACHE) return process.env.FLUXMIRROR_CACHE;
  if (process.platform === 'win32') {
    const base = process.env.LOCALAPPDATA || homedir();
    return join(base, 'fluxmirror', 'cache');
  }
  return join(homedir(), '.fluxmirror', 'cache');
}

function assetName() {
  let os;
  switch (process.platform) {
    case 'darwin': os = 'darwin'; break;
    case 'linux':  os = 'linux';  break;
    case 'win32':  os = 'windows'; break;
    default: return null;
  }
  let arch;
  switch (process.arch) {
    case 'arm64': arch = 'arm64'; break;
    case 'x64':   arch = 'x64';   break;
    default: return null;
  }
  const suffix = os === 'windows' ? '.exe' : '';
  return `fluxmirror-${os}-${arch}${suffix}`;
}

function download(url, dest) {
  return new Promise((resolve) => {
    const tmp = `${dest}.tmp`;
    let file;
    try {
      file = createWriteStream(tmp);
    } catch {
      resolve(false);
      return;
    }
    const cleanup = () => {
      try { file.close(); } catch {}
      try { unlinkSync(tmp); } catch {}
    };
    const fetch = (u, depth) => {
      if (depth > 5) { cleanup(); resolve(false); return; }
      const req = httpsGet(u, (res) => {
        if (res.statusCode && res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
          res.resume();
          fetch(res.headers.location, depth + 1);
          return;
        }
        if (res.statusCode !== 200) { res.resume(); cleanup(); resolve(false); return; }
        res.pipe(file);
        file.on('finish', () => {
          file.close(() => {
            try {
              renameSync(tmp, dest);
              resolve(true);
            } catch {
              cleanup();
              resolve(false);
            }
          });
        });
        res.on('error', () => { cleanup(); resolve(false); });
      });
      req.setTimeout(15000, () => { req.destroy(); cleanup(); resolve(false); });
      req.on('error', () => { cleanup(); resolve(false); });
    };
    fetch(url, 0);
  });
}

async function main() {
  const asset = assetName();
  if (!asset) process.exit(0);

  const dir = cacheDir();
  try { mkdirSync(dir, { recursive: true }); } catch {}

  const bin = join(dir, asset);

  if (!existsSync(bin)) {
    const url = `https://github.com/OpenFluxGate/fluxmirror/releases/latest/download/${asset}`;
    const ok = await download(url, bin);
    if (!ok) process.exit(0);
    if (process.platform !== 'win32') {
      try { chmodSync(bin, 0o755); } catch {}
    }
  }

  try {
    spawnSync(bin, ['hook', '--kind', KIND], { stdio: ['inherit', 'inherit', 'inherit'] });
  } catch {}
  process.exit(0);
}

main().catch(() => process.exit(0));
