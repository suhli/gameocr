// Independent same-input calibration followed by held-out evaluation. No labels enter OCR.
import fs from 'node:fs';import path from 'node:path';import os from 'node:os';import {createHash} from 'node:crypto';
import {Worker} from 'node:worker_threads';
import assert from 'node:assert/strict';
import {engineConfig,startExecutor,resources} from './executor-driver.mjs';
import {accuracy,compareOutputs,quantile} from './ocr-quality.mjs';
const root=path.resolve('.cache/adaptive-benchmark',String(Date.now()));fs.mkdirSync(root,{recursive:true});
const read=p=>JSON.parse(fs.readFileSync(p,'utf8').replace(/^\uFEFF/,''));
const training=read('tests/fixtures/parallel/corpus.json'),evaluation=read('tests/fixtures/adaptive/corpus.json');
const rounds=Number(process.env.GAMEOCR_ADAPTIVE_ROUNDS??5);if(!Number.isInteger(rounds)||rounds<5||rounds>30)throw Error('Use 5–30 evaluation rounds');
const output=process.env.GAMEOCR_ADAPTIVE_BENCH_REPORT??'docs/test-results/adaptive-benchmark.json';
const report={executedAt:new Date().toISOString(),environment:{platform:os.platform(),release:os.release(),cpu:os.cpus()[0]?.model,logicalCores:os.availableParallelism(),node:process.version},engine:engineConfig(),rounds,trainingRounds:7,trainingWarmups:2,evaluationWarmups:0,
  notes:['Synthetic scenes, not real game accuracy. Training and evaluation PNGs are disjoint; held-out text and style are not passed to the learner.','Evaluation includes first-frame shape costs and every strategy switch. Cold model initialization is reported separately; worker load includes native warmup.','Timing excludes PNG corpus decoding but includes analysis, decision, switching, image transfer, OCR and merging.','Training observes identical input per candidate in separate serialized runs. Online exploration on another frame is not strict A/B.','Resource snapshots include host and currently resident workers; retired process CPU time is not available, so no aggregate CPU utilization is invented.','No background training or simultaneous alternative pools.'],training:[],features:[],runs:[]};
