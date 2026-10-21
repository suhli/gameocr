import fs from 'node:fs';import path from 'node:path';import assert from 'node:assert/strict';
import {engineConfig} from './executor-driver.mjs';import {startNative} from './onnx-worker-driver.mjs';import {compareOutputs,quantile} from './ocr-quality.mjs';
const report={executedAt:new Date().toISOString(),scope:'CUDA Small; independent experimental graph, unchanged full graph available for fallback; native decode included',results:[]};
const baseline=new Map();
for(const compact of [false,true]){
 const engine=engineConfig('cuda');
 if(compact){engine.directory=path.resolve('.cache/optimized-models/small-compact-ctc');engine.manifest=JSON.parse(fs.readFileSync(path.join(engine.directory,'ppocrv6-small-compact-ctc-experimental.json'),'utf8'));engine.inference.parameters.experimentalGpuCtc=true;}
 const w=await startNative(engine);try{
  assert.equal(w.ready.type,'ready',JSON.stringify(w.ready));
  for(const image of ['tests/fixtures/english.png','tests/fixtures/mixed-cjk.png','tests/fixtures/blank.png','tests/fixtures/small-text/dense-2560.png']){
   const runs=[];for(let i=0;i<4;i++){const r=await w.recognize(image);assert.equal(r.type,'result',JSON.stringify(r));runs.push(r.result);}
   if(!compact)baseline.set(image,runs[0]);
   const comparison=compareOutputs(baseline.get(image).blocks,runs[0].blocks);
   let maxConfidenceDelta=0;for(let i=0;i<runs[0].blocks.length;i++)maxConfidenceDelta=Math.max(maxConfidenceDelta,Math.abs(runs[0].blocks[i].confidence-baseline.get(image).blocks[i].confidence));
   const row={compact,image,loadMs:w.ready.load_ms,firstMs:runs[0].metrics.totalMs,p50Ms:quantile(runs.slice(1).map(r=>r.metrics.totalMs),.5),comparison,maxConfidenceDelta,runs:runs.map(r=>r.metrics)};
   report.results.push(row);fs.writeFileSync('docs/test-results/compact-ctc.json',JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify({compact,image,p50Ms:row.p50Ms,status:runs[0].metrics.pipeline.compactStatus,comparison,maxConfidenceDelta}));
   assert.equal(comparison.disagreement,false);assert(maxConfidenceDelta<1e-5);
  }
 }finally{await w.close();}
}
