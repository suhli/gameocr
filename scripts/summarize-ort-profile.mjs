import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
const [directory, destination = 'docs/test-results/ctc-provider-profile.json'] = process.argv.slice(2);
if (!directory) throw Error('Usage: node scripts/summarize-ort-profile.mjs PROFILE_DIRECTORY [REPORT]');
const files = fs.readdirSync(directory).filter(f => f.endsWith('.json')).sort();
if (!files.length) throw Error('No ORT profile files');
const profiles = files.map(file => {
  const bytes = fs.readFileSync(path.join(directory, file));
  const nodes = JSON.parse(bytes).filter(e => e.args?.provider);
  const providers = {}, operations = {};
  for (const node of nodes) {
    const provider = node.args.provider, operation = node.args.op_name ?? 'unknown';
    for (const [group, key] of [[providers, provider], [operations, provider + ':' + operation]]) {
      group[key] ??= {events: 0, nodeDurationUs: 0};
      group[key].events++;
      group[key].nodeDurationUs += node.dur ?? 0;
    }
  }
  return {file, sha256: createHash('sha256').update(bytes).digest('hex'), providers, operations,
    cpuNodes: nodes.filter(e => e.args.provider === 'CPUExecutionProvider').map(e => ({name: e.name, op: e.args.op_name, durationUs: e.dur})),
    reductions: nodes.filter(e => /ArgMax|GreaterOrEqual|LessOrEqual|ReduceMax|ReduceMin|And|Cast/.test(e.args.op_name ?? '')).map(e => ({name: e.name, op: e.args.op_name, provider: e.args.provider, durationUs: e.dur}))};
});
fs.writeFileSync(destination, JSON.stringify({executedAt: new Date().toISOString(),
  scope: 'ORT load-time warmup only: detector 1x3x32x32, recognizer 1x3x48x320. Node event duration is not steady-state GPU kernel timing. Raw profiles remain in the supplied local directory.',
  profiles}, null, 2) + '\n');
console.log(JSON.stringify(profiles.map(p => ({file: p.file, providers: p.providers, reductions: p.reductions})), null, 2));
