import { Buffer } from "node:buffer";
import { execFileSync } from "node:child_process";
import { chmodSync, readFileSync, renameSync, rmSync, statSync, writeFileSync } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { ROOT, MARKET_SNAPSHOT, WFSTAT_CATALOG, fail, object } from "./files";

const RELEASE_SNAPSHOT_MAX_AGE_HOURS = 24;
const MAX_SNAPSHOT_FILE_BYTES = 16 * 1024 * 1024;

const REQUIRED_MARKET_KEYS = [
  "updated_at",
  "platform",
  "item_count",
  "catalog_count",
  "source",
  "catalog",
  "items",
  "path_to_info",
  "set_to_parts",
  "relic_rewards",
  "vault_status",
  "baro",
  "surface_fetched_at",
] as const;

// Strict X.Y.Z with no leading zeros. Prerelease and build-metadata suffixes
export interface SnapshotSummary {
  updatedAt: string;
  itemCount: number;
  catalogCount: number;
  resolverCount: number;
}

export function validateSnapshotValues(
  marketValue: unknown,
  resolverValue: unknown,
  options: { maxAgeHours?: number; nowMs?: number } = {},
): SnapshotSummary {
  const market = object(marketValue, "market.json");
  const missing = REQUIRED_MARKET_KEYS.filter((key) => !(key in market));
  if (missing.length > 0) {
    throw new Error(`market.json is missing required keys: ${missing.join(", ")}`);
  }

  const items = object(market.items, "market.json items");
  const catalog = object(market.catalog, "market.json catalog");
  const itemCount = Object.keys(items).length;
  const catalogCount = Object.keys(catalog).length;
  if (itemCount === 0) throw new Error("market.json items are empty");
  if (catalogCount === 0) throw new Error("market.json catalog is empty");
  if (!Number.isInteger(market.item_count) || market.item_count !== itemCount) {
    throw new Error(
      `market.json item_count is ${String(market.item_count)}, but items contains ${itemCount}`,
    );
  }
  if (!Number.isInteger(market.catalog_count) || market.catalog_count !== catalogCount) {
    throw new Error(
      `market.json catalog_count is ${String(market.catalog_count)}, but catalog contains ${catalogCount}`,
    );
  }

  if (!Array.isArray(resolverValue) || resolverValue.length === 0) {
    throw new Error("wfstat-catalog.json must be a non-empty array");
  }
  for (let i = 0; i < resolverValue.length; i++) {
    const entry = resolverValue[i];
    if (
      !Array.isArray(entry) ||
      entry.length !== 2 ||
      typeof entry[0] !== "string" ||
      entry[0].length === 0 ||
      typeof entry[1] !== "object" ||
      entry[1] === null ||
      Array.isArray(entry[1])
    ) {
      throw new Error(`wfstat-catalog.json entry ${i} is not a [path, info] pair`);
    }
  }

  if (typeof market.updated_at !== "string") {
    throw new Error("market.json updated_at must be an ISO timestamp");
  }
  const updatedMs = Date.parse(market.updated_at);
  if (!Number.isFinite(updatedMs)) {
    throw new Error(`market.json updated_at is invalid: ${market.updated_at}`);
  }
  const nowMs = options.nowMs ?? Date.now();
  const ageMs = nowMs - updatedMs;
  if (ageMs < -10 * 60 * 1000) {
    throw new Error(`market.json updated_at is in the future: ${market.updated_at}`);
  }
  if (options.maxAgeHours !== undefined && ageMs > options.maxAgeHours * 60 * 60 * 1000) {
    throw new Error(
      `market.json is ${(ageMs / 3_600_000).toFixed(1)} h old; release limit is ${options.maxAgeHours} h. ` +
        "Run `bun scripts/release.ts snapshot` during release preparation.",
    );
  }

  return {
    updatedAt: market.updated_at,
    itemCount,
    catalogCount,
    resolverCount: resolverValue.length,
  };
}

function parseJson(path: string): unknown {
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    throw new Error(`${path} is not valid JSON: ${(error as Error).message}`);
  }
}

export function validateSnapshotDirectory(dir: string, release: boolean): SnapshotSummary {
  return validateSnapshotValues(
    parseJson(join(dir, "market.json")),
    parseJson(join(dir, "wfstat-catalog.json")),
    release ? { maxAgeHours: RELEASE_SNAPSHOT_MAX_AGE_HOURS } : {},
  );
}

export function printSnapshotSummary(summary: SnapshotSummary) {
  console.log(
    `snapshot ${summary.updatedAt}: ${summary.itemCount} items, ` +
      `${summary.catalogCount} market names, ${summary.resolverCount} resolver paths`,
  );
}

export function cmdSnapshotCheck(argv: string[]) {
  let release = false;
  let dir = join(ROOT, "frontend/public");
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--release") {
      release = true;
    } else if (argv[i] === "--dir") {
      dir = resolve(ROOT, argv[++i] ?? fail("--dir needs a path"));
    } else {
      fail(`unknown snapshot-check flag: ${argv[i]}`);
    }
  }

  try {
    printSnapshotSummary(validateSnapshotDirectory(dir, release));
  } catch (error) {
    fail((error as Error).message);
  }
}

