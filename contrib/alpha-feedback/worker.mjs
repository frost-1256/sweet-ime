const MAX_BODY = 48 * 1024;
const MAX_RECORDS = 32;
const invitePattern = /^[A-Za-z0-9_-]{32,128}$/;
const readingPattern = /^[\u3041-\u309f\u30a0-\u30ff\u3000-\u3003\u3008-\u301f ]+$/u;
const textPattern = /^[\u3041-\u309f\u30a0-\u30ff\u3000-\u3003\u3005\u3007-\u301f\u3400-\u4dbf\u4e00-\u9fff ]+$/u;

function validText(value, pattern) {
    return typeof value === "string" && value.trim().length > 0 &&
        value.length <= 80 && pattern.test(value);
}

function validate(value) {
    if (!value || value.schema !== 1 ||
        typeof value.version !== "string" || !/^\d+\.\d+\.\d+-(?:alpha|beta)(?:\.\d+)?$/.test(value.version) ||
        !Array.isArray(value.records) || value.records.length === 0 || value.records.length > MAX_RECORDS) {
        return null;
    }
    const records = [];
    for (const record of value.records) {
        if (!record || !validText(record.reading, readingPattern) || !validText(record.text, textPattern) ||
            (record.original !== undefined && !validText(record.original, textPattern))) {
            return null;
        }
        const { reading, text, original } = record;
        records.push(original === undefined ? { reading, text } : { reading, text, original });
    }
    return { schema: 1, version: value.version, records };
}

async function readBatch(request) {
    const decoder = new TextDecoder("utf-8", { fatal: true });
    const reader = request.body?.getReader();
    if (!reader) return null;
    let length = 0;
    let text = "";
    try {
        while (true) {
            const { done, value } = await reader.read();
            if (done) break;
            length += value.byteLength;
            if (length > MAX_BODY) {
                await reader.cancel();
                return null;
            }
            text += decoder.decode(value, { stream: true });
        }
        return validate(JSON.parse(text + decoder.decode()));
    } catch {
        return null;
    } finally {
        reader.releaseLock();
    }
}

function reply(status) {
    return new Response(null, { status, headers: { "Cache-Control": "no-store" } });
}

export default {
    async fetch(request, env) {
        if (new URL(request.url).pathname !== "/v1/batches") return reply(404);
        if (request.method !== "POST") return reply(405);
        const ip = request.headers.get("CF-Connecting-IP") ?? "unknown";
        if (!(await env.INGEST_LIMIT.limit({ key: ip })).success) return reply(429);
        const invite = request.headers.get("Authorization")?.slice(7) ?? "";
        if (!invitePattern.test(invite) || !invitePattern.test(env.ALPHA_INVITE ?? "") ||
            request.headers.get("Authorization") !== `Bearer ${env.ALPHA_INVITE}`) return reply(401);
        if (request.headers.get("Content-Type") !== "application/json") return reply(415);
        const batch = await readBatch(request);
        if (!batch) return reply(400);
        const body = JSON.stringify(batch);
        const bytes = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(body));
        const id = Array.from(new Uint8Array(bytes), b => b.toString(16).padStart(2, "0")).join("");
        try {
            await env.REPORTS.put(`v1/${id}.json`, body, {
                httpMetadata: { contentType: "application/json" },
                onlyIf: { etagDoesNotMatch: "*" },
            });
        } catch {
            return reply(503);
        }
        return reply(204);
    },
};
