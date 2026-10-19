import { afterEach, expect, test } from 'bun:test';
import { addModel, getModels, getRuntimeSettings, parseSse, probeModel, removeModel, saveRuntimeSettings, updateModel, updatePriceSpec, streamOrchestrate } from '../src/api';

const originalFetch = globalThis.fetch;
afterEach(() => { globalThis.fetch = originalFetch; });

test('model edit, deactivate, restore and re-register use the expected REST contract', async () => {
  const requests: Array<{ path: string; method: string; body?: string }> = [];
  globalThis.fetch = async (input, init) => {
    requests.push({ path: String(input), method: init?.method ?? 'GET', body: init?.body?.toString() });
    return Response.json({ models: [], updated: true, deleted: true, id: 1 });
  };
  await getModels(false);
  await updateModel('my/model', { api_base: 'https://example.com/v1' });
  await removeModel('my/model');
  await updateModel('my/model', { is_active: 1 });
  await addModel({ name: 'my/model', kind: 'cloud', provider: 'custom' });
  await probeModel('my/model');
  expect(requests.map(({ path, method }) => [method, path])).toEqual([
    ['GET', '/api/models?active_only=false'],
    ['PUT', '/api/models/my%2Fmodel'],
    ['DELETE', '/api/models/my%2Fmodel'],
    ['PUT', '/api/models/my%2Fmodel'],
    ['POST', '/api/models'],
    ['POST', '/api/models/my%2Fmodel/probe'],
  ]);
  expect(JSON.parse(requests[3].body!)).toEqual({ is_active: 1 });
});

test('data-only SSE errors reach the chat UI', async () => {
  globalThis.fetch = async () => new Response('data: {"error":true,"detail":"模型不可用"}\n\n');
  const events: Array<{ event: string; data: any }> = [];
  await streamOrchestrate('hello', [], (event) => events.push(event));
  expect(events).toEqual([{ event: 'message', data: { error: true, detail: '模型不可用' } }]);
});

test('SSE joins multi-line data, ignores keepalives and handles split UTF-8', async () => {
  const frame = ': ping\nevent: result\ndata: {"response":\ndata: "你好👋"}\n\n';
  expect(parseSse(frame)).toEqual([{ event: 'result', data: { response: '你好👋' } }]);
  const encoded = new TextEncoder().encode(frame);
  let index = 0;
  const body = new ReadableStream<Uint8Array>({
    pull(controller) {
      if (index === encoded.length) controller.close();
      else controller.enqueue(encoded.slice(index, ++index));
    },
  });
  globalThis.fetch = async () => new Response(body);
  const events: Array<{ event: string; data: any }> = [];
  await streamOrchestrate('hello', [], (event) => events.push(event));
  expect(events).toEqual([{ event: 'result', data: { response: '你好👋' } }]);
});

test('HTTP rejection produces a visible error event', async () => {
  globalThis.fetch = async () => new Response('forbidden', { status: 403 });
  const events: Array<{ event: string; data: any }> = [];
  await streamOrchestrate('hello', [], (event) => events.push(event));
  expect(events).toEqual([{ event: 'error', data: { error: true, detail: 'HTTP 403' } }]);
});

test('probe failures expose the backend diagnostic', async () => {
  globalThis.fetch = async () => Response.json({ error: '模型未配置可用的 API Key' }, { status: 400 });
  await expect(probeModel('unconfigured')).rejects.toThrow('模型未配置可用的 API Key');
});

test('runtime settings use a whitelisted endpoint', async () => {
  const requests: Array<{ path: string; method: string; body?: string }> = [];
  globalThis.fetch = async (input, init) => {
    requests.push({ path: String(input), method: init?.method ?? 'GET', body: init?.body?.toString() });
    return Response.json({ health_probe_sec: 120, shadow_ratio: 0, block_pii: false, block_jailbreak: true });
  };
  await getRuntimeSettings();
  await saveRuntimeSettings({ health_probe_sec: 120, shadow_ratio: 0, block_pii: false, block_jailbreak: true });
  expect(requests.map(({ method, path }) => [method, path])).toEqual([
    ['GET', '/api/settings/runtime'], ['PUT', '/api/settings/runtime'],
  ]);
  expect(JSON.parse(requests[1].body!)).toEqual({ health_probe_sec: 120, shadow_ratio: 0, block_pii: false, block_jailbreak: true });
});

test('missing cloud price can be created at the pricing endpoint', async () => {
  let request = '';
  globalThis.fetch = async (input) => {
    request = String(input);
    return Response.json({ ok: true });
  };
  await updatePriceSpec('openai', 'custom/model', { input_cost: 1e-6, output_cost: 2e-6 });
  expect(request).toBe('/api/pricing/specs/openai/custom%2Fmodel');
});