const REMOTE_SNAPSHOT_SCRIPT = `set -euo pipefail
APP=/srv/wfm/app
exec 9<"$APP"
if ! flock -n 9; then
  echo "ABORT: wfm-scrape currently owns the snapshot lock; retry after it finishes." >&2
  exit 75
fi
state=$(systemctl show wfm-scrape.service -p ActiveState --value 2>/dev/null || true)
case "\${state:-unknown}" in
  inactive|failed) ;;
  *) echo "ABORT: wfm-scrape.service is \${state:-unknown}; retry after it finishes." >&2; exit 75 ;;
esac
cd "$APP/frontend/public"
market_size=$(stat -c %s market.json)
resolver_size=$(stat -c %s wfstat-catalog.json)
printf '%s\\n%s\\n' "$market_size" "$resolver_size"
cat market.json wfstat-catalog.json
`;

export function splitSnapshotFrame(frame: Buffer): { market: Buffer; resolver: Buffer } {
  const firstNewline = frame.indexOf(0x0a);
  const secondNewline = firstNewline === -1 ? -1 : frame.indexOf(0x0a, firstNewline + 1);
  if (firstNewline < 1 || secondNewline < firstNewline + 2) {
    throw new Error("production returned an invalid snapshot frame header");
  }
  const headers = [frame.subarray(0, firstNewline), frame.subarray(firstNewline + 1, secondNewline)]
    .map((part) => part.toString("ascii"));
  if (headers.some((header) => !/^\d+$/.test(header))) {
    throw new Error(`production returned invalid snapshot sizes: ${headers.join(", ")}`);
  }
  const sizes = headers.map(Number);
  if (sizes.some((size) => !Number.isSafeInteger(size) || size <= 0 || size > MAX_SNAPSHOT_FILE_BYTES)) {
    throw new Error(`production returned invalid snapshot sizes: ${sizes.join(", ")}`);
  }
  const payloadAt = secondNewline + 1;
  if (frame.length !== payloadAt + sizes[0] + sizes[1]) {
    throw new Error("production snapshot frame was truncated or had trailing data");
  }
  return {
    market: frame.subarray(payloadAt, payloadAt + sizes[0]),
    resolver: frame.subarray(payloadAt + sizes[0]),
  };
}

function atomicReplace(path: string, contents: Buffer) {
  const temporary = join(dirname(path), `.${basename(path)}.${process.pid}.tmp`);
  try {
    writeFileSync(temporary, contents);
    chmodSync(temporary, statSync(path).mode & 0o777);
    renameSync(temporary, path);
  } finally {
    rmSync(temporary, { force: true });
  }
}

export function cmdSnapshot(argv: string[]) {
  let host = "wfm";
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--host") {
      host = argv[++i] ?? fail("--host needs an SSH host or alias");
    } else {
      fail(`unknown snapshot flag: ${argv[i]}`);
    }
  }
  if (!/^[A-Za-z0-9_.@:-]+$/.test(host) || host.startsWith("-")) {
    fail(`invalid SSH host: ${host}`);
  }

  const tracked = [MARKET_SNAPSHOT, WFSTAT_CATALOG];
  const dirty = execFileSync("git", ["status", "--porcelain=v1", "--", ...tracked], {
    cwd: ROOT,
    encoding: "utf8",
  }).trim();
  if (dirty) {
    fail(`refusing to overwrite locally modified snapshot files:\n${dirty}`);
  }

  let frame: Buffer;
  try {
    frame = execFileSync(
      "ssh",
      ["-o", "BatchMode=yes", host, "bash", "-s"],
      {
        input: REMOTE_SNAPSHOT_SCRIPT,
        maxBuffer: 2 * MAX_SNAPSHOT_FILE_BYTES + 1024,
        stdio: ["pipe", "pipe", "inherit"],
      },
    );
  } catch (error) {
    const status = (error as { status?: number }).status;
    if (status === 75) fail("production scrape is active; retry when it finishes");
    fail(`could not copy the production snapshot from ${host}`);
  }

  let parts: { market: Buffer; resolver: Buffer };
  try {
    parts = splitSnapshotFrame(frame);
    const summary = validateSnapshotValues(
      JSON.parse(parts.market.toString("utf8")),
      JSON.parse(parts.resolver.toString("utf8")),
      { maxAgeHours: RELEASE_SNAPSHOT_MAX_AGE_HOURS },
    );
    printSnapshotSummary(summary);
  } catch (error) {
    fail(`production snapshot rejected: ${(error as Error).message}`);
  }

  // Match the generator's publication order: catalog first, market.json last
  // as the anchor that says the pair is ready.
  const marketPath = join(ROOT, MARKET_SNAPSHOT);
  const resolverPath = join(ROOT, WFSTAT_CATALOG);
  const originalMarket = readFileSync(marketPath);
  const originalResolver = readFileSync(resolverPath);
  try {
    atomicReplace(resolverPath, parts.resolver);
    atomicReplace(marketPath, parts.market);
  } catch (error) {
    try {
      atomicReplace(resolverPath, originalResolver);
      atomicReplace(marketPath, originalMarket);
    } catch (rollbackError) {
      fail(
        `snapshot install failed (${(error as Error).message}) and rollback failed: ` +
          (rollbackError as Error).message,
      );
    }
    fail(`snapshot install failed; restored previous pair: ${(error as Error).message}`);
  }
  console.log(`copied ${MARKET_SNAPSHOT} and ${WFSTAT_CATALOG} from ${host}`);
  console.log("include both files in the desktop release-preparation commit");
}
