// Uses a real benchmark archive; never fabricates candidate latency samples.
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {engineConfig,startExecutor} from './executor-driver.mjs';

const seed=process.env.GAMEOCR_ADAPTIVE_SEED;
assert(seed,'Set GAMEOCR_ADAPTIVE_SEED to learned.json from benchmark-adaptive.mjs');
const root=path.resolve('.cache/adaptive-switch',String(Date.now()));fs.mkdirSync(root,{recursive:true});
const profiles=path.join(root,'profiles.json');fs.copyFileSync(seed,profiles);
const image=path.resolve('tests/fixtures/parallel/hud-1440.png');
const report={executedAt:new Date().toISOString(),seedSha256:createHash('sha256').update(fs.readFileSync(seed)).digest('hex'),binarySha256:createHash('sha256').update(fs.readFileSync(process.env.GAMEOCR_EXECUTOR_BENCH??'target/release/executor-bench.exe')).digest('hex')};
let driver;
try {
  driver=await startExecutor('adaptive-switch-'+path.basename(root),engineConfig(),{},undefined,{executionMode:'adaptive',adaptive:{learning:true,exploration:false,maxCpuWorkers:2,maxGpuWorkers:0},profiles});
  assert.equal(driver.ready.type,'ready');
  const before=(await driver.request({type:'statistics'})).summary;
  report.cancelled=await driver.request({type:'cancel',image,delayMs:30});
  assert.equal(report.cancelled.type,'error');assert.match(report.cancelled.message,/取消/);
  assert.deepEqual((await driver.request({type:'statistics'})).summary,before);
  const state=(await driver.request({type:'status'})).executor;
  assert.equal(state.workers.length,2,'Measured history must actually select a replacement pool to exercise this test');
  assert(state.workers.every(w=>w.pid!==driver.ready.executor.workers[0].pid));
  report.recovered=await driver.request({type:'recognize',image,frameId:1});
  assert.equal(report.recovered.type,'result');assert.equal(report.recovered.result.metrics.execution.adaptive.strategy,'cpu-parallel-2');
  assert(report.recovered.result.blocks.some(b=>b.text.includes('Health')));
  report.status='passed';
}catch(error){report.status='failed';report.error=String(error);throw error;}
finally{await driver?.close();fs.writeFileSync('docs/test-results/adaptive-switch-cancellation.json',JSON.stringify(report,null,2)+'\n');console.log('Adaptive strategy-switch cancellation: '+report.status);}
