import { describe, expect, test } from "bun:test";

import { Buffer } from "node:buffer";
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import {
  releaseNotesBody,
  releaseNotesTemplate,
  splitSnapshotFrame,
  validateReleaseNotes,
  validateSnapshotValues,
} from "./release";

const NOW = Date.parse("2026-09-02T12:00:00Z");

function validMarket() {
  return {
    updated_at: "2026-09-02T10:00:00Z",
    platform: "pc",
    item_count: 1,
    catalog_count: 1,
    source: "fixture",
    catalog: { "primed continuity": "primed_continuity" },
    items: { primed_continuity: { low_sell: 10 } },
    path_to_info: {},
    set_to_parts: {},
    relic_rewards: {},
    vault_status: {},
    baro: {},
    surface_fetched_at: {},
  };
}

const validResolver = [["/Lotus/Fixture", { name: "Fixture", category: "Misc" }]];

describe("release snapshot validation", () => {
  test("accepts a current, internally consistent pair", () => {
    expect(
      validateSnapshotValues(validMarket(), validResolver, {
        maxAgeHours: 24,
        nowMs: NOW,
      }),
    ).toEqual({
      updatedAt: "2026-09-02T10:00:00Z",
      itemCount: 1,
      catalogCount: 1,
      resolverCount: 1,
    });
  });

  test("rejects missing shape, count drift, and malformed resolver rows", () => {
    const missing = validMarket();
    delete (missing as Partial<ReturnType<typeof validMarket>>).items;
    expect(() => validateSnapshotValues(missing, validResolver)).toThrow(
      "missing required keys: items",
    );

    const drifted = validMarket();
    drifted.item_count = 2;
    expect(() => validateSnapshotValues(drifted, validResolver)).toThrow(
      "items contains 1",
    );

    expect(() => validateSnapshotValues(validMarket(), [["missing-info"]])).toThrow(
      "entry 0 is not a [path, info] pair",
    );
  });

  test("rejects stale, invalid, and future timestamps for a release", () => {
    const stale = validMarket();
    stale.updated_at = "2026-08-31T10:00:00Z";
    expect(() =>
      validateSnapshotValues(stale, validResolver, { maxAgeHours: 24, nowMs: NOW }),
    ).toThrow("release limit is 24 h");

    const invalid = validMarket();
    invalid.updated_at = "not-a-date";
    expect(() => validateSnapshotValues(invalid, validResolver)).toThrow(
      "updated_at is invalid",
    );

    const future = validMarket();
    future.updated_at = "2026-09-02T12:11:00Z";
    expect(() => validateSnapshotValues(future, validResolver, { nowMs: NOW })).toThrow(
      "updated_at is in the future",
    );
  });
});

describe("production snapshot framing", () => {
  test("splits two exact byte payloads", () => {
    const market = Buffer.from('{"market":true}\n');
    const resolver = Buffer.from('[["path",{}]]\n');
    const frame = Buffer.concat([
      Buffer.from(`${market.length}\n${resolver.length}\n`),
      market,
      resolver,
    ]);

    const split = splitSnapshotFrame(frame);
    expect(split.market.equals(market)).toBe(true);
    expect(split.resolver.equals(resolver)).toBe(true);
  });

  test("rejects malformed sizes and a truncated or extended payload", () => {
    expect(() => splitSnapshotFrame(Buffer.from("wat\n1\nx"))).toThrow(
      "invalid snapshot sizes",
    );
    expect(() => splitSnapshotFrame(Buffer.from("2\n1\nxy"))).toThrow(
      "truncated or had trailing data",
    );
    expect(() => splitSnapshotFrame(Buffer.from("1\n1\nxyz"))).toThrow(
      "truncated or had trailing data",
    );
  });
});

const validReleaseNotes = `# 🐧 TennoWorth Desktop 0.6.7

TennoWorth Desktop 0.6.7 is ready.

This patch makes relic reward recognition more dependable on Linux.

## Changelog (2)

### Linux

- Keep the reward crop aligned across more display shapes.
  - This nested detail does not inflate the change count.

### Reliability

- Preserve stable reward names between capture attempts.

## Updating

TennoWorth checks for updates automatically. Downloads are available below.`;

