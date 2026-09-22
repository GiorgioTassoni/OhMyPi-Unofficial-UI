#!/usr/bin/env bun
/**
 * Generates `crates/omp-settings/catalog.json` — the machine-readable copy of the settings
 * schema's *presentation* half that the desktop settings screen renders from.
 *
 * Sources, each authoritative for one thing (see `docs/13-settings-mapping.md` and the
 * generator contract):
 *
 *   - the pinned tarball  `package/src/config/settings-schema.ts` — the 45 `ui.condition` names
 *     (and, for resolving a doc/engine disagreement, the schema's own declared defaults);
 *   - the pinned sidecar  `omp config list --json`  — key set, `type`, `description`, and which
 *     values are unset;
 *   - the pinned sidecar  `omp config list`          — enum domains (its own type display) and the
 *     engine's tab grouping;
 *   - `docs/13-settings-mapping.md`                  — section, group, control, restart class,
 *     disposition, credentials, the danger list, and each section's blurb.
 *
 * Usage:
 *   bun scripts/gen-settings-catalog.ts                 # write the catalog
 *   bun scripts/gen-settings-catalog.ts --check         # recompute, diff, exit non-zero on drift
 *   bun scripts/gen-settings-catalog.ts --package <dir> # read an installed copy of the package
 *
 * The output is deterministic: doc order, stable key order, no timestamps, no machine paths.
 */
import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";

const REPO_ROOT = resolve(import.meta.dir, "..");
const ENGINE_VERSION = "18.2.6";
const TARBALL_URL = `https://registry.npmjs.org/@oh-my-pi/pi-coding-agent/-/pi-coding-agent-${ENGINE_VERSION}.tgz`;
const SCHEMA_MEMBER = "package/src/config/settings-schema.ts";
const UI_MEMBER = "package/src/config/settings-ui.ts";
const SCHEMA_REL = join("src", "config", "settings-schema.ts");
const UI_REL = join("src", "config", "settings-ui.ts");
const SIDECAR_REL = join("src-tauri", "binaries", "omp-x86_64-unknown-linux-gnu");
const DOC_REL = join("docs", "13-settings-mapping.md");
const OUT_REL = join("crates", "omp-settings", "catalog.json");

/** Cross-check numbers, all of which must hold or the generator refuses to emit a catalog. */
const EXPECT = {
	rows: 505,
	distinct: 505,
	sections: 16,
	curated: 309,
	deferred: 46,
	hidden: 150,
	enums: 91,
	credentials: 8,
	danger: 30,
	dangerConfirm: 22,
	dangerWarn: 8,
	conditions: 45,
	conditionNames: 12,
	live: 6,
	app: 13,
} as const;

/**
 * The only keys the app can push into a *running* session: one RPC setter each, from the Mode
 * and Effort popovers — `set_steering_mode`, `set_follow_up_mode`, `set_interrupt_mode`,
 * `set_thinking_level`, `set_auto_compaction`, `set_auto_retry`
 * (`modes/rpc/rpc-mode.ts:1315-1490`).
 *
 * docs/13 classes eleven rows `RPC live`, but that class is broader than the app's reach: the
 * five `tier.*` rows are read per request *inside* the process (they are `set_fast_mode`'s
 * world), and an out-of-process config write never reaches them — measured, see the
 * generator contract's §"The measurement that fixes `restart`". Those five are `sidecar`.
 */
const RPC_LIVE_KEYS: Record<string, true> = {
	steeringMode: true,
	followUpMode: true,
	interruptMode: true,
	defaultThinkingLevel: true,
	"compaction.enabled": true,
	"retry.enabled": true,
};

/** App-owned UI policy: which nav group a section belongs to, and its icon. */
const SECTION_CHROME: Record<string, { navGroup: "app" | "agent" | "system"; icon: string }> = {
	General: { navGroup: "app", icon: "sliders" },
	Appearance: { navGroup: "app", icon: "sparkle" },
	"Shortcuts & Keys": { navGroup: "app", icon: "keyboard" },
	Notifications: { navGroup: "app", icon: "bell" },
	"Usage & Limits": { navGroup: "app", icon: "gauge" },
	"Model & Providers": { navGroup: "agent", icon: "sparkle" },
	Agent: { navGroup: "agent", icon: "bot" },
	"Context & Compaction": { navGroup: "agent", icon: "layers" },
	Memory: { navGroup: "agent", icon: "brain" },
	Skills: { navGroup: "agent", icon: "wand" },
	MCP: { navGroup: "agent", icon: "plug" },
	LSP: { navGroup: "agent", icon: "code" },
	Session: { navGroup: "agent", icon: "clock" },
	"Terminal & Shell": { navGroup: "agent", icon: "terminal" },
	Advanced: { navGroup: "system", icon: "wrench" },
	"Raw config": { navGroup: "system", icon: "scroll" },
};

const TYPES: Record<string, true> = {
	boolean: true,
	number: true,
	enum: true,
	string: true,
	array: true,
	record: true,
};
const CONTROLS: Record<string, true> = {
	toggle: true,
	select: true,
	number: true,
	text: true,
	list: true,
	"record/JSON": true,
	secret: true,
};
/** docs/13 "Live vs restart" classes. `[UNVERIFIED]` is the one row the doc could not classify. */
const LIVE_CLASSES: Record<string, true> = {
	"RPC live": true,
	"next call": true,
	"sidecar restart": true,
	"app restart": true,
	"[UNVERIFIED]": true,
};
const DISPOSITIONS: Record<string, "curated" | "deferred" | "hidden"> = {
	yes: "curated",
	deferred: "deferred",
	hidden: "hidden",
};
/** Whole-word acronyms kept upper-case when humanising a key segment or an enum value. */
const ACRONYMS: Record<string, true> = {
	gc: true,
	lsp: true,
	mcp: true,
	tts: true,
	stt: true,
	url: true,
	json: true,
	api: true,
	id: true,
	cdp: true,
	db: true,
	os: true,
	ttl: true,
	http: true,
	ansi: true,
	ascii: true,
	sql: true,
	wal: true,
	ast: true,
	ci: true,
	dap: true,
};

