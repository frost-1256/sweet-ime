import assert from "node:assert/strict";
import test from "node:test";
import worker from "./worker.mjs";

const invite = "a".repeat(32);
const url = "https://feedback.example/v1/batches";
const sample = {
    schema: 1, version: "1.0.0-alpha.5",
    records: [{ reading: "こうえん", text: "講演", original: "公園" }],
};

function setup(limit = true) {
    const records = new Map();
    return {
        records,
        async post(body, authorization = `Bearer ${invite}`) {
            const request = new Request(url, {
                method: "POST",
                headers: { Authorization: authorization, "Content-Type": "application/json" },
                body: typeof body === "string" ? body : JSON.stringify(body),
            });
            return worker.fetch(request, {
                ALPHA_INVITE: invite,
                INGEST_LIMIT: { limit: async () => ({ success: limit }) },
                REPORTS: {
                    put: async (key, value, options) => {
                        assert.equal(options.onlyIf.etagDoesNotMatch, "*");
                        if (!records.has(key)) records.set(key, JSON.parse(value));
                    },
                },
            });
        },
    };
}

test("stores only the conversion fields and deduplicates a batch", async () => {
    const { records, post } = setup();
    assert.equal((await post({ ...sample, app: "private", installationId: "id" })).status, 204);
    assert.equal((await post(sample)).status, 204);
    assert.equal(records.size, 1);
    assert.deepEqual([...records.values()], [sample]);
});

test("rejects invalid credentials and rate-limited requests", async () => {
    assert.equal((await setup().post(sample, "Bearer invalid")).status, 401);
    assert.equal((await setup(false).post(sample)).status, 429);
});

test("rejects stable versions, disallowed content and oversized bodies", async () => {
    const { records, post } = setup();
    assert.equal((await post({ ...sample, version: "1.0.0" })).status, 400);
    assert.equal((await post({ ...sample, records: [{ reading: "ぱすわーど", text: "秘密123" }] })).status, 400);
    assert.equal((await post("x".repeat(48 * 1024 + 1))).status, 400);
    assert.equal(records.size, 0);
});
