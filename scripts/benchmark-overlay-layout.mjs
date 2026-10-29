// Pure layout microbenchmark. Does not measure WebView, native windows or OCR.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {layoutSubtitles, overlayDefaults} from '../src/lib/overlayGeometry.ts';

const output = process.argv[2] ?? 'artifacts/overlay-layout.json';
const samples = [];
for (const [width, height] of [[1920, 1080], [3840, 2160]]) {
  for (const count of [3, 32, 64]) {
    const items = Array.from({length: count}, (_, i) => {
      const x = (i % 8) * width / 8, y = Math.floor(i / 8) * height / 8;
      return {id: String(i), text: '任务完成・次の目標 Quest complete',
        bbox: [x, y, x + width / 10, y + height / 20], sourceBlockIds: [String(i)]};
    });
    const layout = () => layoutSubtitles(items, width, height, width, height, overlayDefaults);
    for (let i = 0; i < 1000; i++) layout();
    const times = [];
    for (let i = 0; i < 10000; i++) {
      const start = performance.now();
      layout();
      times.push(performance.now() - start);
    }
    const meanMs = times.reduce((sum, value) => sum + value, 0) / times.length;
    times.sort((a, b) => a - b);
    samples.push({width, height, sourceItems: count, visibleItems: layout().length,
      iterations: times.length, meanMs, p50Ms: times[5000], p95Ms: times[9500]});
  }
}
const report = {
  executedAt: new Date().toISOString(),
  scope: 'Node pure source-coordinate mapping and mixed-text layout only; synthetic input. No desktop, paint, end-to-end latency, idle CPU/memory, or flicker measurements.',
  node: process.version, platform: process.platform, cpu: os.cpus()[0]?.model,
  sourceSha256: createHash('sha256').update(fs.readFileSync('src/lib/overlayGeometry.ts')).digest('hex'),
  samples,
};
fs.mkdirSync(path.dirname(output), {recursive: true});
fs.writeFileSync(output, JSON.stringify(report, null, 2) + '\n');
console.log(JSON.stringify(report, null, 2));