const hash=p=>createHash('sha256').update(fs.readFileSync(p)).digest('hex');
report.binarySha256=Object.fromEntries([process.env.GAMEOCR_EXECUTOR_BENCH??'target/release/executor-bench.exe','src-tauri/resources/workers/onnx/cpu/onnx-worker.exe','src-tauri/resources/workers/onnx/cpu/onnxruntime.dll'].map(p=>[p,hash(p)]));
report.datasets={training:training.scenarios.map(s=>({name:s.name,sha256:hash(path.join('tests/fixtures/parallel',s.image))})),evaluation:evaluation.scenarios.map(s=>({name:s.name,group:s.group,sha256:hash(path.join('tests/fixtures/adaptive',s.image))}))};
if(report.datasets.training.some(a=>report.datasets.evaluation.some(b=>a.sha256===b.sha256)))throw Error('Training/evaluation leakage');
const save=()=>{
  const temporary=output+'.tmp';fs.writeFileSync(temporary,JSON.stringify(report,null,2)+'\n');
  for(let attempt=0;;attempt++)try{fs.renameSync(temporary,output);return;}catch(error){if(attempt===20)throw error;Atomics.wait(new Int32Array(new SharedArrayBuffer(4)),0,0,50);}
};
let frame=0;const samplesByScene=new Map(),seed=path.join(root,'learned.json');
const load=(name,mode,profiles,adaptive={learning:true,exploration:false,maxCpuWorkers:2,maxGpuWorkers:0})=>startExecutor('adaptive-bench-'+path.basename(root)+'-'+name,engineConfig(),{enableParallelInference:mode==='manual',enableCpuWorkers:false,gpuWorkerCount:0,cpuWorkerCount:2},undefined,{executionMode:mode,adaptive,profiles});
const recognize=async(d,image)=>{const r=await d.request({type:'recognize',image:path.resolve(image),frameId:++frame});if(r.type!=='result')throw Error(JSON.stringify(r));return r;};
const snapshot=async d=>{const s=(await d.request({type:'status'})).executor;return resources([d.child.pid,...s.workers.map(w=>w.pid)]);};
function summarize(samples){const times=samples.map(s=>s.wallMs);return {frames:samples.length,medianMs:quantile(times,.5),p95Ms:quantile(times,.95),meanMs:times.reduce((a,b)=>a+b,0)/times.length,framesPerSecond:1000/(times.reduce((a,b)=>a+b,0)/times.length),fallbackRate:samples.filter(s=>s.execution.wholeFrameFallback).length/samples.length,failures:samples.reduce((n,s)=>n+s.execution.failedTasks,0),retries:samples.reduce((n,s)=>n+s.execution.retriedTasks,0),strategyCounts:Object.fromEntries(['cpu-single','cpu-parallel-2'].map(id=>[id,samples.filter(s=>s.strategy===id).length])),explorations:samples.filter(s=>s.execution.adaptive?.exploring).length,analysisMedianMs:quantile(samples.map(s=>s.execution.adaptive?.analysisMs??0),.5),decisionMedianMs:quantile(samples.map(s=>s.execution.adaptive?.decisionMs??0),.5),switchTotalMs:samples.reduce((n,s)=>n+(s.execution.adaptive?.switchMs??0),0)};}
for(const [id,mode] of [['cpu-single','single'],['cpu-parallel-2','manual']]){
  let d;const run={strategy:id,scenes:[],status:'running'};report.training.push(run);save();
  try{
    d=await load('train-'+id,mode,seed);if(d.ready.type!=='ready')throw Error(JSON.stringify(d.ready));run.load=d.ready;
    for(const scene of training.scenarios){
      const image=path.resolve('tests/fixtures/parallel',scene.image);for(let i=0;i<2;i++)await recognize(d,image);
      const samples=[];
      for(let i=0;i<7;i++){
        const r=await recognize(d,image),quality=accuracy(scene.groundTruth,r.result.blocks),execution=r.result.metrics.execution;
        const comparison=id==='cpu-parallel-2'?compareOutputs(samplesByScene.get(scene.name),r.result.blocks):null;
        if(id==='cpu-single')samplesByScene.set(scene.name,r.result.blocks);
        samples.push({wallMs:r.wallMs,execution,accuracy:quality,comparison,blocks:r.result.blocks,strategy:id});
        const observed=await d.request({type:'observe',image,strategy:id,latencyMs:r.wallMs,blocks:r.result.blocks.length,fallback:execution.wholeFrameFallback,retries:execution.retriedTasks,qualityRisk:quality.characterErrorRate>0||Boolean(comparison?.disagreement)});if(observed.type!=='observed')throw Error(JSON.stringify(observed));
      }
      run.scenes.push({name:scene.name,...summarize(samples),samples});console.log('Train '+id+' '+scene.name+' '+quantile(samples.map(s=>s.wallMs),.5).toFixed(1)+' ms');save();
    }
    if(id==='cpu-single')for(const scene of training.scenarios){const times=[];let features;for(let i=0;i<32;i++){const r=await d.request({type:'analyze',image:path.resolve('tests/fixtures/parallel',scene.image)});times.push(r.analysisMs);features=r.features;}report.features.push({scene:scene.name,medianMs:quantile(times,.5),p95Ms:quantile(times,.95),features});}
    await d.request({type:'flushProfiles'});run.status='passed';
  }catch(e){run.status='failed';run.error=String(e);throw e;}finally{await d?.close();save();}
}
report.learnedSummary=read(seed).summary;
const modes=[['cpu-1','single',false,false,false],['cpu-2','manual',false,false,false],['adaptive-cold','adaptive',false,true,false],['adaptive-history','adaptive',true,false,false],['adaptive-no-explore','adaptive',true,true,false],['adaptive-explore','adaptive',true,true,true]];
const alternating=['heldout-hud','heldout-dialogue','heldout-menu','heldout-hud','heldout-menu','style-transfer','novel-center','heldout-dialogue'];
for(const [name,mode,history,learning,exploration] of modes){
  let d;const run={name,history,learning,exploration,status:'running',samples:[],scenes:[]};report.runs.push(run);save();
  const profile=path.join(root,name+'.json');if(history)fs.copyFileSync(seed,profile);
  try {
    d=await load(name,mode,profile,{learning,exploration,maxCpuWorkers:2,maxGpuWorkers:0});if(d.ready.type!=='ready')throw Error(JSON.stringify(d.ready));run.load=d.ready;run.resourcesBefore=await snapshot(d);
    for(const scene of evaluation.scenarios){const samples=[];
      for(let i=0;i<rounds;i++){
        const r=await recognize(d,path.join('tests/fixtures/adaptive',scene.image));const execution=r.result.metrics.execution;
        const sample={scene:scene.name,phase:'blocked-layouts',wallMs:r.wallMs,execution,strategy:execution.adaptive?.strategy??(mode==='manual'?'cpu-parallel-2':'cpu-single'),accuracy:accuracy(scene.groundTruth,r.result.blocks),blocks:r.result.blocks,workers:r.executor.workers};samples.push(sample);run.samples.push(sample);
      }
      run.scenes.push({name:scene.name,group:scene.group,...summarize(samples)});console.log(name+' '+scene.name+': '+quantile(samples.map(s=>s.wallMs),.5).toFixed(1)+' ms');save();
    }
    for(let round=0;round<3;round++)for(const name of alternating){const scene=evaluation.scenarios.find(s=>s.name===name);const r=await recognize(d,path.join('tests/fixtures/adaptive',scene.image));const execution=r.result.metrics.execution;run.samples.push({scene:name,phase:'alternating',wallMs:r.wallMs,execution,strategy:execution.adaptive?.strategy??(mode==='manual'?'cpu-parallel-2':'cpu-single'),accuracy:accuracy(scene.groundTruth,r.result.blocks),blocks:r.result.blocks,workers:r.executor.workers});}
    run.summary=summarize(run.samples);run.alternating=summarize(run.samples.filter(s=>s.phase==='alternating'));run.resourcesAfter=await snapshot(d);run.finalStatistics=(await d.request({type:'statistics'})).summary;run.status='passed';
  }catch(e){run.status='failed';run.error=String(e);throw e;}finally{await d?.close();save();}
}
for(const run of report.runs)for(const scene of run.scenes){const base=report.runs.find(r=>r.name==='cpu-1').scenes.find(s=>s.name===scene.name);scene.sameInputCpuBaseline={medianMs:base.medianMs,speedup:base.medianMs/scene.medianMs};}
// A separate controlled contention experiment, excluded from all six timing comparisons.
// Only these owned threads are terminated; the user's applications are untouched.
let cancelDriver;
try {
  const cancelProfile=path.join(root,'cancel-switch.json');fs.copyFileSync(seed,cancelProfile);
  cancelDriver=await load('cancel-switch','adaptive',cancelProfile);
  assert.equal(cancelDriver.ready.type,'ready');
  const before=(await cancelDriver.request({type:'statistics'})).summary;
  const cancelled=await cancelDriver.request({type:'cancel',image:path.resolve('tests/fixtures/adaptive/heldout-hud.png'),delayMs:30});
  assert.equal(cancelled.type,'error');assert.match(cancelled.message,/取消/);
  assert.deepEqual((await cancelDriver.request({type:'statistics'})).summary,before);
  const after=(await cancelDriver.request({type:'status'})).executor;
  const recovered=await recognize(cancelDriver,'tests/fixtures/adaptive/heldout-hud.png');
  report.cancelSwitch={status:'passed',switchedDuringCancellation:after.workers[0]?.pid!==cancelDriver.ready.executor.workers[0]?.pid,cancelled,subsequentFrame:recovered.result.metrics.execution};
}finally{await cancelDriver?.close();save();}
const driftProfile=path.join(root,'load-change.json');fs.copyFileSync(seed,driftProfile);
report.loadChange={notes:['Finite CPU contention followed by recovery; reported separately from throughput comparisons. Recalibration is reported only when actually observed.'],initialRecalibrations:read(seed).profiles.reduce((n,p)=>n+Object.values(p.strategies).reduce((m,s)=>m+s.recalibrations,0),0),samples:[]};
let driftDriver;const burners=[];
try {
  driftDriver=await load('load-change','adaptive',driftProfile,{learning:true,exploration:true,maxCpuWorkers:2,maxGpuWorkers:0});
  if(driftDriver.ready.type!=='ready')throw Error(JSON.stringify(driftDriver.ready));
  for(const phase of ['before','contended','recovery']) {
    if(phase==='contended')for(let i=0;i<Math.min(8,os.availableParallelism());i++)burners.push(new Worker('const end=Date.now()+30000;let x=1;while(Date.now()<end){for(let i=0;i<100000;i++)x=Math.sqrt(x+1.01);}',{eval:true}));
    if(phase==='recovery')await Promise.all(burners.map(w=>w.terminate()));
    for(let i=0;i<8;i++) {const r=await recognize(driftDriver,path.join('tests/fixtures/adaptive','heldout-hud.png'));report.loadChange.samples.push({phase,wallMs:r.wallMs,execution:r.result.metrics.execution,blocks:r.result.blocks});}
    console.log('Load change: '+phase);save();
  }
  await driftDriver.request({type:'flushProfiles'});
  report.loadChange.statistics=(await driftDriver.request({type:'statistics'})).summary;
  report.loadChange.recalibrations=read(driftProfile).profiles.reduce((n,p)=>n+Object.values(p.strategies).reduce((m,s)=>m+s.recalibrations,0),0);
  report.loadChange.status='passed';
}finally{await Promise.all(burners.map(w=>w.terminate()));await driftDriver?.close();save();}
report.completedAt=new Date().toISOString();save();console.log('Saved '+output);
