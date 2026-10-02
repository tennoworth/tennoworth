import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

export const ROOT = fileURLToPath(new URL("../..", import.meta.url));

export const CARGO_TOML = "rust/tennoworth-desktop/Cargo.toml";
export const CARGO_LOCK = "rust/Cargo.lock";
export const CHANGELOG = "CHANGELOG.md";
export const MARKET_SNAPSHOT = "frontend/public/market.json";
export const WFSTAT_CATALOG = "frontend/public/wfstat-catalog.json";
// Strict X.Y.Z with no leading zeros. Prerelease and build-metadata suffixes
// are rejected on purpose, not for lack of a regex: the updater has ONE
// endpoint, and semver orders 0.4.0-beta.1 above 0.3.8, so publishing a
// prerelease today would offer it to every stable install. Prereleases unlock
// when a separate beta endpoint exists.
export const SEMVER = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;

export const read = (rel: string) => readFileSync(join(ROOT, rel), "utf8");
export const write = (rel: string, text: string) =>
  writeFileSync(join(ROOT, rel), text);

export function fail(message: string): never {
  console.error(`error: ${message}`);
  process.exit(1);
}

type JsonObject = Record<string, unknown>;
export function object(value: unknown, name: string): JsonObject {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error(`${name} must be an object`);
  }
  return value as JsonObject;
}

