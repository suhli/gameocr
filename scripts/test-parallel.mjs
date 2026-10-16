// Integration tests use the production executor and real ONNX/GGUF workers.
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {engineConfig,startExecutor,resources} from './executor-driver.mjs';

const checks=[],samples=[];
const report={executedAt:new Date().toISOString(),checks,longRun:null};
const output=path.resolve(process.env.GAMEOCR_PARALLEL_REPORT??'docs/test-results/parallel-integration.json');
const image=name=>path.resolve('tests/fixtures/parallel',name+'.png');
const parallel={enableParallelInference:true,enableCpuWorkers:false,gpuWorkerCount:0,cpuWorkerCount:2};
const delay=ms=>new Promise(r=>setTimeout(r,ms));
const cpuBudget=Math.min(4,os.availableParallelism());
assert(cpuBudget>=2,'Parallel integration requires at least two logical CPUs');
let frame=0;
const request=(driver,name='hud-1080')=>driver.request({type:'recognize',image:image(name),frameId:++frame});
function valid(response){
  assert.equal(response.type,'result',JSON.stringify(response));
  const r=response.result;assert.equal(r.frameId,frame);assert.equal(r.runtime,'onnx');
  assert.equal(r.metrics.execution.failedTasks,0);assert.equal(response.executor.queuedTasks,0);
  for(const b of r.blocks){assert(b.confidence>=0&&b.confidence<=1);assert.equal(b.polygon.length,4);assert(b.bbox[0]>=0&&b.bbox[2]<=r.sourceWidth&&b.bbox[1]>=0&&b.bbox[3]<=r.sourceHeight);}
  return r;
}
function gone(pids){
  if(!pids.length)return;
  const command=`@(Get-Process -Id @(${pids.join(',')}) -ErrorAction SilentlyContinue).Count`;
  assert.equal(Number(execFileSync('powershell.exe',['-NoProfile','-Command',command],{windowsHide:true,encoding:'utf8'}).trim()),0,'Owned worker process leaked');
}
async function withDriver(name,engine,settings,run,root){
  const driver=await startExecutor(name,engine,settings,root);
  const pids=driver.ready.executor?.workers.map(w=>w.pid).filter(Number.isInteger)??[];
  try{await run(driver);}finally{await driver.close();await delay(100);gone(pids);}
}

await withDriver('default',engineConfig(),{enableParallelInference:false},async d=>{
  assert.equal(d.ready.executor.parallel,false);assert.equal(d.ready.executor.workers.length,1);
  const r=valid(await request(d));assert.equal(r.metrics.execution.tileCount,1);assert.equal(r.blocks.length,4);
  checks.push('Default single Worker: unchanged OCR output and model identity');
});
await withDriver('cpu-pool',engineConfig(),parallel,async d=>{
  assert.equal(d.ready.executor.workers.length,2);assert(d.ready.executor.workers.every(w=>w.device==='CPU'&&w.threads===Math.floor(cpuBudget/2)));
  const r=valid(await request(d));assert.equal(r.metrics.execution.tileCount,4);assert.equal(r.blocks.length,4);assert.equal(new Set(r.blocks.map(b=>b.text)).size,4);
  const status=await d.request({type:'status'});assert(status.executor.workers.every(w=>w.completedTasks>0));checks.push('CPU-only selection ignores mixed flag; two resident workers share bounded tiles and restore coordinates');
  assert.equal(valid(await request(d,'blank')).blocks.length,0);
  const boundary=valid(await request(d,'cross-boundary'));assert(boundary.blocks.some(b=>b.text==='CROSS BOUNDARY: the complete sentence must survive'));assert(boundary.metrics.execution.wholeFrameFallback);checks.push('Blank stays whole; crossing sentence is complete without duplicates after boundary repair');
  const cancelled=await d.request({type:'cancel',image:image('dense-menu'),delayMs:30});assert.equal(cancelled.type,'error');assert.match(cancelled.message,/取消/);assert.equal(cancelled.executor.queuedTasks,0);assert(cancelled.executor.workers.every(w=>w.state==='idle'));valid(await request(d));checks.push('Cancellation drains active native calls, discards pending tiles, publishes no partial result, reuses workers');
  const victim=d.ready.executor.workers[0].pid;
  const parent=Number(execFileSync('powershell.exe',['-NoProfile','-Command',`(Get-CimInstance Win32_Process -Filter "ProcessId=${victim}").ParentProcessId`],{windowsHide:true,encoding:'utf8'}).trim());assert.equal(parent,d.child.pid);
  const pending=request(d,'dense-menu');await delay(15);execFileSync('taskkill.exe',['/PID',String(victim),'/F'],{windowsHide:true,stdio:'pipe'});
  const recovered=await pending;assert.equal(recovered.type,'result',JSON.stringify(recovered));assert.equal(recovered.result.metrics.execution.failedTasks,1);assert.equal(recovered.result.metrics.execution.retriedTasks,1);assert.equal(recovered.executor.workers.filter(w=>w.state==='failed').length,1);assert(recovered.result.blocks.length>=18);valid(await request(d));checks.push('Owned worker killed during inference: exactly one retry on survivor, complete frame, next frame succeeds');
});

