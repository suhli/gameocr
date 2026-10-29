// Actual Svelte components in headless Chromium with deterministic IPC fixtures.
// Does not operate the Windows desktop, run models, or prove OS click-through.
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {createServer} from 'vite';
import {chromium, expect} from '@playwright/test';
import {previewFixture} from '../tests/overlay/preview-fixture.ts';
import {overlayDefaults} from '../src/lib/overlayGeometry.ts';

const output = path.resolve(process.env.GAMEOCR_OVERLAY_DISPLAY_OUTPUT ?? 'artifacts/overlay-display-fix');
const fixtureDir = path.resolve('artifacts/overlay-display-fixture');
fs.mkdirSync(output, {recursive: true});
fs.mkdirSync(fixtureDir, {recursive: true});
fs.writeFileSync(path.join(fixtureDir, 'index.html'), '<html><body><div id="app"></div><script type="module" src="./entry.js"></script></body></html>');
fs.writeFileSync(path.join(fixtureDir, 'entry.js'), `import {mount} from 'svelte';import Harness from './Harness.svelte';import '/src/app.css';mount(Harness,{target:document.getElementById('app')});`);
fs.writeFileSync(path.join(fixtureDir, 'Harness.svelte'), `
<script>
import CapturePreview from '/src/components/CapturePreview.svelte';
import OverlayControl from '/src/components/OverlayControl.svelte';
let state=$state(window.__fixture),settings=$state(window.__mock.settings);
window.setPreview=next=>state={...state,...next};
</script>
<div style="padding:16px"><OverlayControl onSettings={next=>settings=next} onError={error=>{throw error;}}/>
<div style="height:620px;margin-top:16px"><CapturePreview frame={state.frame} result={state.ocr} translation={state.translation} overlaySettings={settings} showTranslations={state.showTranslations??true} onSelect={()=>{}}/></div></div>`);

const fixture = previewFixture();
fixture.frame.dataUrl = 'data:image/svg+xml;base64,' + Buffer.from('<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720"><rect width="1280" height="720" fill="#26384c"/><text x="400" y="245" fill="white" font-size="40">New Game</text><text x="400" y="365" fill="white" font-size="40">Load Game</text></svg>').toString('base64');
const snapshot = {serial: 1, settings: {...overlayDefaults, overlayEnabled: true},
  identity: {frameId: 7, generation: 3, epoch: 2, revision: 5}, targetId: 'fixture',
  sourceWidth: 1280, sourceHeight: 720, geometry: {rect: [0, 0, 1280, 720], clientRect: [0, 0, 1280, 720], dpi: 96, mapping: 'fixture', foreground: true, visible: true},
  items: fixture.translation.results.map((result, index) => ({id: String(index), text: result.translatedText, bbox: fixture.ocr.blocks[index].bbox, sourceBlockIds: result.sourceBlockIds})), visible: true, reason: 'fixture'};
