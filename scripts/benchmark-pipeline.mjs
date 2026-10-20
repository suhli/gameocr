import fs from 'node:fs';import path from 'node:path';import assert from 'node:assert/strict';
import {engineConfig} from './executor-driver.mjs';import {startNative} from './onnx-worker-driver.mjs';import {compareOutputs,quantile} from './ocr-quality.mjs';
const outputs=new Map(),report=[];
for(const variant of ['before','after']){
  if(variant==='before')process.env.GAMEOCR_ONNX_WORKER=path.resolve('.cache/baseline-pipeline/onnx-worker.exe');else delete process.env.GAMEOCR_ONNX_WORKER;
  const worker=await startNative(engineConfig());
  try{
    assert.equal(worker.ready.type,'ready',JSON.stringify(worker.ready));
    // Alternate wide/narrow batches and image dimensions to expose stale padding.
    const samples=[];
    for(let cycle=0;cycle<5;cycle++)for(const image of ['tests/fixtures/small-text/dense-2560.png','tests/fixtures/english.png','tests/fixtures/blank.png']){
      const r=await worker.recognize(image);assert.equal(r.type,'result',JSON.stringify(r));const result=r.result;
      if(variant==='before')outputs.set(image,result.blocks);
      else {assert.equal(compareOutputs(outputs.get(image),result.blocks).disagreement,false);for(let i=0;i<result.blocks.length;i++)assert(Math.abs(result.blocks[i].confidence-outputs.get(image)[i].confidence)<1e-5);}
      if(cycle>0)samples.push({image,metrics:result.metrics});
    }
    for(const image of [...outputs.keys()]){
      const rows=samples.filter(r=>r.image===image);const row={variant,image,p50Ms:quantile(rows.map(r=>r.metrics.totalMs),.5),parsingMs:quantile(rows.map(r=>r.metrics.parsingMs),.5),preprocessingMs:quantile(rows.map(r=>r.metrics.preprocessingMs),.5)};report.push(row);console.log(JSON.stringify(row));
    }
  }finally{await worker.close();}
}
fs.writeFileSync('docs/test-results/pipeline.json',JSON.stringify({executedAt:new Date().toISOString(),report},null,2)+'\n');