describe("desktop release-note contract", () => {
  test("accepts one contextual title emoji and counted plain categories", () => {
    expect(validateReleaseNotes(validReleaseNotes, "0.6.7")).toEqual({
      icon: "🐧",
      changeCount: 2,
      categoryCount: 2,
    });

    const windows = validReleaseNotes.replace("🐧", "🪟");
    expect(validateReleaseNotes(windows, "0.6.7").icon).toBe("🪟");
  });

  test("extracts one version without imposing the new contract on history", () => {
    const changelog = `# Changelog

## 0.6.7 - 2026-09-04

${validReleaseNotes}

## 0.6.6 - 2026-09-03

- Historical notes stay readable.`;
    expect(releaseNotesBody(changelog, "0.6.7")).toBe(validReleaseNotes);
    expect(releaseNotesBody(changelog, "0.6.6")).toBe(
      "- Historical notes stay readable.",
    );
  });

  test("rejects the deliberately incomplete prepare scaffold", () => {
    const scaffold = releaseNotesTemplate("0.6.7", "2026-09-04");
    const body = releaseNotesBody(`# Changelog\n\n${scaffold}`, "0.6.7");
    expect(() => validateReleaseNotes(body, "0.6.7")).toThrow(
      "template placeholders",
    );
  });

  test("requires exactly one emoji and keeps it in the release title", () => {
    expect(() =>
      validateReleaseNotes(validReleaseNotes.replace("🐧 ", ""), "0.6.7"),
    ).toThrow("first line must be");

    expect(() =>
      validateReleaseNotes(
        validReleaseNotes.replace("### Linux", "### 🐧 Linux"),
        "0.6.7",
      ),
    ).toThrow("exactly one emoji");

    expect(() =>
      validateReleaseNotes(validReleaseNotes, "0.6.8"),
    ).toThrow("TennoWorth Desktop 0.6.8");
  });

  test("requires the introduction, honest count, categories, and updating copy", () => {
    const oneParagraph = validReleaseNotes.replace(
      "TennoWorth Desktop 0.6.7 is ready.\n\n",
      "",
    );
    expect(() => validateReleaseNotes(oneParagraph, "0.6.7")).toThrow(
      "announcement paragraph",
    );

    expect(() =>
      validateReleaseNotes(
        validReleaseNotes.replace("## Changelog (2)", "## Changelog (3)"),
        "0.6.7",
      ),
    ).toThrow("declares 3 changes but contains 2");

    expect(() =>
      validateReleaseNotes(
        validReleaseNotes.replace("### Linux\n\n", ""),
        "0.6.7",
      ),
    ).toThrow("must begin with a plain category heading");

    expect(() =>
      validateReleaseNotes(
        validReleaseNotes.replace("### Reliability", "### Empty\n\n### Reliability"),
        "0.6.7",
      ),
    ).toThrow('the category "Empty" has no change bullets');

    expect(() =>
      validateReleaseNotes(
        validReleaseNotes.replace(
          "## Updating\n\nTennoWorth checks for updates automatically. Downloads are available below.",
          "## Updating",
        ),
        "0.6.7",
      ),
    ).toThrow('the "## Updating" section is empty');
  });
});


test("snapshot CLI resolves its checkout from paths containing spaces and URL characters", () => {
  const root = mkdtempSync(join(tmpdir(), "tennoworth release # %-"));
  try {
    const script = join(root, "scripts", "release.ts");
    const publicDir = join(root, "frontend", "public");
    mkdirSync(join(root, "scripts"), { recursive: true });
    mkdirSync(publicDir, { recursive: true });
    copyFileSync(fileURLToPath(new URL("./release.ts", import.meta.url)), script);
    writeFileSync(join(publicDir, "market.json"), JSON.stringify({
      ...validMarket(), updated_at: new Date(Date.now() - 1000).toISOString(),
    }));
    writeFileSync(join(publicDir, "wfstat-catalog.json"), JSON.stringify(validResolver));
    const output = execFileSync(process.execPath, [script, "snapshot-check"], {
      cwd: tmpdir(), encoding: "utf8", stdio: ["ignore", "pipe", "pipe"],
    });
    expect(output).toContain("1 items, 1 market names, 1 resolver paths");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});


test("cached update offers keep immutable downloads across releases", () => {
  const workflow = readFileSync(new URL("../.github/workflows/release-desktop.yml", import.meta.url), "utf8");
  const step = workflow.split("      - name: Assemble latest.json\n")[1];
  const script = step.split("          python3 - <<'PY'\n")[1].split("\n          PY")[0]
    .split("\n").map(line => line.slice(10)).join("\n");
  const root = mkdtempSync(join(tmpdir(), "tennoworth updater-"));
  try {
    mkdirSync(join(root, "dist"));
    const manifests = ["0.7.102", "0.7.103"].map(version => {
      writeFileSync(join(root, "dist", `TennoWorth_${version}_x64-setup.exe.sig`), "windows-signature");
      writeFileSync(join(root, "dist", "TennoWorth-x86_64.AppImage.sig"), "linux-signature");
      execFileSync(process.platform === "win32" ? "python" : "python3", ["-c", script], {
        cwd: root, env: { ...process.env, VERSION: version, GH_REPO: "example/market" }, stdio: "pipe",
      });
      return JSON.parse(readFileSync(join(root, "manifest", "latest.json"), "utf8"));
    });
    for (const manifest of manifests) {
      expect(Object.keys(manifest.platforms).sort()).toEqual(["linux-x86_64", "windows-x86_64", "windows-x86_64-nsis"]);
      const base = `https://github.com/example/market/releases/download/desktop-v${manifest.version}`;
      expect(manifest.platforms["windows-x86_64"]).toEqual({ signature: "windows-signature", url: `${base}/TennoWorth_${manifest.version}_x64-setup.exe` });
      expect(manifest.platforms["windows-x86_64-nsis"]).toEqual(manifest.platforms["windows-x86_64"]);
      expect(manifest.platforms["linux-x86_64"]).toEqual({ signature: "linux-signature", url: `${base}/TennoWorth-x86_64.AppImage` });
    }
    expect(manifests[0].platforms["linux-x86_64"].url).not.toBe(manifests[1].platforms["linux-x86_64"].url);
  } finally { rmSync(root, { recursive: true, force: true }); }
});