let browser, server;
const errors = [], checks = [];
try {
  server = await createServer({server: {host: '127.0.0.1', port: 0, strictPort: false, open: false}, logLevel: 'error'});
  await server.listen();
  const origin = `http://127.0.0.1:${server.httpServer.address().port}`;
  browser = await chromium.launch({channel: process.env.GAMEOCR_TEST_BROWSER ?? 'msedge', headless: true});
  const context = await browser.newContext({viewport: {width: 1280, height: 720}, deviceScaleFactor: 1});
  await context.addInitScript(({fixture, settings, snapshot}) => {
    const callbacks = new Map(), listeners = new Map(); let sequence = 0;
    window.isTauri = true;
    window.__fixture = structuredClone(fixture);
    window.__mock = {settings, snapshot, calls: [], acks: [], emit(event, payload) {
      for (const [id, entry] of listeners) if (entry.event === event) callbacks.get(entry.handler)?.({id, event, payload: structuredClone(payload)});
    }};
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {unregisterListener: (_event, id) => listeners.delete(id)};
    window.__TAURI_INTERNALS__ = {
      transformCallback(callback) {const id = ++sequence; callbacks.set(id, callback); return id;},
      async invoke(command, args = {}) {
        window.__mock.calls.push(command);
        if (command === 'plugin:event|listen') {const id = ++sequence; listeners.set(id, args); return id;}
        if (command === 'plugin:event|unlisten') {listeners.delete(args.eventId); return;}
        if (command === 'overlay_settings') return structuredClone(window.__mock.settings);
        if (command === 'overlay_snapshot') return structuredClone(window.__mock.snapshot);
        if (command === 'overlay_rendered') {window.__mock.acks.push(args.serial); return;}
        if (command === 'update_overlay_settings') {
          window.__mock.settings = structuredClone(args.settings);
          window.__mock.emit('overlay-settings-changed', args.settings); return;
        }
        throw Error('Unexpected IPC command: ' + command);
      },
    };
  }, {fixture, settings: overlayDefaults, snapshot});
  const page = await context.newPage(); page.on('pageerror', error => errors.push(error.message));
  await page.goto(origin + '/artifacts/overlay-display-fixture/index.html');
  const captions = page.locator('.preview-subtitles .subtitle');
  await expect(captions).toHaveCount(2); await expect(captions.first()).toContainText('新游戏');
  await expect(page.getByRole('button', {name: '游戏窗口字幕'})).toHaveAttribute('aria-pressed', 'false');
  checks.push('Preview renders completed Chinese captions with native overlay disabled');
  const before = await captions.first().boundingBox();
  await page.getByRole('button', {name: '放大', exact: true}).click();
  await expect.poll(async () => (await captions.first().boundingBox()).width).toBeGreaterThan(before.width);
  await page.getByRole('button', {name: '适应窗口', exact: true}).click();
  checks.push('Preview captions follow image zoom and fit');
  await page.screenshot({path: path.join(output, 'preview-translations.png')});
  await page.evaluate(() => window.setPreview({showTranslations: false}));
  await expect(captions).toHaveCount(0);
  await page.evaluate(() => window.setPreview({showTranslations: true}));
  await expect(captions).toHaveCount(2);
  await page.evaluate(() => window.setPreview({frame: {...window.__fixture.frame, frameId: 99}}));
  await expect(captions).toHaveCount(0);
  await page.evaluate(() => window.setPreview(window.__fixture));
  await expect(captions).toHaveCount(2);
  checks.push('Preview toggle works; a different live image cannot inherit old captions');
  await page.getByRole('button', {name: '游戏窗口字幕'}).click();
  await expect(page.getByRole('button', {name: '游戏窗口字幕'})).toHaveAttribute('aria-pressed', 'true');
  await expect(captions).toHaveCount(2);
  assert(!(await page.evaluate(() => window.__mock.calls)).some(command => /translate_once|load_model|unload_model|update_translation_settings/.test(command)));
  await page.evaluate(() => window.__mock.emit('overlay-settings-changed', {...window.__mock.settings, overlayEnabled: false}));
  await expect(page.getByRole('button', {name: '游戏窗口字幕'})).toHaveAttribute('aria-pressed', 'false');
  checks.push('Native subtitle toggle updates settings without model/inference calls; saved setting events synchronize the control');

  const overlay = await context.newPage(); overlay.on('pageerror', error => errors.push(error.message));
  await overlay.goto(origin + '/index.html?overlay');
  await expect(overlay.locator('.subtitle')).toHaveCount(2);
  await expect.poll(() => overlay.evaluate(() => window.__mock.acks.includes(1))).toBe(true);
  await overlay.screenshot({path: path.join(output, 'standalone-renderer.png'), omitBackground: true});
  await overlay.evaluate(() => window.__mock.emit('overlay-state', {...window.__mock.snapshot, serial: 2, identity: null, visible: false, items: []}));
  await expect(overlay.locator('.subtitle')).toHaveCount(0);
  await overlay.evaluate(() => window.__mock.emit('overlay-state', window.__mock.snapshot));
  await expect(overlay.locator('main')).toHaveAttribute('data-serial', '2');
  await expect(overlay.locator('.subtitle')).toHaveCount(0);
  checks.push('Real standalone renderer paints, acknowledges layout and rejects a delayed snapshot after clearing');
  assert.deepEqual(errors, []);
  fs.writeFileSync(path.join(output, 'display-report.json'), JSON.stringify({passed: true, executedAt: new Date().toISOString(), scope: 'Headless Chromium; production Svelte components and deterministic mocked IPC. No real models, WGC, HWND, OS focus or click-through verification.', checks, errors}, null, 2) + '\n');
  console.log('PASS', checks);
} finally {await browser?.close(); await server?.close();}
