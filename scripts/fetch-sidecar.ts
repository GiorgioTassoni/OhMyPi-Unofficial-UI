#!/usr/bin/env bun
/**
 * Fetch, verify and stage the `omp` sidecar.
 *
 * The app bundles upstream's prebuilt standalone binary rather than building
 * one (`docs/11-v1-scope.md` §4 / D2), so this script is the supply-chain
 * boundary: it pins an exact release tag, verifies every download against a
 * digest recorded here, and refuses to stage anything that does not match.
 *
 *   bun scripts/fetch-sidecar.ts                 # stage the host target
 *   bun scripts/fetch-sidecar.ts --target linux-x64
 *   bun scripts/fetch-sidecar.ts --audit         # verify the manifest without downloading
 *   bun scripts/fetch-sidecar.ts --verify-staged # re-hash what is staged against the pin
 *   bun scripts/fetch-sidecar.ts --all           # stage every target (very large)
 *
 * The embedded digests are authoritative. The release's own SHA256SUMS.txt is
 * cross-checked as a second opinion, but a disagreement fails the run rather
 * than being resolved in the download's favour.
 */

import { mkdir } from "node:fs/promises";
import { dirname } from "node:path";

/** Pinned upstream release. Never a floating `latest`. */
const RELEASE_TAG = "v18.14.0";
const REPO = "can1357/oh-my-pi";
const RELEASE_BASE = `https://github.com/${REPO}/releases/download/${RELEASE_TAG}`;

/** One stageable target: the upstream asset and its recorded digest. */
interface Target {
	/** Upstream asset filename. */
	asset: string;
	/** Rust/Tauri target triple, used for the staged filename. */
	triple: string;
	/** SHA-256 recorded from the pinned release. */
	sha256: string;
	/** Size in bytes, for a cheap sanity check before hashing. */
	bytes: number;
}

/**
 * Every prebuilt asset published for the pinned release.
 *
 * Sizes and digests were read from the GitHub release API for `v18.14.0`; the
 * `--audit` mode re-verifies them against the release's SHA256SUMS.txt.
 */
const TARGETS: Record<string, Target> = {
	"darwin-arm64": {
		asset: "omp-darwin-arm64",
		triple: "aarch64-apple-darwin",
		sha256: "608843959ab34295d52460dd09dd7d548b6c903c08a83c67954c2d4c36fd1f5a",
		bytes: 214816912,
	},
	"darwin-x64": {
		asset: "omp-darwin-x64",
		triple: "x86_64-apple-darwin",
		sha256: "fc119dff47f99569e32b0a2b10c25db9733ac6faef6ca005539b6e6f610e5984",
		bytes: 226728400,
	},
	"linux-x64": {
		asset: "omp-linux-x64",
		triple: "x86_64-unknown-linux-gnu",
		sha256: "e7dd2b606c4ebe9d1a7bcfad1b8820db968cd6b05409fed1c6bc8db0b6a95a8f",
		bytes: 276244656,
	},
	"linux-arm64": {
		asset: "omp-linux-arm64",
		triple: "aarch64-unknown-linux-gnu",
		sha256: "dba8d170499709bc87359e02e677821aa76a6ef998ca0254578fe4ea07aff5bc",
		bytes: 236047352,
	},
	"linux-musl-x64": {
		asset: "omp-linux-musl-x64",
		triple: "x86_64-unknown-linux-musl",
		sha256: "794b2db4baf1d37bc11ba8ddcdf39d5bd4a9743849f9c5beee835efeb62949d6",
		bytes: 231409064,
	},
	"linux-musl-arm64": {
		asset: "omp-linux-musl-arm64",
		triple: "aarch64-unknown-linux-musl",
		sha256: "5b464a4a882c4c9f3576f7ef126756bd878f8e220f0340a9720bea3ed7b931dc",
		bytes: 228334696,
	},
	"windows-x64": {
		asset: "omp-windows-x64.exe",
		triple: "x86_64-pc-windows-msvc",
		sha256: "620a5935cdb6a9ed52d8b0c93892d6b5bb84155244a98f3be82af0455177bf29",
		bytes: 242797056,
	},
	"windows-arm64": {
		asset: "omp-windows-arm64.exe",
		triple: "aarch64-pc-windows-msvc",
		sha256: "759336455917ec0ed6c0d1da0e8bf13a69577fd3828e1a4c968fa068b3e73c1f",
		bytes: 225334784,
	},
};

