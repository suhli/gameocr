import fs from 'node:fs';import path from 'node:path';import assert from 'node:assert/strict';
import {engineConfig,resources} from './executor-driver.mjs';import {startNative} from './onnx-worker-driver.mjs';
import {accuracy,compareOutputs,quantile} from './ocr-quality.mjs';
const corpusPath=path.resolve(process.env.GAMEOCR_TEMPORAL_CORPUS??'tests/fixtures/temporal/corpus.json');
const corpus=JSON.parse(fs.readFileSync(corpusPath,'utf8'));const device=process.env.GAMEOCR_TEST_DEVICE??'cpu';
const destination=process.env.GAMEOCR_TEMPORAL_REPORT??`docs/test-results/temporal-${device}.json`;
const report={executedAt:new Date().toISOString(),device,source:corpus.source,scope:'Native worker, file decode included; no WGC/UI. Sequential frames at maximum processing rate; atMs describes corpus order, not actual scheduling.',results:[]};
if(process.env.GAMEOCR_TEMPORAL_RESUME==='1'&&fs.existsSync(destination))report.results=JSON.parse(fs.readFileSync(destination,'utf8')).results;
const save=()=>{const temp=destination+'.tmp';for(let attempt=0;;attempt++){try{fs.writeFileSync(temp,JSON.stringify(report,null,2)+'\n');fs.renameSync(temp,destination);return;}catch(e){if(attempt>=8)throw e;Atomics.wait(new Int32Array(new SharedArrayBuffer(4)),0,0,100);}}};
for(const scene of corpus.scenarios.filter(s=>!process.env.GAMEOCR_TEMPORAL_SCENE||s.name.includes(process.env.GAMEOCR_TEMPORAL_SCENE))){
  if(report.results.filter(r=>r.scene===scene.name).length===5)continue;
  report.results=report.results.filter(r=>r.scene!==scene.name);
  const baseline=[];
  for(const variant of ['full','old-cache','cache-only','rec-only','auto']){
    const engine=engineConfig(device);Object.assign(engine.inference.parameters,scene.parameters??{},
      {incremental:variant==='old-cache',temporal:{enabled:!['full','old-cache'].includes(variant),strategy:variant==='old-cache'?'auto':variant,refreshMs:1000,maxReuse:30,minConfidence:.85,edgeDelta:12}});
    const worker=await startNative(engine);const runs=[];
    try{
      assert.equal(worker.ready.type,'ready',JSON.stringify(worker.ready));
      for(const [index,frame] of scene.frames.entries()){
        const response=await worker.request({type:'recognize',image:path.resolve(path.dirname(corpusPath),frame.image),frame_id:index+1,timestamp:Date.now(),mode:'grounding',cache_context:[frame.generation??1,frame.epoch??1,0]});
        assert.equal(response.type,'result',JSON.stringify(response));const result=response.result;
        if(variant==='full')baseline.push(result.blocks);
        const quality=accuracy(frame.groundTruth,result.blocks);
        const comparison=compareOutputs(baseline[index],result.blocks);
        runs.push({frame:index,metrics:result.metrics,quality,comparison,blocks:result.blocks});
        if(result.metrics.temporal?.path==='cache-only'){assert.equal(result.metrics.pipeline.detectorCalls,0);assert.equal(result.metrics.pipeline.recognizerCalls,0);}
        if(result.metrics.temporal?.path==='rec-only')assert.equal(result.metrics.pipeline.detectorCalls,0);
      }
      const times=runs.slice(2).map(r=>r.metrics.totalMs);const row={scene:scene.name,variant,loadMs:worker.ready.load_ms,firstMs:runs[0].metrics.totalMs,p50Ms:quantile(times,.5),p95Ms:quantile(times,.95),p99Ms:quantile(times,.99),meanMs:times.reduce((a,b)=>a+b,0)/times.length,
        detectorCalls:runs.reduce((n,r)=>n+r.metrics.pipeline.detectorCalls,0),recognizerCalls:runs.reduce((n,r)=>n+r.metrics.pipeline.recognizerCalls,0),
        disagreements:runs.filter(r=>r.comparison.disagreement).length,resources:resources([worker.child.pid]),runs};
      report.results.push(row);save();console.log(JSON.stringify({scene:scene.name,variant,p50Ms:row.p50Ms,disagreements:row.disagreements,paths:runs.map(r=>r.metrics.temporal?.path)}));
    }finally{await worker.close();}
  }
}
save();
