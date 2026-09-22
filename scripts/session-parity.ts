/**
 * Does the app read the session store the way the engine does?
 *
 * The sidebar shows the same sessions the engine's own resume picker shows, so two
 * implementations that disagree would be a bug the user sees as a wrong badge — and this
 * is not hypothetical: the first run of this script found that `omp-store` derived a
 * small session's lifecycle from its *first* message instead of its last, disagreeing with
 * the engine on 10 of 29 sessions (a 4 KB prefix used as if it were a tail).
 *
 * What it does:
 *   1. runs `omp-store`'s ignored live test, which scans the real store and dumps JSON
 *   2. runs the engine's own `listSessionsReadOnly` over the same buckets
 *   3. compares field by field and exits non-zero on any difference
 *
 * The engine side is deliberately the **read-only** scan: `listAllSessions` repairs
 * orphaned `.bak` files, and a check must never write to the user's store.
 *
 * Usage: `bun scripts/session-parity.ts`
 *
 * Two differences are expected and reported, not failed: a session file that is being
 * written while this runs (its `size`, and possibly its last message) — the live session
 * this app is talking to.
 */

import { spawnSync } from "node:child_process";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

const repo = path.dirname(import.meta.dir);
const dumpPath = path.join(os.tmpdir(), "omp-store-dump.json");

/** The engine's package root, from wherever `omp` on PATH actually points. */
function engineRoot(): string {
  const omp = Bun.which("omp");
  if (!omp) throw new Error("`omp` is not on PATH");
  const cli = fs.realpathSync(omp);
  // `<root>/dist/cli.js`
  return path.dirname(path.dirname(cli));
}

async function appScan(): Promise<Record<string, unknown>[]> {
  fs.rmSync(dumpPath, { force: true });
  const run = spawnSync(
    "cargo",
    ["test", "-p", "omp-store", "--test", "listing", "--", "--ignored", "--nocapture", "the_real_store"],
    { cwd: repo, encoding: "utf-8" },
  );
  if (run.status !== 0) {
    throw new Error(`the catalogue's own scan failed:\n${run.stdout}\n${run.stderr}`);
  }
  if (!fs.existsSync(dumpPath)) {
    throw new Error(`the scan produced no dump at ${dumpPath}`);
  }
  return JSON.parse(fs.readFileSync(dumpPath, "utf-8"));
}

async function engineScan(root: string): Promise<Record<string, unknown>[]> {
  const { FileSessionStorage } = await import(`${root}/src/session/session-storage.ts`);
  const { listSessionsReadOnly } = await import(`${root}/src/session/session-listing.ts`);
  const { getSessionsDir } = await import(
    `${path.dirname(root)}/pi-utils/src/dirs.ts`
  );

  const sessionsRoot = getSessionsDir();
  const storage = new FileSessionStorage();
  const buckets = fs
    .readdirSync(sessionsRoot)
    .filter((name) => fs.statSync(path.join(sessionsRoot, name)).isDirectory());

  const all: Record<string, unknown>[] = [];
  for (const bucket of buckets) {
    for (const info of await listSessionsReadOnly(path.join(sessionsRoot, bucket), storage)) {
      const parentPath: string | undefined = info.parentSessionPath;
      all.push({
        id: info.id,
        status: info.status,
        title: info.title ?? null,
        firstMessage: info.firstMessage,
        cwd: info.cwd,
        parentId: parentPath
          ? parentPath.includes("/")
            ? parentPath.slice(parentPath.lastIndexOf("_") + 1, -".jsonl".length) || null
            : parentPath
          : null,
        messageCount: info.messageCount,
        size: info.size,
      });
    }
  }
  return all;
}

/** The fields that must match. `size` is excluded from the verdict: a session being written
 * moves it between the two scans. */
const FIELDS = ["status", "title", "firstMessage", "cwd", "parentId", "messageCount"] as const;

const root = engineRoot();
const app = new Map((await appScan()).map((row) => [row.id as string, row]));
const engine = new Map((await engineScan(root)).map((row) => [row.id as string, row]));

console.log(`engine: ${root}`);
console.log(`sessions: app ${app.size}, engine ${engine.size}`);

let failures = 0;
for (const id of new Set([...app.keys(), ...engine.keys()])) {
  const mine = app.get(id);
  const theirs = engine.get(id);
  if (!mine || !theirs) {
    failures += 1;
    console.log(`MISSING ${id}: app=${!!mine} engine=${!!theirs}`);
    continue;
  }
  for (const field of FIELDS) {
    if (mine[field] !== theirs[field]) {
      failures += 1;
      console.log(`DIFF ${id.slice(0, 8)} ${field}: app=${JSON.stringify(mine[field])} engine=${JSON.stringify(theirs[field])}`);
    }
  }
  if (mine.size !== theirs.size) {
    console.log(`note ${id.slice(0, 8)} size moved: app=${mine.size} engine=${theirs.size} (a session being written)`);
  }
}

if (failures > 0) {
  console.error(`\n${failures} difference(s): the app's catalogue does not match the engine's.`);
  process.exit(1);
}
console.log(`\nthe catalogue matches the engine on all ${app.size} sessions`);
