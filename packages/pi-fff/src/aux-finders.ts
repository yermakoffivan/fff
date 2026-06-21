import fs from "node:fs";
import path from "node:path";
import { FileFinder } from "@ff-labs/fff-node";

// Hotfix prototype for issue #463: agent occasionally needs to grep/find
// outside the workspace cwd. Maintain a tiny rotating pool of additional
// FileFinder instances rooted at out-of-workspace paths. LRU-evicted at
// MAX_AUX, dropped after IDLE_TTL_MS of no use.

export const MAX_AUX = 3;
export const IDLE_TTL_MS = 5 * 60 * 1000;

interface AuxEntry {
  root: string;
  finder: FileFinder;
  lastUsed: number;
}

export interface AuxOpts {
  frecencyDbPath?: string;
  historyDbPath?: string;
  enableFsRootScanning: boolean;
}

export class AuxFinderPool {
  private entries: AuxEntry[] = [];
  constructor(private opts: AuxOpts) {}

  private sweepIdle(now = Date.now()): void {
    const kept: AuxEntry[] = [];
    for (const e of this.entries) {
      if (now - e.lastUsed > IDLE_TTL_MS) {
        if (!e.finder.isDestroyed) e.finder.destroy();
      } else {
        kept.push(e);
      }
    }
    this.entries = kept;
  }

  async acquire(root: string): Promise<FileFinder> {
    this.sweepIdle();
    const existing = this.entries.find((e) => e.root === root);
    if (existing && !existing.finder.isDestroyed) {
      existing.lastUsed = Date.now();
      return existing.finder;
    }
    if (this.entries.length >= MAX_AUX) {
      let oldest = this.entries[0];
      for (const e of this.entries) if (e.lastUsed < oldest.lastUsed) oldest = e;
      if (!oldest.finder.isDestroyed) oldest.finder.destroy();
      this.entries = this.entries.filter((e) => e !== oldest);
    }
    const result = FileFinder.create({
      basePath: root,
      frecencyDbPath: this.opts.frecencyDbPath,
      historyDbPath: this.opts.historyDbPath,
      aiMode: true,
      enableHomeDirScanning: true,
      enableFsRootScanning: this.opts.enableFsRootScanning,
    });
    if (!result.ok)
      throw new Error(`Failed to create aux file finder for ${root}: ${result.error}`);
    await result.value.waitForScan(15000);
    this.entries.push({ root, finder: result.value, lastUsed: Date.now() });
    return result.value;
  }

  destroyAll(): void {
    for (const e of this.entries) if (!e.finder.isDestroyed) e.finder.destroy();
    this.entries = [];
  }

  // Exposed for tests/diagnostics.
  size(): number {
    this.sweepIdle();
    return this.entries.length;
  }
}

// Split an absolute path into the longest non-glob directory prefix (the aux
// root) and a remainder usable as a path constraint relative to that root.
// Returns null if the prefix doesn't exist on disk.
export function resolveAuxRoot(
  absPath: string,
): { root: string; suffix: string } | null {
  const normalized = path.normalize(absPath.trim());
  if (!path.isAbsolute(normalized)) return null;
  const parts = normalized.split(path.sep);
  const firstGlob = parts.findIndex((p) => /[*?[{]/.test(p));
  let rootPath: string;
  let suffix: string;
  if (firstGlob === -1) {
    rootPath = normalized;
    suffix = "";
  } else {
    rootPath = parts.slice(0, firstGlob).join(path.sep) || path.sep;
    suffix = parts.slice(firstGlob).join("/");
  }
  const stripped = rootPath.replace(/\/+$/, "") || "/";
  let stat: fs.Stats;
  try {
    stat = fs.statSync(stripped);
  } catch {
    return null;
  }
  if (stat.isFile()) {
    return { root: path.dirname(stripped), suffix: path.basename(stripped) };
  }
  return { root: stripped, suffix };
}

// Decide whether a `path` parameter should route to the workspace finder or
// to an aux finder. An absolute path is "outside workspace" when the relative
// from cwd starts with `..`. Returns null to signal "no rerouting".
export function routePathConstraint(
  pathConstraint: string | undefined,
  cwd: string,
): { root: string; suffix: string } | null {
  if (!pathConstraint) return null;
  const trimmed = pathConstraint.trim();
  if (!trimmed || !path.isAbsolute(trimmed)) return null;
  const rel = path.relative(cwd, trimmed);
  if (!rel.startsWith("..") && rel !== "..") return null;
  return resolveAuxRoot(trimmed);
}