// A CPU-only resource tree is assembled with hard links; the CUDA tree is absent.
const cpuRoot=path.resolve('.cache/parallel-cpu-only');const source=path.resolve('src-tauri/resources/workers/onnx/cpu'),destination=path.join(cpuRoot,'workers/onnx/cpu');fs.mkdirSync(destination,{recursive:true});
for(const name of fs.readdirSync(source)){const src=path.join(source,name),dst=path.join(destination,name);if(fs.statSync(src).isFile()&&!fs.existsSync(dst))fs.linkSync(src,dst);}
await withDriver('fallback',engineConfig('auto'),{...parallel,gpuWorkerCount:1},async d=>{
  assert.equal(d.ready.type,'ready');assert.equal(d.ready.device,'CPU');assert.match(d.ready.message,/回退 CPU/);valid(await request(d));checks.push('CPU-only resources: Auto missing-CUDA fallback performs actual OCR');
},cpuRoot);
await withDriver('explicit-cuda',engineConfig('cuda'),{...parallel,gpuWorkerCount:1},async d=>{assert.equal(d.ready.type,'error');assert.match(d.ready.message,/CUDA Worker 初始化失败/);checks.push('Explicit CUDA never silently falls back');},cpuRoot);
await withDriver('no-workers',engineConfig('cpu'),{...parallel,cpuWorkerCount:0},async d=>{assert.equal(d.ready.type,'error');checks.push('Zero effective workers rejected');});
await withDriver('thread-budget',engineConfig('cpu'),{...parallel,cpuWorkerCount:cpuBudget},async d=>{assert.equal(d.ready.type,'ready');assert(d.ready.executor.workers.every(w=>w.threads===1));checks.push(`CPU thread budget is divided across ${cpuBudget} session/worker pairs`);});

const gguf={schemaVersion:2,id:'Q8_0',name:'LightOnOCR-3 Q8_0',runtime:'llamacpp',adapter:'lightonocr',version:'3',quantization:'Q8_0',capabilities:{plain:true,grounding:true,nativeCoordinates:false,confidence:false,generation:true},files:[{role:'main',path:'LightOnOCR-3-0.8B-Q8_0.gguf',size:811843648,sha256:'b5c262bc905015e78002ea0e360bcbc7e6b3edd71d29a80857f677392f48eb15'},{role:'mmproj',path:'LightOnOCR-3-0.8B-mmproj-f16.gguf',size:204987136,sha256:'e77f2ebb738f5c7852e955f09440c407d9c54b56c6f6098900380706302e8742'}],source:'test',parameters:null,installed:true,imported:false};
if(process.env.GAMEOCR_SKIP_GGUF!=='1')await withDriver('gguf', {manifest:gguf,directory:path.resolve('models'),device:'cpu',threads:4,inference:{runtime:'llamacpp',parameters:{model:'',mmproj:'',device:'cpu',threads:4,contextSize:8192,maxTokens:2048,temperature:0.1,maxImageEdge:1280}}},parallel,async d=>{
  assert.equal(d.ready.type,'ready');assert.equal(d.ready.executor.parallel,false);assert.equal(d.ready.executor.workers.length,1);
  const r=await d.request({type:'recognize',image:path.resolve('tests/fixtures/english.png'),frameId:++frame});assert.equal(r.type,'result');assert.equal(r.result.runtime,'llamacpp');assert(r.result.blocks.some(b=>b.text.includes('Health')));assert(r.result.blocks.every(b=>b.confidence===null));checks.push('GGUF ignores enabled ONNX parallel settings and preserves single-worker inference/confidence');
});
else report.gguf='Explicitly skipped; requires separately downloaded Q8_0 + mmproj';

await withDriver('sustained',engineConfig(),parallel,async d=>{
  for(let i=0;i<10;i++)valid(await request(d,i%2?'dense-menu':'hud-1080'));
  const pids=[d.child.pid,...d.ready.executor.workers.map(w=>w.pid)];
  const start=Date.now(),duration=Number(process.env.GAMEOCR_LONG_RUN_SECONDS??180)*1000;let count=0,nextSample=0;
  while(Date.now()-start<duration){const response=await request(d,count%2?'dense-menu':'hud-1080');valid(response);assert(response.executor.workers.every(w=>w.state==='idle'));assert.equal(fs.readdirSync(d.cache).length,0);count++;
    if(Date.now()-start>=nextSample){samples.push({elapsedMs:Date.now()-start,frames:count,...resources(pids)});nextSample+=30000;console.log('Sustained CPU2: '+count+' frames, '+Math.round((Date.now()-start)/1000)+' seconds');}
  }
  samples.push({elapsedMs:Date.now()-start,frames:count,...resources(pids)});
  const memory=sample=>(Array.isArray(sample.processes)?sample.processes:[sample.processes]).reduce((sum,p)=>sum+(p?.PrivateMemorySize64??0),0);
  assert(samples.every(s=>Array.isArray(s.processes)&&s.processes.length===pids.length),'Process memory telemetry unavailable');
  const growth=memory(samples.at(-1))-memory(samples[0]);assert(growth<128*1024*1024,'Private memory grew by more than 128 MiB after warmup');
  report.longRun={durationMs:Date.now()-start,frames:count,privateMemoryGrowthBytes:growth,samples};checks.push('Sustained CPU2: bounded batch/queue, stable memory after warmup, temporary files cleaned per frame');
});
checks.push('All tested pools shut down their owned worker processes');fs.mkdirSync(path.dirname(output),{recursive:true});fs.writeFileSync(output,JSON.stringify(report,null,2)+'\n');console.log('Passed '+checks.length+' real executor checks.');
