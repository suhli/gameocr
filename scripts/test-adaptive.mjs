import fs from 'node:fs';import path from 'node:path';import os from 'node:os';import assert from 'node:assert/strict';import {execFileSync} from 'node:child_process';
import {engineConfig,startExecutor} from './executor-driver.mjs';
import {accuracy} from './ocr-quality.mjs';
const root=path.resolve('.cache/adaptive-tests',String(Date.now()));fs.mkdirSync(root,{recursive:true});
const profiles=path.join(root,'profiles.json'),checks=[],samples=[];let frame=0;
const corpus=JSON.parse(fs.readFileSync('tests/fixtures/parallel/corpus.json','utf8').replace(/^\uFEFF/,''));
const scene=name=>corpus.scenarios.find(s=>s.name===name);const image=name=>path.resolve('tests/fixtures/parallel',scene(name).image);
const report={executedAt:new Date().toISOString(),checks,samples,profiles:'isolated test archive',hardware:os.cpus()[0]?.model};
const open=(name,options={},engine=engineConfig())=>startExecutor('adaptive-'+path.basename(root)+'-'+name,engine,{},undefined,{executionMode:'adaptive',adaptive:{learning:true,exploration:true,maxCpuWorkers:2,maxGpuWorkers:0},profiles,...options});
const recognize=(d,name='hud-1080')=>d.request({type:'recognize',image:image(name),frameId:++frame});
function valid(r,name='hud-1080'){assert.equal(r.type,'result',JSON.stringify(r));assert.equal(r.result.frameId,frame);const a=accuracy(scene(name).groundTruth,r.result.blocks);assert.equal(a.characterErrorRate,0,JSON.stringify(a));assert.equal(a.exactLineRecall,1);assert.equal(r.executor.queuedTasks,0);assert(r.executor.workers.length<=2);assert(r.executor.workers.reduce((s,w)=>s+w.threads,0)<=Math.min(4,os.availableParallelism()));return r.result.metrics.execution.adaptive;}
let d;
try {
  d=await open('online');assert.equal(d.ready.type,'ready');assert.equal(d.ready.executor.workers.length,1);assert.equal(d.ready.executor.workers[0].threads,Math.min(4,os.availableParallelism()));
  const f1=await d.request({type:'analyze',image:image('hud-1080')}),f2=await d.request({type:'analyze',image:image('hud-1080')});assert.deepEqual(f1.features,f2.features);samples.push({name:'features',analysisMs:f1.analysisMs,features:f1.features});checks.push('Deterministic real-frame features, bounded thumbnail, true single-worker thread baseline');
  let exploration=0;
  for(let i=0;i<24;i++){const r=await recognize(d);const a=valid(r);exploration+=Number(a.exploring);samples.push({name:'online',wallMs:r.wallMs,adaptive:a,workers:r.executor.workers});}
  assert(exploration<=1);checks.push('Online real OCR: at most one exploration in 24 frames; at most two workers and bounded total thread budget');
  const before=(await d.request({type:'statistics'})).summary;
  const cancelled=await d.request({type:'cancel',image:image('dense-menu'),delayMs:5});assert.equal(cancelled.type,'error');assert.match(cancelled.message,/取消/);const after=(await d.request({type:'statistics'})).summary;assert.deepEqual(after,before);valid(await recognize(d));checks.push('Cancellation during real inference records neither success nor failure and pool remains reusable');
  await d.close();d=null;const saved=JSON.parse(fs.readFileSync(profiles,'utf8').replace(/^\uFEFF/,''));assert(saved.profiles.length>0&&saved.profiles.length<=256);assert(fs.statSync(profiles).size<=4*1024*1024);assert(!fs.readFileSync(profiles,'utf8').includes('Health:'));checks.push('Atomic archive contains bounded aggregate features/statistics, no OCR text');
  const unchanged=fs.readFileSync(profiles,'utf8');d=await open('restart',{adaptive:{learning:false,exploration:true,maxCpuWorkers:2,maxGpuWorkers:0}});const reused=valid(await recognize(d));assert(reused.similarSamples>=5);assert(!reused.exploring);await d.close();d=null;assert.equal(fs.readFileSync(profiles,'utf8'),unchanged);checks.push('Restart reuses compatible history; disabling learning is read-only and disables exploration');
  d=await open('fingerprint',{adaptive:{learning:false,exploration:false,maxCpuWorkers:2,maxGpuWorkers:0}},engineConfig('cpu',12));const isolated=valid(await recognize(d));assert.equal(isolated.similarSamples,0);await d.close();d=null;checks.push('Changed Batch Size isolates the configuration fingerprint');
  d=await open('clear');await d.request({type:'clearProfiles'});assert.equal((await d.request({type:'statistics'})).summary.classes,0);await d.close();d=null;
  fs.writeFileSync(profiles,'{invalid');d=await open('corrupt',{adaptive:{learning:true,exploration:false,maxCpuWorkers:1,maxGpuWorkers:0}});assert.equal((await d.request({type:'statistics'})).summary.recovered,true);valid(await recognize(d));assert.equal((await d.request({type:'status'})).executor.workers.length,1);checks.push('Clear is persisted; corrupt archive recovers; CPU worker cap one is respected');
  const baseline=(await d.request({type:'statistics'})).summary;const pid=(await d.request({type:'status'})).executor.workers[0].pid;
  const parent=execFileSync('powershell.exe',['-NoProfile','-Command',`(Get-CimInstance Win32_Process -Filter "ProcessId=${pid}").ParentProcessId`],{encoding:'utf8',windowsHide:true}).trim();assert.equal(Number(parent),d.child.pid);
  const pending=recognize(d,'dense-menu');await new Promise(r=>setTimeout(r,50));execFileSync('taskkill.exe',['/PID',String(pid),'/F'],{windowsHide:true,stdio:'ignore'});
  const failed=await pending;assert.equal(failed.type,'error');const failures=(await d.request({type:'statistics'})).summary;assert.equal(failures.failedFrames,baseline.failedFrames+1);assert.equal(failures.classes,baseline.classes+1);checks.push('Owned worker crash records a failed attempt and never a successful latency sample');
  await d.close();d=null;
  const cuda=await open('explicit-cuda',{},engineConfig('cuda'));assert.equal(cuda.ready.type,'error');assert.match(cuda.ready.message,/CPU|CUDA/);await cuda.close();checks.push('Adaptive rejects explicit CUDA rather than silently using CPU');
  report.status='passed';report.explorationsObserved=exploration;
}catch(e){report.status='failed';report.error=String(e);throw e;}
finally{await d?.close();fs.writeFileSync(process.env.GAMEOCR_ADAPTIVE_REPORT??'docs/test-results/adaptive-integration.json',JSON.stringify(report,null,2)+'\n');console.log('Adaptive '+report.status+': '+checks.length+' checks');}
