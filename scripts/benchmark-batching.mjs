import fs from 'node:fs';import assert from 'node:assert/strict';
import {engineConfig} from './executor-driver.mjs';import {startNative} from './onnx-worker-driver.mjs';import {accuracy,quantile} from './ocr-quality.mjs';
const corpus=JSON.parse(fs.readFileSync('tests/fixtures/small-text/corpus.json','utf8').replace(/^\uFEFF/,''));
const scene=corpus.scenarios.find(s=>s.width===2560);const report=[];
for(const widthBuckets of [false,true]){
  const engine=engineConfig(process.env.GAMEOCR_TEST_DEVICE??'cpu');Object.assign(engine.inference.parameters,{widthBuckets});
  const worker=await startNative(engine);
  try{
    assert.equal(worker.ready.type,'ready',JSON.stringify(worker.ready));const results=[];
    for(let n=0;n<6;n++){
      const response=await worker.recognize('tests/fixtures/small-text/'+scene.image);assert.equal(response.type,'result',JSON.stringify(response));
      const quality=accuracy(scene.groundTruth,response.result.blocks);assert.equal(quality.exactLineRecall,1);assert.equal(quality.unexpectedBlocks,0);assert.equal(quality.characterErrorRate,0);
      if(n)results.push(response.result);
    }
    const row={widthBuckets,device:engine.device,quality:accuracy(scene.groundTruth,results[0].blocks),medianMs:quantile(results.map(r=>r.metrics.totalMs),.5),p95Ms:quantile(results.map(r=>r.metrics.totalMs),.95),recognitionMs:quantile(results.map(r=>r.metrics.recognitionMs),.5),batches:results[0].metrics.recognitionBatches,pixels:results[0].metrics.recognitionPixels};
    report.push(row);console.log(JSON.stringify(row));
  }finally{await worker.close();}
}
fs.writeFileSync(`docs/test-results/batching-${process.env.GAMEOCR_TEST_DEVICE??'cpu'}.json`,JSON.stringify({executedAt:new Date().toISOString(),measurement:'Native adapter; excludes desktop capture/encoding',report},null,2)+'\n');
