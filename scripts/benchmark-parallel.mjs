// Real inference on labelled inputs. Ground truth is used only for evaluation.
import fs from 'node:fs';import path from 'node:path';import os from 'node:os';
import {spawn} from 'node:child_process';
import {createHash} from 'node:crypto';
import {engineConfig,startExecutor,resources} from './executor-driver.mjs';
const corpusPath=path.resolve(process.env.GAMEOCR_BENCH_CORPUS??'tests/fixtures/parallel/corpus.json');
const corpus=JSON.parse(fs.readFileSync(corpusPath,'utf8').replace(/^\uFEFF/,''));
const rounds=Number(process.env.GAMEOCR_BENCH_ROUNDS??5),warmups=2;
if(!Number.isInteger(rounds)||rounds<3||rounds>100)throw Error('Use 3–100 measured rounds');
const modes=[['cpu-1','cpu',false,0,1],['gpu-1','cuda',false,1,0],['cpu-2','cpu',true,0,2],['gpu-2','cuda',true,2,0],['mixed-1-1','auto',true,1,1],['mixed-2-2','auto',true,2,2]];
if(process.env.GAMEOCR_BENCH_ATTRIBUTION==='1')modes.push(['cpu-1-raw','cpu',true,0,1],['gpu-1-raw','cuda',true,1,0]);
const selected=process.env.GAMEOCR_BENCH_MODES?.split(',');const inputs=process.env.GAMEOCR_BENCH_INPUTS?.split(',');
const output=path.resolve(process.env.GAMEOCR_BENCH_REPORT??'docs/test-results/parallel-benchmark.json');
const report={executedAt:new Date().toISOString(),source:corpus.source,coordinateGroundTruth:corpus.coordinateGroundTruth,environment:{platform:os.platform(),release:os.release(),cpu:os.cpus()[0]?.model,logicalCores:os.availableParallelism()},rounds,warmups,notes:['Corpus image decoding is excluded; frame transfer, splitting, queue, execution and merge are included.','GPU snapshots cover the entire device including unrelated applications.','Ground-truth boxes are font layout rectangles; coordinate IoU is approximate.'],runs:[]};
report.engine=engineConfig();report.environment.node=process.version;
report.binarySha256=Object.fromEntries([process.env.GAMEOCR_EXECUTOR_BENCH??'target/release/executor-bench.exe','src-tauri/resources/workers/onnx/cpu/onnx-worker.exe','src-tauri/resources/workers/onnx/cuda/onnx-worker.exe'].filter(p=>fs.existsSync(p)).map(p=>[p,createHash('sha256').update(fs.readFileSync(p)).digest('hex')]));
fs.mkdirSync(path.dirname(output),{recursive:true});
const save=()=>fs.writeFileSync(output,JSON.stringify(report,null,2)+'\n');
const quantile=(values,p)=>[...values].sort((a,b)=>a-b)[Math.max(0,Math.ceil(values.length*p)-1)];
const norm=s=>s.normalize('NFKC').replace(/\s+/gu,'').toLowerCase();
function distance(a,b){a=[...a];b=[...b];let prev=b.map((_,i)=>i+1);prev.unshift(0);for(let i=0;i<a.length;i++){let next=[i+1];for(let j=0;j<b.length;j++)next.push(Math.min(prev[j]+(a[i]===b[j]?0:1),prev[j+1]+1,next[j]+1));prev=next;}return prev[b.length];}
function iou(a,b){const inter=Math.max(0,Math.min(a[2],b[2])-Math.max(a[0],b[0]))*Math.max(0,Math.min(a[3],b[3])-Math.max(a[1],b[1]));const area=r=>Math.max(0,r[2]-r[0])*Math.max(0,r[3]-r[1]);return inter/Math.max(1,area(a)+area(b)-inter);}
function accuracy(expected,blocks){
  const used=new Set();let exact=0,matched=0,errors=0,characters=0,overlap=0,center=0;
  for(const gt of expected){const candidates=blocks.map((b,i)=>({b,i,overlap:b.bbox?iou(gt.bbox,b.bbox):0})).filter(v=>!used.has(v.i)).sort((a,b)=>b.overlap-a.overlap);const best=candidates[0]?.overlap>0.05?candidates[0]:null;
    const text=norm(gt.text);characters+=[...text].length;
    if(best){used.add(best.i);matched++;const recognized=norm(best.b.text);errors+=distance(text,recognized);if(text===recognized)exact++;overlap+=best.overlap;const a=gt.bbox,b=best.b.bbox;center+=Math.hypot((a[0]+a[2]-b[0]-b[2])/2,(a[1]+a[3]-b[1]-b[3])/2);}else errors+=[...text].length;
  }
  for(const [i,b] of blocks.entries())if(!used.has(i))errors+=[...norm(b.text)].length;
  return {exactLineRecall:expected.length?exact/expected.length:Number(blocks.length===0),locatedLineRecall:expected.length?matched/expected.length:Number(blocks.length===0),characterErrorRate:errors/Math.max(1,characters),meanBoxIoU:matched?overlap/matched:null,meanCenterErrorPx:matched?center/matched:null,unexpectedBlocks:blocks.length-used.size};
}
let frameId=0;
const parameters=[[6,960]];
if(process.env.GAMEOCR_BENCH_TUNING==='1')parameters.push([1,960],[12,960],[6,640],[6,1280]);
for(const [batchSize,detectionResize] of parameters)for(const [name,device,enabled,gpu,cpu] of modes){
  if(process.env.GAMEOCR_BENCH_PARAMETERS&&!process.env.GAMEOCR_BENCH_PARAMETERS.split(',').includes(`${batchSize}:${detectionResize}`))continue;
  if(selected&&!selected.includes(name))continue;
  const run={name,batchSize,detectionResize,status:'running',scenes:[]};report.runs.push(run);save();
  let driver,sampler;const gpuSamples=[];
  try{
    driver=await startExecutor('bench-'+name+'-'+batchSize+'-'+detectionResize,engineConfig(device,batchSize,detectionResize),{enableParallelInference:enabled,enableCpuWorkers:cpu>0,gpuWorkerCount:gpu,cpuWorkerCount:cpu});
    run.load=driver.ready;if(driver.ready.type!=='ready'){run.status='load-failed';continue;}
    const pids=[driver.child.pid,...driver.ready.executor.workers.map(w=>w.pid)];
    run.resourcesBefore=resources(pids);
    // One persistent sampler; no process launch in each timed frame.
    sampler=spawn('nvidia-smi.exe',['--id=0','--query-gpu=timestamp,utilization.gpu,memory.used','--format=csv,noheader,nounits','--loop-ms=1000'],{windowsHide:true,stdio:['ignore','pipe','ignore']});
    let buffer='';sampler.on('error',()=>{});sampler.stdout.on('data',data=>{buffer+=data;const lines=buffer.split('\n');buffer=lines.pop();gpuSamples.push(...lines.map(line=>line.trim()).filter(Boolean));});
    for(const scene of corpus.scenarios){
      if(inputs&&!inputs.includes(scene.name))continue;
      const image=path.resolve(path.dirname(corpusPath),scene.image);
      for(let i=0;i<warmups;i++){const response=await driver.request({type:'recognize',image,frameId:++frameId});if(response.type!=='result')throw Error(JSON.stringify(response));}
      const samples=[];
      for(let i=0;i<rounds;i++){
        const response=await driver.request({type:'recognize',image,frameId:++frameId});if(response.type!=='result')throw Error(JSON.stringify(response));
        const {result}=response;const quality=accuracy(scene.groundTruth,result.blocks);
        samples.push({wallMs:response.wallMs,execution:result.metrics.execution,accuracy:quality,device:result.device,blocks:result.blocks,workers:response.executor.workers});
      }
      const times=samples.map(s=>s.wallMs);run.scenes.push({name:scene.name,width:scene.width,height:scene.height,medianMs:quantile(times,.5),p95Ms:quantile(times,.95),framesPerSecond:1000/(times.reduce((a,b)=>a+b,0)/times.length),samples});
      console.log(name+' '+scene.name+': median '+quantile(times,.5).toFixed(1)+' ms, P95 '+quantile(times,.95).toFixed(1)+' ms');save();
    }
    run.resourcesAfter=resources(pids);run.status='passed';
    const processList=s=>Array.isArray(s.processes)?s.processes:[s.processes];
    const cpuSeconds=s=>processList(s).reduce((sum,p)=>sum+(p?.CPU??0),0);
    const elapsedSeconds=(Date.parse(run.resourcesAfter.at)-Date.parse(run.resourcesBefore.at))/1000;
    run.processCpuPercentOfMachine=100*(cpuSeconds(run.resourcesAfter)-cpuSeconds(run.resourcesBefore))/elapsedSeconds/os.availableParallelism();
  }catch(error){run.status='failed';run.error=String(error);console.error(name+': '+error);}
  finally{sampler?.kill();run.gpuDeviceWideSamples=gpuSamples;await driver?.close();save();}
}
for(const run of report.runs)for(const scene of run.scenes){
  const baselines=report.runs.filter(b=>['cpu-1','gpu-1'].includes(b.name)&&b.status==='passed'&&b.batchSize===run.batchSize&&b.detectionResize===run.detectionResize).flatMap(b=>b.scenes.filter(s=>s.name===scene.name).map(s=>({mode:b.name,medianMs:s.medianMs,accuracy:s.samples[0].accuracy})));
  baselines.sort((a,b)=>a.medianMs-b.medianMs);if(baselines.length)scene.comparableBaseline={...baselines[0],speedup:baselines[0].medianMs/scene.medianMs};
}
save();console.log('Saved '+output);
