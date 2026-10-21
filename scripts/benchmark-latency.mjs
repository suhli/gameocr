// No cache: every measured sample runs actual OCR. Each variant gets its own process.
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {engineConfig,startExecutor,resources} from './executor-driver.mjs';
import {startNative} from './onnx-worker-driver.mjs';
import {quantile,accuracy} from './ocr-quality.mjs';
const destination=process.env.GAMEOCR_LATENCY_REPORT??'docs/test-results/latency-baseline.json';
const report={executedAt:new Date().toISOString(),cpu:os.cpus()[0]?.model,scope:'Native: decode through result; executor: PPM transfer through merge. Neither includes WGC or UI. First frame separate; three warm samples. No cache.',results:[],unsupported:['CUDA Adaptive: existing scheduler supports CPU only','RTX 5060 Ti unavailable']};
const save=()=>fs.writeFileSync(destination,JSON.stringify(report,null,2)+'\n');
const corpus=JSON.parse(fs.readFileSync('tests/fixtures/small-text/corpus.json','utf8').replace(/^\uFEFF/,''));
for(const device of ['cpu','cuda'])for(const strategy of ['native','single','manual','adaptive']){
  if(device==='cuda'&&strategy==='adaptive')continue;
  const engine=engineConfig(device);const native=strategy==='native';
  const worker=native?await startNative(engine):await startExecutor('latency-'+device+'-'+strategy,engine,
    {enableParallelInference:strategy==='manual',enableCpuWorkers:false,gpuWorkerCount:2,cpuWorkerCount:2},undefined,{executionMode:strategy});
  try{
    assert.equal(worker.ready.type,'ready',JSON.stringify(worker.ready));
    for(const scene of corpus.scenarios.filter(s=>s.width===2560)){
      const image=path.resolve('tests/fixtures/small-text/dense-2560.png');const runs=[];
      for(let i=0;i<4;i++){
        const response=native?await worker.recognize(image):await worker.request({type:'recognize',image,frameId:i+1});
        assert.equal(response.type,'result',JSON.stringify(response));
        const result=response.result;const quality=accuracy(scene.groundTruth,result.blocks);
        runs.push({metrics:result.metrics,quality});
      }
      const samples=runs.slice(1).map(r=>r.metrics.execution?.endToEndMs??r.metrics.totalMs);
      const row={device,strategy,load:worker.ready,first:runs[0],warm:runs.slice(1),meanMs:samples.reduce((a,b)=>a+b,0)/samples.length,p50Ms:quantile(samples,.5),p95Ms:quantile(samples,.95),p99Ms:quantile(samples,.99),resources:resources(worker.child?[worker.child.pid,...(worker.ready.executor?.workers??[]).map(w=>w.pid)]:[])};
      if(native)row.workerSha256=createHash('sha256').update(fs.readFileSync(worker.executable)).digest('hex');
      report.results.push(row);save();console.log(JSON.stringify({device,strategy,p50Ms:row.p50Ms,quality:runs[1].quality}));
    }
  }finally{await worker.close();}
}
save();