/** Where Tauri expects `externalBin` entries to live. */
const BINARIES_DIR = "src-tauri/binaries";
/** Attribution we must ship alongside the bundled binary. */
const NOTICES_PATH = "src-tauri/THIRD-PARTY-NOTICES.txt";

function fail(message: string): never {
	process.stderr.write(`error: ${message}\n`);
	process.exit(1);
}

function parseArgs(argv: string[]): {
	targets: string[];
	audit: boolean;
	all: boolean;
	verifyStaged: boolean;
} {
	const targets: string[] = [];
	let audit = false;
	let all = false;
	let verifyStaged = false;

	for (let index = 0; index < argv.length; index += 1) {
		const arg = argv[index];
		if (arg === "--audit") {
			audit = true;
		} else if (arg === "--verify-staged") {
			verifyStaged = true;
		} else if (arg === "--all") {
			all = true;
		} else if (arg === "--target") {
			const value = argv[index + 1];
			if (!value) fail("--target requires a value");
			targets.push(value);
			index += 1;
		} else if (arg.startsWith("--target=")) {
			targets.push(arg.slice("--target=".length));
		} else {
			fail(`unknown argument: ${arg}`);
		}
	}

	return { targets, audit, all, verifyStaged };
}

/** Map the running host onto a manifest key, or null when unsupported. */
function hostTarget(): string | null {
	const platform = process.platform;
	const arch = process.arch;
	if (platform === "darwin" && arch === "arm64") return "darwin-arm64";
	if (platform === "darwin" && arch === "x64") return "darwin-x64";
	if (platform === "linux" && arch === "x64") return "linux-x64";
	if (platform === "linux" && arch === "arm64") return "linux-arm64";
	if (platform === "win32" && arch === "x64") return "windows-x64";
	if (platform === "win32" && arch === "arm64") return "windows-arm64";
	return null;
}

function sha256(bytes: Uint8Array): string {
	const hasher = new Bun.CryptoHasher("sha256");
	hasher.update(bytes);
	return hasher.digest("hex");
}

/** Download the release's own digest manifest, if it is available. */
async function fetchReleaseSums(): Promise<Map<string, string>> {
	const response = await fetch(`${RELEASE_BASE}/SHA256SUMS.txt`, { redirect: "follow" });
	if (!response.ok) fail(`could not fetch SHA256SUMS.txt (HTTP ${response.status})`);
	const text = await response.text();
	const sums = new Map<string, string>();
	for (const line of text.split("\n")) {
		const match = /^([0-9a-f]{64})\s+\*?(.+)$/i.exec(line.trim());
		if (match) sums.set(match[2].trim(), match[1].toLowerCase());
	}
	return sums;
}

/** Cross-check the embedded manifest against the release's digest file. */
async function audit(): Promise<void> {
	const sums = await fetchReleaseSums();
	let mismatches = 0;

	for (const [key, target] of Object.entries(TARGETS)) {
		const published = sums.get(target.asset);
		if (!published) {
			process.stdout.write(`?  ${key}: ${target.asset} is absent from SHA256SUMS.txt\n`);
			mismatches += 1;
			continue;
		}
		const matches = published === target.sha256;
		if (!matches) mismatches += 1;
		process.stdout.write(
			`${matches ? "ok" : "!!"} ${key.padEnd(18)} ${target.asset.padEnd(24)} ${target.sha256.slice(0, 16)}…\n`,
		);
	}

	if (mismatches > 0) fail(`${mismatches} manifest entr${mismatches === 1 ? "y" : "ies"} disagree with the release`);
	process.stdout.write(`\n${Object.keys(TARGETS).length} targets verified against ${RELEASE_TAG}\n`);
}

/** Download, verify and stage one target. */
async function stage(key: string, sums: Map<string, string> | null): Promise<void> {
	const target = TARGETS[key];
	if (!target) fail(`unknown target "${key}". Known: ${Object.keys(TARGETS).join(", ")}`);

	const url = `${RELEASE_BASE}/${target.asset}`;
	process.stdout.write(`→ ${key}: downloading ${target.asset} (${target.bytes} bytes)\n`);

	const response = await fetch(url, { redirect: "follow" });
	if (!response.ok) fail(`download failed for ${target.asset} (HTTP ${response.status})`);
	const bytes = new Uint8Array(await response.arrayBuffer());

	if (bytes.byteLength !== target.bytes) {
		fail(`size mismatch for ${target.asset}: expected ${target.bytes}, got ${bytes.byteLength}`);
	}

	const digest = sha256(bytes);
	if (digest !== target.sha256) {
		fail(
			`digest mismatch for ${target.asset}\n  expected ${target.sha256}\n  actual   ${digest}\n` +
				`Refusing to stage an unverified binary.`,
		);
	}

	if (sums) {
		const published = sums.get(target.asset);
		if (published && published !== digest) {
			fail(`the release's SHA256SUMS.txt disagrees with the embedded digest for ${target.asset}`);
		}
	}

	const destination = stagedPath(key, target);
	await mkdir(dirname(destination), { recursive: true });
	await Bun.write(destination, bytes);
	if (!key.startsWith("windows")) await Bun.$`chmod +x ${destination}`.quiet();
	process.stdout.write(`✓ staged ${destination}\n`);
}

