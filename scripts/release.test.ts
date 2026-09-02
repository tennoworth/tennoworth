import { describe, expect, test } from "bun:test";

import { Buffer } from "node:buffer";

import { splitSnapshotFrame, validateSnapshotValues } from "./release";

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