function fail(message: string): never {
	console.error(`gen-settings-catalog: ${message}`);
	process.exit(1);
}

function note(message: string): void {
	console.error(`gen-settings-catalog: note: ${message}`);
}

/* ------------------------------------------------------------------ inputs */

interface Args {
	check: boolean;
	packageDir: string | null;
}

function parseArgs(argv: string[]): Args {
	const args: Args = { check: false, packageDir: null };
	for (let i = 0; i < argv.length; i++) {
		if (argv[i] === "--check") args.check = true;
		else if (argv[i] === "--package") {
			const dir = argv[++i];
			if (!dir) fail("--package needs a directory");
			args.packageDir = resolve(REPO_ROOT, dir);
		} else fail(`unknown argument: ${argv[i]}`);
	}
	return args;
}

/** Strip ANSI escapes; refuse to parse output that still carries any (terminal-dependence). */
function stripAnsi(raw: string, label: string): string {
	const stripped = raw.replace(/\u001b\][^\u0007\u001b]*(?:\u0007|\u001b\\)/g, "").replace(/\u001b\[[0-?]*[ -/]*[@-~]/g, "");
	if (stripped.includes("\u001b")) fail(`${label}: output still carries ANSI escapes after stripping`);
	return stripped.replace(/\r\n/g, "\n");
}

function runEngine(args: string[], what: string): string {
	const binary = resolve(REPO_ROOT, SIDECAR_REL);
	const proc = Bun.spawnSync([binary, "config", ...args], { cwd: REPO_ROOT, stdout: "pipe", stderr: "pipe" });
	const out = proc.stdout.toString();
	const err = proc.stderr.toString();
	if (proc.exitCode !== 0) fail(`${what} failed (exit ${proc.exitCode}): ${stripAnsi(err, what).trim()}`);
	return stripAnsi(out, what);
}

interface PackageSources {
	schema: string;
	ui: string;
}

async function loadPackageSources(packageDir: string | null): Promise<PackageSources> {
	if (packageDir) {
		const manifestPath = join(packageDir, "package.json");
		if (!existsSync(manifestPath)) fail(`--package ${packageDir}: no package.json`);
		const version = (JSON.parse(await readFile(manifestPath, "utf8")) as { version?: string }).version;
		if (version !== ENGINE_VERSION) fail(`--package ${packageDir}: package.json version is ${version}, expected ${ENGINE_VERSION}`);
		const schemaPath = join(packageDir, SCHEMA_REL);
		const uiPath = join(packageDir, UI_REL);
		if (!existsSync(schemaPath)) fail(`--package ${packageDir}: missing ${SCHEMA_REL}`);
		if (!existsSync(uiPath)) fail(`--package ${packageDir}: missing ${UI_REL}`);
		return {
			schema: await readFile(schemaPath, "utf8"),
			ui: await readFile(uiPath, "utf8"),
		};
	}
	const sandbox = await mkdtemp(join(tmpdir(), "omp-settings-catalog-"));
	try {
		const response = await fetch(TARBALL_URL);
		if (!response.ok) fail(`failed to download ${TARBALL_URL}: HTTP ${response.status} ${response.statusText}`);
		const tarball = join(sandbox, `pi-coding-agent-${ENGINE_VERSION}.tgz`);
		await writeFile(tarball, Buffer.from(await response.arrayBuffer()));
		const extract = (member: string): string => {
			const proc = Bun.spawnSync(["tar", "-xzOf", tarball, member], { stdout: "pipe", stderr: "pipe" });
			if (proc.exitCode !== 0) fail(`tar -xzOf ${member} failed: ${proc.stderr.toString().trim()}`);
			return proc.stdout.toString();
		};
		return { schema: extract(SCHEMA_MEMBER), ui: extract(UI_MEMBER) };
	} finally {
		await rm(sandbox, { recursive: true, force: true });
	}
}

/* ------------------------------------------------------------- small tools */

/** camelCase / snake_case / kebab-case → words. */
function words(segment: string): string[] {
	return segment
		.replace(/([a-z0-9])([A-Z])/g, "$1 $2")
		.replace(/([A-Z]+)([A-Z][a-z])/g, "$1 $2")
		.split(/[^A-Za-z0-9]+/)
		.filter(Boolean);
}

/** `sleepPrevention` → `Sleep prevention`, `wal` → `WAL`, `always-ask` → `Always ask`. */
function humanise(segment: string): string {
	const parts = words(segment).map((w) => (ACRONYMS[w.toLowerCase()] === true ? w.toUpperCase() : w.toLowerCase()));
	if (parts.length === 0) return segment;
	const joined = parts.join(" ");
	return joined[0].toUpperCase() + joined.slice(1);
}

type Literal = { kind: "literal"; value: unknown } | { kind: "nonliteral"; raw: string } | { kind: "interpolated"; raw: string };

function deepEqual(a: unknown, b: unknown): boolean {
	if (Array.isArray(a) || Array.isArray(b)) {
		return Array.isArray(a) && Array.isArray(b) && a.length === b.length && a.every((v, i) => deepEqual(v, b[i]));
	}
	if (a && b && typeof a === "object" && typeof b === "object") {
		const ka = Object.keys(a as object).sort();
		const kb = Object.keys(b as object).sort();
		return ka.length === kb.length && ka.every((k, i) => k === kb[i] && deepEqual((a as Record<string, unknown>)[k], (b as Record<string, unknown>)[k]));
	}
	if (typeof a === "number" && typeof b === "number") return a === b;
	return typeof a === typeof b && a === b;
}

/**
 * Parse a TS string/number/boolean/JSON literal into a value. Anything else (a schema constant
 * such as `EMPTY_STRING_ARRAY`, arithmetic like `4 * 1024 * 1024`, an `as const` cast, a
 * template literal with interpolation) comes back as `nonliteral`/`interpolated`.
 */