/**
 * Where one target's binary lives once staged.
 *
 * The upstream asset *is* the binary — measured: the release publishes one executable per
 * triple, not an archive — which is why one digest answers both "is the download right" and
 * "is what we are about to bundle right", and why there is no extracted form to hash.
 */
function stagedPath(key: string, target: Target): string {
	return `${BINARIES_DIR}/omp-${target.triple}${key.startsWith("windows") ? ".exe" : ""}`;
}

/**
 * Re-hash what is staged and compare it with the pin, without the network.
 *
 * This is what a release build runs before bundling (`tauri.conf.json`'s `beforeBuildCommand`):
 * `docs/14` requires the build to fail on a mismatch, and re-downloading to prove it would make
 * every offline build fail instead. A binary nobody has staged is a failure here rather than a
 * bundling error three minutes later.
 */
async function verifyStaged(keys: string[]): Promise<void> {
	for (const key of keys) {
		const target = TARGETS[key];
		if (!target) fail(`unknown target "${key}". Known: ${Object.keys(TARGETS).join(", ")}`);

		const staged = stagedPath(key, target);
		const file = Bun.file(staged);
		if (!(await file.exists())) {
			fail(`${staged} is not staged\n  run: bun scripts/fetch-sidecar.ts --target ${key}`);
		}

		const digest = sha256(new Uint8Array(await file.arrayBuffer()));
		if (digest !== target.sha256) {
			fail(
				`the staged sidecar does not match the pin\n  ${staged}\n  expected ${target.sha256}\n  actual   ${digest}\n` +
					`Refusing to build against a binary that is not the pinned release.`,
			);
		}

		process.stdout.write(`ok ${staged}\n`);
	}

	process.stdout.write(
		`\n${keys.length} staged sidecar${keys.length === 1 ? "" : "s"} verified against the pin\n`,
	);
}

/** Fetch the upstream attribution file we must ship with the binary. */
async function stageNotices(): Promise<void> {
	const response = await fetch(`${RELEASE_BASE}/THIRD-PARTY-NOTICES.txt`, { redirect: "follow" });
	if (!response.ok) {
		const licenseResp = await fetch(`${RELEASE_BASE}/LICENSE`, { redirect: "follow" });
		if (licenseResp.ok) {
			await Bun.write(NOTICES_PATH, await licenseResp.text());
			process.stdout.write(`✓ staged ${NOTICES_PATH} (from LICENSE)\n`);
			return;
		}
		fail(`could not fetch THIRD-PARTY-NOTICES.txt or LICENSE (HTTP ${response.status})`);
	}
	await Bun.write(NOTICES_PATH, await response.text());
	process.stdout.write(`✓ staged ${NOTICES_PATH}\n`);
}

const { targets, audit: auditOnly, all, verifyStaged: verifyOnly } = parseArgs(process.argv.slice(2));

if (auditOnly && verifyOnly) fail("--audit and --verify-staged are different questions; ask one");

if (verifyOnly) {
	await verifyStaged(all ? Object.keys(TARGETS) : targets.length > 0 ? targets : [hostTarget() ?? fail(`unsupported host ${process.platform}/${process.arch}; pass --target`)]);
} else if (auditOnly) {
	await audit();
} else {
	const selected = all ? Object.keys(TARGETS) : targets.length > 0 ? targets : [hostTarget() ?? fail(`unsupported host ${process.platform}/${process.arch}; pass --target`)];
	const sums = await fetchReleaseSums().catch(() => null);
	await stageNotices();
	for (const key of selected) {
		await stage(key, sums);
	}
	process.stdout.write(`\nDone. ${RELEASE_TAG} sidecar(s) staged under ${BINARIES_DIR}/.\n`);
}