function parseLiteral(raw: string): Literal {
	const text = raw.trim().replace(/,\s*$/, "").trim();
	if (text === "") return { kind: "nonliteral", raw };
	if (text === "undefined") return { kind: "nonliteral", raw };
	if (text === "true") return { kind: "literal", value: true };
	if (text === "false") return { kind: "literal", value: false };
	const numeric = text.replace(/_/g, "");
	if (/^-?\d+(\.\d+)?$/.test(numeric)) return { kind: "literal", value: Number(numeric) };
	const quote = text[0];
	if (quote === '"' || quote === "'" || quote === "`") {
		if (!text.endsWith(quote) || text.length < 2) return { kind: "nonliteral", raw };
		const inner = text.slice(1, -1);
		if (quote === "`" && inner.includes("${")) return { kind: "interpolated", raw };
		if (quote === "`" && inner.includes("\n")) return { kind: "literal", value: inner };
		const unescaped = inner
			.replace(/\\(['"`\\$nrtbfv0])/g, (_m, ch: string) =>
				ch === "n" ? "\n" : ch === "t" ? "\t" : ch === "r" ? "\r" : ch === "b" ? "\b" : ch === "f" ? "\f" : ch === "v" ? "\v" : ch === "0" ? "\0" : ch,
			)
			.replace(/\\(u\{[0-9a-fA-F]+\}|u[0-9a-fA-F]{4}|x[0-9a-fA-F]{2})/g, (_m, esc: string) => {
				const code = esc.startsWith("u{") ? Number.parseInt(esc.slice(2, -1), 16) : Number.parseInt(esc.slice(esc.length - (esc.startsWith("x") ? 2 : 4)), 16);
				return String.fromCodePoint(code);
			});
		return { kind: "literal", value: unescaped };
	}
	if (text.startsWith("[") || text.startsWith("{")) {
		try {
			return { kind: "literal", value: JSON.parse(text.replace(/\s+as\s+const$/, "")) };
		} catch {
			return { kind: "nonliteral", raw };
		}
	}
	if (/^[\d\s*+().\-/]+$/.test(text) && /\d/.test(text)) {
		try {
			const value = Number(new Function(`"use strict";return (${text});`)());
			if (Number.isFinite(value)) return { kind: "literal", value };
		} catch {
			/* fall through */
		}
	}
	return { kind: "nonliteral", raw };
}

/** docs/13 default cells are markdown: backticks, and sometimes a quoted or underscored literal. */
function parseDocDefault(cell: string): Literal {
	let text = cell.trim();
	if (text.startsWith("`") && text.endsWith("`") && text.length >= 2) text = text.slice(1, -1).trim();
	if (text === "…" || text.includes("…") || text.includes("...")) return { kind: "nonliteral", raw: text };
	return parseLiteral(text);
}

/** The value that follows a `field:` in TS source: up to the first top-level `,`, `}` or newline. */
function readTsValue(text: string): string {
	let depth = 0;
	let quote: string | null = null;
	let i = 0;
	for (; i < text.length; i++) {
		const ch = text[i];
		if (quote) {
			if (ch === "\\") i++;
			else if (ch === quote) quote = null;
			continue;
		}
		if (ch === '"' || ch === "'" || ch === "`") quote = ch;
		else if (ch === "[" || ch === "{" || ch === "(") depth++;
		else if (ch === "]" || ch === ")") depth--;
		else if (ch === "}") {
			if (depth === 0) break;
			depth--;
		} else if (ch === "," && depth === 0) break;
	}
	return text.slice(0, i).trim();
}

/* --------------------------------------------------------------- the schema */

interface SchemaEntry {
	key: string;
	label: string | null;
	description: string | null;
	condition: string | null;
	defaultRaw: string | null;
}

/**
 * Read `SETTINGS_SCHEMA` one top-level entry at a time. Entries sit at exactly one tab of
 * indentation; everything deeper belongs to them. `ui.label` / `ui.description` / `ui.condition`
 * are the entry's own level-3 fields.
 */
function parseSchema(source: string): SchemaEntry[] {
	const lines = source.split("\n");
	const start = lines.findIndex((l) => l.startsWith("export const SETTINGS_SCHEMA = {"));
	if (start < 0) fail("settings-schema.ts: `export const SETTINGS_SCHEMA = {` not found");
	const end = lines.findIndex((l, i) => i > start && l === "} as const;");
	if (end < 0) fail("settings-schema.ts: end of SETTINGS_SCHEMA (`} as const;`) not found");
	const keyRe = /^\t(?:"([^"]+)"|([A-Za-z_$][\w$]*)): \{/;
	const bounds: number[] = [];
	for (let i = start + 1; i < end; i++) {
		const m = keyRe.exec(lines[i]);
		if (m) bounds.push(i);
	}
	const entries: SchemaEntry[] = [];
	for (let b = 0; b < bounds.length; b++) {
		const from = bounds[b];
		const to = b + 1 < bounds.length ? bounds[b + 1] : end;
		const header = keyRe.exec(lines[from])!;
		const entry: SchemaEntry = { key: header[1] ?? header[2], label: null, description: null, condition: null, defaultRaw: null };
		const inline = lines[from].slice(header[0].length);
		const readField = (field: string, text: string): string | null => {
			const m = new RegExp(String.raw`(?:^|[\s{])${field}:\s*([\s\S]*)$`).exec(text);
			return m ? readTsValue(m[1]) : null;
		};
		// level-2 fields: the rest of the key line, plus body lines at exactly two tabs
		const level2: string[] = [inline];
		for (let i = from + 1; i < to; i++) if (/^\t\t\S/.test(lines[i])) level2.push(lines[i].slice(2));
		for (const text of level2) {
			if (entry.defaultRaw === null) entry.defaultRaw = readField("default", text);
		}
		for (let i = from + 1; i < to; i++) {
			const m = /^\t\t\t(label|description|condition):\s*(.*)$/.exec(lines[i]);
			if (!m) continue;
			const [, field, rest] = m;
			if (field === "condition") {
				const q = /^"([A-Za-z][A-Za-z0-9]*)"/.exec(rest.trim());
				if (!q) fail(`settings-schema.ts:${i + 1}: unparsable condition: ${lines[i].trim()}`);
				if (entry.condition && entry.condition !== q[1]) fail(`settings-schema.ts:${i + 1}: entry ${entry.key} has two conditions`);
				entry.condition = q[1];
				continue;
			}
			let raw = rest.trim();
			if (raw === "") {
				// the literal continues on the following, deeper-indented lines
				const parts: string[] = [];
				let j = i + 1;
				while (j < to && (/^\t\t\t\t/.test(lines[j]) || lines[j].trim() === "")) {
					parts.push(lines[j].trim());
					j++;
				}
				raw = parts.join(" ");
			}
			const parsed = parseLiteral(raw);
			const value = parsed.kind === "literal" ? String(parsed.value) : raw.replace(/,$/, "");
			if (field === "label") entry.label = value;
			else entry.description = value;
		}
		entries.push(entry);
	}
	return entries;
}

/** The twelve `CONDITIONS` names the UI can gate on (`settings-ui.ts:14-60`). */
function parseConditionNames(source: string): string[] {
	const lines = source.split("\n");
	const start = lines.findIndex((l) => l.startsWith("const CONDITIONS: Record<string, () => boolean> = {"));
	if (start < 0) fail("settings-ui.ts: CONDITIONS declaration not found");
	const names: string[] = [];
	for (let i = start + 1; i < lines.length; i++) {
		if (lines[i] === "};") break;
		const m = /^\t([A-Za-z][A-Za-z0-9]*):/.exec(lines[i]);
		if (m) names.push(m[1]);
	}
	if (names.length === 0) fail("settings-ui.ts: CONDITIONS object parsed as empty");
	return names;
}

/* ----------------------------------------------------------------- engines */

interface EngineKey {
	value?: unknown;
	hasValue: boolean;
	type: string;
	description: string;
}

function parseEngineJson(text: string): Map<string, EngineKey> {
	let parsed: Record<string, { value?: unknown; type?: string; description?: string }>;
	try {
		parsed = JSON.parse(text);
	} catch (error) {
		fail(`omp config list --json: output is not JSON (${String(error)})`);
	}
	const keys = new Map<string, EngineKey>();
	for (const [key, entry] of Object.entries(parsed)) {
		keys.set(key, {
			hasValue: Object.hasOwn(entry, "value"),
			value: entry.value,
			type: String(entry.type),
			description: typeof entry.description === "string" ? entry.description : "",
		});
	}
	return keys;
}

interface PlainList {
	tabs: Map<string, string>;
	domains: Map<string, string[]>;
}

/** `config list` — the engine's own tab grouping and its type display (enum domains). */
function parsePlainList(text: string): PlainList {
	const tabs = new Map<string, string>();
	const domains = new Map<string, string[]>();
	let tab = "";
	for (const line of text.split("\n")) {
		const heading = /^\[([^\]]+)\]$/.exec(line);
		if (heading) {
			tab = heading[1];
			continue;
		}
		const row = /^ {2}(\S+) = (.*)$/.exec(line);
		if (!row) continue;
		const [, key, rest] = row;
		if (tab === "") fail(`omp config list: ${key} appears before any [tab] heading`);
		tabs.set(key, tab);
		const display = /\(([^()]*)\)$/.exec(rest);
		if (display) domains.set(key, display[1].split("|").map((v) => v.trim()).filter((v) => v !== ""));
	}
	return { tabs, domains };
}

/* --------------------------------------------------------------------- doc */

interface DocRow {
	key: string;
	type: string;
	defaultRaw: string;
	section: string;
	control: string;
	live: string;
	disposition: "curated" | "deferred" | "hidden";
	group: string;
}

interface DocSection {
	title: string;
	blurb: string;
	order: number;
}

interface Doc {
	sections: DocSection[];
	rows: DocRow[];
	credentials: string[];
	danger: { key: string; level: "confirm" | "warn"; why: string }[];
	/** Rows whose `Key` cell named more than one key (each key still becomes its own row). */
	multiKeyCells: number;
}

function headingLine(lines: string[], heading: string): number {
	const index = lines.findIndex((l) => l.trim() === heading);
	if (index < 0) fail(`docs/13-settings-mapping.md: heading "${heading}" not found`);
	return index;
}

function parseDoc(source: string): Doc {
	const lines = source.split("\n");
	const nav = headingLine(lines, "## Nav structure");
	const keyMapping = headingLine(lines, "## Key mapping");
	const secrets = headingLine(lines, "## Secrets");
	const danger = headingLine(lines, "## Keys that must NOT be exposed");
	const hatch = headingLine(lines, "## Raw config escape hatch");
	if (!(nav < keyMapping && keyMapping < secrets && secrets < danger && danger < hatch)) {
		fail("docs/13-settings-mapping.md: sections are not in the expected order");
	}

	// Nav structure bullets: the section list, in emission order, with each section's blurb.
	const sections: DocSection[] = [];
	for (let i = nav + 1; i < keyMapping; i++) {
		const bullet = /^- \*\*(.+?)\*\* \((\d+) keys\) — (.*)$/.exec(lines[i]);
		if (!bullet) continue;
		let blurb = bullet[3].trim();
		let j = i + 1;
		while (j < keyMapping && /^ {2}\S/.test(lines[j])) {
			blurb += ` ${lines[j].trim()}`;
			j++;
		}
		sections.push({ title: bullet[1], blurb, order: sections.length + 1 });
	}
	if (sections.length !== EXPECT.sections) {
		fail(`docs/13 §Nav structure: parsed ${sections.length} sections, expected ${EXPECT.sections}`);
	}
	for (const section of sections) {
		if (!SECTION_CHROME[section.title]) fail(`docs/13: section "${section.title}" has no navGroup/icon in SECTION_CHROME`);
		if (section.blurb === "") fail(`docs/13: section "${section.title}" has no rationale line`);
	}

	// Key mapping: `###` sections, `####` sub-groups, and one row per key.
	const rows: DocRow[] = [];
	const headingSections: { title: string; keys: number; parsed: number }[] = [];
	let section: { title: string; keys: number; parsed: number } | null = null;
	let group: string | null = null;
	let multiKeyCells = 0;
	for (let i = keyMapping + 1; i < secrets; i++) {
		const line = lines[i];
		const sectionHeading = /^### (.+?) \((\d+) keys\)$/.exec(line);
		if (sectionHeading) {
			section = { title: sectionHeading[1], keys: Number(sectionHeading[2]), parsed: 0 };
			headingSections.push(section);
			group = null;
			continue;
		}
		if (line.startsWith("### ")) fail(`docs/13:${i + 1}: unrecognised section heading: ${line}`);
		const groupHeading = /^#### (.+?) — `[^`]+` \(\d+\)$/.exec(line);
		if (groupHeading) {
			group = groupHeading[1].trim();
			continue;
		}
		if (line.startsWith("#### ")) fail(`docs/13:${i + 1}: unrecognised sub-group heading: ${line}`);
		if (!line.startsWith("| ")) continue;
		if (line.startsWith("| --- ") || line.startsWith("| Key |")) continue;
		if (!section) fail(`docs/13:${i + 1}: table row before any section heading`);
		if (!group) fail(`docs/13:${i + 1}: table row before any sub-group heading`);
		const cells = line.trim().replace(/^\|/, "").replace(/\|$/, "").split("|").map((c) => c.trim());
		if (cells.length !== 7) fail(`docs/13:${i + 1}: expected 7 cells, got ${cells.length}: ${line}`);
		const [keyCell, type, defaultCell, navSection, control, live, dispositionCell] = cells;
		// A cell may name several keys (`a`, `b`) and/or line-break them; each becomes its own row.
		const keyNames = [...keyCell.matchAll(/`([^`]+)`/g)].map((m) => m[1].trim()).flatMap((k) => k.split("<br>").map((s) => s.trim())).filter(Boolean);
		if (keyNames.length === 0) fail(`docs/13:${i + 1}: row has no backticked key: ${line}`);
		if (keyNames.length > 1) multiKeyCells++;
		if (!TYPES[type]) fail(`docs/13:${i + 1}: unknown type "${type}"`);
		if (navSection !== section.title) fail(`docs/13:${i + 1}: row says section "${navSection}", heading says "${section.title}"`);
		if (!CONTROLS[control]) fail(`docs/13:${i + 1}: unknown control "${control}"`);
		if (!LIVE_CLASSES[live]) fail(`docs/13:${i + 1}: unknown live class "${live}"`);
		const disposition = DISPOSITIONS[dispositionCell];
		if (!disposition) fail(`docs/13:${i + 1}: unknown v1 disposition "${dispositionCell}"`);
		if (defaultCell.includes("<br>")) fail(`docs/13:${i + 1}: default cell contains <br>: ${defaultCell}`);
		for (const key of keyNames) {
			rows.push({ key, type, defaultRaw: defaultCell, section: navSection, control, live, disposition, group });
			section.parsed++;
		}
	}
	for (const heading of headingSections) {
		if (heading.parsed !== heading.keys) {
			fail(`docs/13:§${heading.title}: heading claims ${heading.keys} keys, parsed ${heading.parsed}`);
		}
	}
	const titles = headingSections.map((s) => s.title);
	const expectedTitles = sections.map((s) => s.title).filter((t) => t !== "Raw config");
	if (titles.length !== expectedTitles.length || titles.some((t, i) => t !== expectedTitles[i])) {
		fail(`docs/13: §Key mapping sections [${titles.join(", ")}] do not match §Nav structure [${expectedTitles.join(", ")}]`);
	}

	// §Secrets: the credential keys.
	const credentials: string[] = [];
	for (let i = secrets + 1; i < danger; i++) {
		const row = /^\| `([^`]+)` \|/.exec(lines[i]);
		if (row) credentials.push(row[1]);
	}

	// §Keys that must NOT be exposed: three sub-tables, first two `confirm`, third `warn`.
	const dangerRows: Doc["danger"] = [];
	let table = -1;
	for (let i = danger + 1; i < hatch; i++) {
		if (lines[i].startsWith("### ")) {
			table++;
			continue;
		}
		if (!lines[i].startsWith("| ")) continue;
		if (lines[i].startsWith("| --- ") || lines[i].startsWith("| Key |")) continue;
		const cells = lines[i].trim().replace(/^\|/, "").replace(/\|$/, "").split("|").map((c) => c.trim());
		if (cells.length !== 2) fail(`docs/13:${i + 1}: danger row expected 2 cells, got ${cells.length}: ${lines[i]}`);
		const key = /^`([^`]+)`$/.exec(cells[0]);
		if (!key) fail(`docs/13:${i + 1}: danger row key is not a single backticked key: ${cells[0]}`);
		if (table < 0 || table > 2) fail(`docs/13:${i + 1}: danger row outside the three sub-tables`);
		if (cells[1] === "") fail(`docs/13:${i + 1}: danger row ${key[1]} has no reason`);
		dangerRows.push({ key: key[1], level: table < 2 ? "confirm" : "warn", why: cells[1] });
	}
	return { sections, rows, credentials, danger: dangerRows, multiKeyCells };
}

/* ------------------------------------------------------------------ catalog */

function slug(title: string): string {
	return title
		.toLowerCase()
		.replace(/&/g, " and ")
		.replace(/[^a-z0-9]+/g, "-")
		.replace(/^-+|-+$/g, "");
}

function restartOf(key: string, live: string): "live" | "sidecar" | "app" {
	if (RPC_LIVE_KEYS[key]) {
		// the app owns a setter for this one and pushes the write into the live session
		if (live !== "RPC live") fail(`docs/13: ${key} has an RPC setter but the doc classes it "${live}"`);
		return "live";
	}
	switch (live) {
		case "RPC live":
			// the doc's class, not ours: an out-of-process write never reaches a per-request read
			return "sidecar";
		// The doc's `next call` class is honest for a write made *inside* the process; every write
		// this app makes comes from outside it, and the session's Settings layer is built at
		// construction — measured, see §"The measurement that fixes `restart`".
		case "next call":
		case "sidecar restart":
			return "sidecar";
		case "app restart":
			return "app";
		case "[UNVERIFIED]":
			// Unknown consumption point ⇒ promise the conservative thing.
			return "sidecar";
		default:
			return fail(`unknown live class "${live}"`);
	}
}

async function main(): Promise<void> {
	const args = parseArgs(Bun.argv.slice(2));
	const sidecar = resolve(REPO_ROOT, SIDECAR_REL);
	if (!existsSync(sidecar)) {
		fail(`pinned sidecar ${SIDECAR_REL} is missing — run \`bun scripts/fetch-sidecar.ts\` first`);
	}

	const [{ schema, ui }, docSource, engineJsonText, enginePlainText] = await Promise.all([
		loadPackageSources(args.packageDir),
		readFile(resolve(REPO_ROOT, DOC_REL), "utf8"),
		Promise.resolve(runEngine(["list", "--json"], "omp config list --json")),
		Promise.resolve(runEngine(["list"], "omp config list")),
	]);

	const schemaEntries = parseSchema(schema);
	const schemaByKey = new Map(schemaEntries.map((e) => [e.key, e]));
	const conditionNames = parseConditionNames(ui);
	const engine = parseEngineJson(engineJsonText);
	const plain = parsePlainList(enginePlainText);
	const doc = parseDoc(docSource);

	/*
	 * docs/13 §"Keys that must NOT be exposed" outranks the `v1?` column of §Key mapping: a key the
	 * doc says must never have a settings row cannot be curated. The list is the explicit, reasoned
	 * half of the doc, and the app's typed-confirmation decision (`11` §Q7) is built on it. The
	 * `v1?` column records the mapping's default disposition *before* the list is applied, so a
	 * danger key that the column calls `yes` becomes `hidden` here; the third table's `deferred`
	 * rows keep `deferred`, which renders no curated row either.
	 */
	const dangerKeys = new Set(doc.danger.map((entry) => entry.key));
	const forcedHidden: string[] = [];
	for (const row of doc.rows) {
		if (row.disposition === "curated" && dangerKeys.has(row.key)) {
			row.disposition = "hidden";
			forcedHidden.push(row.key);
		}
	}

	/* ------------------------------------------------------- cross-check 1 */
	const docKeys = doc.rows.map((r) => r.key);
	const docKeySet = new Set(docKeys);
	if (doc.rows.length !== EXPECT.rows) fail(`check 1: parsed ${doc.rows.length} key rows from docs/13, expected ${EXPECT.rows}`);
	if (docKeySet.size !== EXPECT.distinct) fail(`check 1: ${docKeySet.size} distinct keys, expected ${EXPECT.distinct}`);
	if (docKeySet.size !== docKeys.length) {
		const dupes = docKeys.filter((k, i) => docKeys.indexOf(k) !== i);
		fail(`check 1: duplicate rows for ${[...new Set(dupes)].join(", ")}`);
	}
	if (schemaEntries.length !== EXPECT.rows) fail(`check 1: settings-schema.ts declares ${schemaEntries.length} entries, expected ${EXPECT.rows}`);
	const onlyDoc = [...docKeySet].filter((k) => !engine.has(k));
	const onlyEngine = [...engine.keys()].filter((k) => !docKeySet.has(k));
	const onlySchema = [...schemaByKey.keys()].filter((k) => !docKeySet.has(k));
	if (onlyDoc.length) fail(`check 1: docs/13 lists keys the engine does not: ${onlyDoc.join(", ")}`);
	if (onlyEngine.length) fail(`check 1: the engine has keys docs/13 does not: ${onlyEngine.join(", ")}`);
	if (onlySchema.length) fail(`check 1: the schema has keys docs/13 does not: ${onlySchema.join(", ")}`);

	/* ------------------------------------------------------- cross-check 2 */
	if (doc.sections.length !== EXPECT.sections) fail(`check 2: ${doc.sections.length} sections, expected ${EXPECT.sections}`);
	for (const row of doc.rows) {
		if (!row.section) fail(`check 2: ${row.key} has no section`);
		if (!row.group) fail(`check 2: ${row.key} has no group`);
		if (!doc.sections.some((s) => s.title === row.section)) fail(`check 2: ${row.key} names unknown section "${row.section}"`);
	}

	/* ------------------------------------------------------- cross-check 3 */
	const dispositions = { curated: 0, deferred: 0, hidden: 0 };
	for (const row of doc.rows) dispositions[row.disposition]++;
	if (dispositions.curated !== EXPECT.curated) fail(`check 3: curated ${dispositions.curated}, expected ${EXPECT.curated}`);
	if (dispositions.deferred !== EXPECT.deferred) fail(`check 3: deferred ${dispositions.deferred}, expected ${EXPECT.deferred}`);
	if (dispositions.hidden !== EXPECT.hidden) fail(`check 3: hidden ${dispositions.hidden}, expected ${EXPECT.hidden}`);
	// The invariant that catches the doc contradicting itself: a key it says must never have a row,
	// must not have one. `confirm` keys are the ones the app itself writes; neither may be curated.
	const dispositionByKey = new Map(doc.rows.map((row) => [row.key, row.disposition]));
	const curatedDanger = doc.danger.filter((entry) => dispositionByKey.get(entry.key) === "curated");
	if (curatedDanger.length) fail(`check 3: keys on the do-not-expose list are curated: ${curatedDanger.map((e) => e.key).join(", ")}`);
	const confirmCurated = doc.danger.filter((entry) => entry.level === "confirm" && dispositionByKey.get(entry.key) === "curated");
	if (confirmCurated.length) fail(`check 3: confirm-level danger keys are curated: ${confirmCurated.map((e) => e.key).join(", ")}`);

	/* ------------------------------------------------------- cross-check 4 */
	const restarts = { live: 0, sidecar: 0, app: 0 };
	const liveKeys = new Set<string>();
	for (const row of doc.rows) {
		const restart = restartOf(row.key, row.live);
		restarts[restart]++;
		if (restart === "live") liveKeys.add(row.key);
	}
	const settersWithoutLive = Object.keys(RPC_LIVE_KEYS).filter((key) => !liveKeys.has(key));
	if (settersWithoutLive.length) fail(`check 4: RPC-settable keys that did not map to live: ${settersWithoutLive.join(", ")}`);
	if (liveKeys.size !== EXPECT.live) fail(`check 4: ${liveKeys.size} live keys, expected ${EXPECT.live}`);
	if (restarts.live !== EXPECT.live) fail(`check 4: ${restarts.live} live rows, expected ${EXPECT.live}`);
	if (restarts.app !== EXPECT.app) fail(`check 4: ${restarts.app} app rows, expected ${EXPECT.app}`);
	if (restarts.live + restarts.sidecar + restarts.app !== doc.rows.length) fail("check 4: restart totals do not add up to the row count");

	/* ------------------------------------------------------- cross-check 5 */
	const keys: Record<string, unknown>[] = [];
	const enumKeys: string[] = [];
	const unknownConditions: string[] = [];
	const conditionsSeen = new Set<string>();
	const usedConditionNames = new Set<string>();
	const descriptionDisagreements: string[] = [];
	const interpolatedDescriptions: string[] = [];
	const defaultOverrides: string[] = [];
	const defaultDivergences: string[] = [];
	const typeDivergences: string[] = [];
	const credentialSet = new Set(doc.credentials);
	const enumsWithoutDomain: string[] = [];

	for (const row of doc.rows) {
		const engineKey = engine.get(row.key);
		if (!engineKey) fail(`check 1: no engine entry for ${row.key}`);
		const schemaEntry = schemaByKey.get(row.key);
		if (!plain.tabs.has(row.key)) fail(`check 5: ${row.key} missing from \`omp config list\``);

		// type — the engine owns it; docs/13 must agree.
		if (engineKey.type !== row.type) typeDivergences.push(`${row.key}: doc ${row.type} vs engine ${engineKey.type}`);
		const isEnum = row.type === "enum";
		let values: string[] = [];
		if (isEnum) {
			enumKeys.push(row.key);
			const domain = plain.domains.get(row.key) ?? [];
			if (domain.length === 0 || (domain.length === 1 && domain[0] === engineKey.type)) {
				enumsWithoutDomain.push(row.key);
			} else {
				values = domain;
			}
		}

		// `description` is the engine's own; the schema's copy is only consulted for the report below.
		const schemaDescription = schemaEntry?.description ?? null;
		if (schemaDescription?.includes("${")) {
			// a template literal interpolates constants at runtime; comparing it textually is meaningless
			interpolatedDescriptions.push(row.key);
		} else if (
			schemaDescription !== null &&
			schemaDescription.replace(/\s+/g, " ").trim() !== engineKey.description.replace(/\s+/g, " ").trim()
		) {
			descriptionDisagreements.push(row.key);
		}

		// condition — the schema's own symbolic name.
		const condition = schemaEntry?.condition ?? null;
		if (condition) {
			if (!conditionNames.includes(condition)) unknownConditions.push(`${row.key} → ${condition}`);
			else {
				conditionsSeen.add(row.key);
				usedConditionNames.add(condition);
			}
		}

		// default — docs/13's literal must match the engine's value; a difference is only tolerated
		// when the schema's own declared default proves the engine value is a local override.
		const docDefault = parseDocDefault(row.defaultRaw);
		if (docDefault.kind === "literal" && engineKey.hasValue) {
			if (!deepEqual(docDefault.value, engineKey.value)) {
				const schemaDefault = parseLiteral(schemaEntry?.defaultRaw ?? "");
				if (schemaDefault.kind === "literal" && deepEqual(schemaDefault.value, docDefault.value)) {
					defaultOverrides.push(`${row.key}: doc ${JSON.stringify(docDefault.value)}, engine ${JSON.stringify(engineKey.value)}`);
				} else {
					defaultDivergences.push(
						`${row.key}: doc ${JSON.stringify(docDefault.value)}, engine ${JSON.stringify(engineKey.value)}, schema ${JSON.stringify(
							schemaDefault.kind === "literal" ? schemaDefault.value : schemaDefault.raw,
						)}`,
					);
				}
			}
		}

		const optionLabels: Record<string, string> = {};
		for (const value of values) optionLabels[value] = humanise(value);

		const entry: Record<string, unknown> = {
			key: row.key,
			label: schemaEntry?.label ?? humanise(row.key.split(".").pop() ?? row.key),
			type: engineKey.type,
			description: engineKey.description,
			values,
		};
		if (values.length > 0) entry.optionLabels = optionLabels;
		entry.credential = credentialSet.has(row.key);
		if (condition) entry.condition = condition;
		entry.tab = plain.tabs.get(row.key);
		entry.group = row.group;
		entry.section = slug(row.section);
		entry.disposition = row.disposition;
		entry.control = row.control === "record/JSON" ? "record" : row.control;
		entry.restart = restartOf(row.key, row.live);
		keys.push(entry);
	}

	if (enumKeys.length !== EXPECT.enums) fail(`check 5: ${enumKeys.length} enum keys, expected ${EXPECT.enums}`);
	if (enumsWithoutDomain.length) fail(`check 5: enums with no domain parsed from \`omp config list\`: ${enumsWithoutDomain.join(", ")}`);
	if (unknownConditions.length) fail(`check 8: conditions that are not CONDITIONS names: ${unknownConditions.join(", ")}`);
	if (conditionsSeen.size !== EXPECT.conditions) fail(`check 8: ${conditionsSeen.size} keys carry a condition, expected ${EXPECT.conditions}`);
	if (conditionNames.length !== EXPECT.conditionNames) fail(`check 8: ${conditionNames.length} CONDITIONS names, expected ${EXPECT.conditionNames}`);
	const unusedConditions = conditionNames.filter((n) => !usedConditionNames.has(n));
	if (unusedConditions.length) fail(`check 8: CONDITIONS names never used: ${unusedConditions.join(", ")}`);

	/* ------------------------------------------------------- cross-check 6 */
	if (doc.credentials.length !== EXPECT.credentials) fail(`check 6: ${doc.credentials.length} credentials, expected ${EXPECT.credentials}`);
	for (const key of doc.credentials) if (!docKeySet.has(key)) fail(`check 6: credential ${key} is not a key row`);

	/* ------------------------------------------------------- cross-check 7 */
	if (doc.danger.length !== EXPECT.danger) fail(`check 7: ${doc.danger.length} danger keys, expected ${EXPECT.danger}`);
	const confirmCount = doc.danger.filter((d) => d.level === "confirm").length;
	const warnCount = doc.danger.filter((d) => d.level === "warn").length;
	if (confirmCount !== EXPECT.dangerConfirm) fail(`check 7: ${confirmCount} confirm keys, expected ${EXPECT.dangerConfirm}`);
	if (warnCount !== EXPECT.dangerWarn) fail(`check 7: ${warnCount} warn keys, expected ${EXPECT.dangerWarn}`);
	for (const entry of doc.danger) if (!docKeySet.has(entry.key)) fail(`check 7: danger key ${entry.key} is not in the key set`);

	/* ------------------------------------------------------- cross-check 9 */
	if (typeDivergences.length) fail(`check 9: doc/engine type mismatches: ${typeDivergences.join("; ")}`);
	if (defaultDivergences.length) {
		fail(`check 9: docs/13 defaults disagree with the engine (and not because of a local config override):\n  ${defaultDivergences.join("\n  ")}`);
	}
	for (const override of defaultOverrides) {
		note(`docs/13 default vs this machine's config (schema default agrees with the doc, keeping the doc): ${override}`);
	}
	for (const key of interpolatedDescriptions) note(`description of ${key} is an interpolated template literal in the schema; the engine's text is used`);
	for (const key of descriptionDisagreements) note(`engine and schema descriptions differ for ${key}; the engine's is used`);

	const catalog = {
		engineVersion: ENGINE_VERSION,
		schemaSha256: createHash("sha256").update(schema, "utf8").digest("hex"),
		sections: doc.sections.map((section) => {
			const chrome = SECTION_CHROME[section.title];
			return { id: slug(section.title), title: section.title, blurb: section.blurb, navGroup: chrome.navGroup, icon: chrome.icon, order: section.order };
		}),
		keys,
		danger: doc.danger.map((entry) => ({ key: entry.key, level: entry.level, why: entry.why })),
	};
	const json = `${JSON.stringify(catalog, null, 2)}\n`;
	const outPath = resolve(REPO_ROOT, OUT_REL);

	console.log(
		`gen-settings-catalog: ${doc.rows.length} rows / ${docKeySet.size} distinct · ${doc.sections.length} sections · ` +
			`${dispositions.curated} curated / ${dispositions.deferred} deferred / ${dispositions.hidden} hidden · ` +
			`${enumKeys.length} enums · ${doc.credentials.length} credentials · ${doc.danger.length} danger keys ` +
			`(${confirmCount} confirm + ${warnCount} warn) · ${conditionsSeen.size} conditions · ` +
			`restart: ${restarts.live} live / ${restarts.sidecar} sidecar / ${restarts.app} app · ${Buffer.byteLength(json)} bytes`,
	);
	if (forcedHidden.length) {
		note(`docs/13 §"Keys that must NOT be exposed" overrides the v1? column of §Key mapping for ${forcedHidden.length} key(s): ${forcedHidden.join(", ")}`);
	}
	if (doc.multiKeyCells > 0) note(`docs/13: ${doc.multiKeyCells} table row(s) listed more than one key; each key emitted`);

	if (args.check) {
		if (!existsSync(outPath)) fail(`${OUT_REL} does not exist — run \`bun scripts/gen-settings-catalog.ts\` to generate it`);
		const onDisk = await readFile(outPath, "utf8");
		if (onDisk === json) {
			console.log(`gen-settings-catalog: --check OK (${OUT_REL} is up to date)`);
			return;
		}
		const expected = json.split("\n");
		const actual = onDisk.split("\n");
		const differing: number[] = [];
		if (expected.length === actual.length) {
			for (let i = 0; i < expected.length; i++) if (expected[i] !== actual[i]) differing.push(i);
		} else {
			let head = 0;
			while (head < expected.length && head < actual.length && expected[head] === actual[head]) head++;
			let tail = 0;
			while (
				tail < expected.length - head &&
				tail < actual.length - head &&
				expected[expected.length - 1 - tail] === actual[actual.length - 1 - tail]
			) {
				tail++;
			}
			console.error(`gen-settings-catalog: line count differs — on disk ${actual.length}, generated ${expected.length}`);
			for (let i = Math.max(0, head - 2); i < Math.min(actual.length, head + 4); i++) console.error(`-  ${actual[i] ?? ""}`);
			for (let i = Math.max(0, head - 2); i < Math.min(expected.length, head + 4); i++) console.error(`+  ${expected[i] ?? ""}`);
		}
		console.error(`--- ${OUT_REL} (on disk, ${actual.length} lines)`);
		console.error(`+++ generated (${expected.length} lines)`);
		for (const index of differing.slice(0, 3)) {
			console.error(`@@ line ${index + 1} @@`);
			for (let i = Math.max(0, index - 1); i < Math.min(actual.length, index + 2); i++) console.error(`-  ${actual[i]}`);
			for (let i = Math.max(0, index - 1); i < Math.min(expected.length, index + 2); i++) console.error(`+  ${expected[i]}`);
		}
		if (differing.length > 3) console.error(`gen-settings-catalog: ${differing.length - 3} further differing line(s): ${differing.slice(3, 13).map((i) => i + 1).join(", ")}${differing.length > 13 ? ", …" : ""}`);
		console.error(`gen-settings-catalog: ${OUT_REL} is stale — ${differing.length} line(s) differ`);
		process.exit(1);
	}

	await mkdir(dirname(outPath), { recursive: true });
	await writeFile(outPath, json);
	console.log(`gen-settings-catalog: wrote ${OUT_REL}`);
}

await main();
